//! LLM-backed Codenames seat: renders the observation, demands strict JSON,
//! maps replies onto engine actions. Parse failures surface as
//! [`AgentError::Malformed`] so the rethink loop can re-prompt; legality itself
//! is left to the engine, whose verbatim `IllegalMove` text is the feedback.

use super::{golden, prompts, AgentError, AgentReply, SeatAgent};
use crate::codenames::actions::{Action, DecisionPoint};
use crate::codenames::observation::Observation;
use crate::llm::{ChatClient, ChatMessage, TokenUsage};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct LlmSeatAgent {
    client: Arc<ChatClient>,
    /// Rating identity recorded on the seat (anchor-prefixed for anchors).
    rated_model_id: String,
    persona: Option<String>,
    temperature: f32,
    max_tokens: u32,
}

impl LlmSeatAgent {
    pub fn new(
        client: Arc<ChatClient>,
        rated_model_id: String,
        persona: Option<String>,
        temperature: f32,
        max_tokens: u32,
    ) -> Self {
        Self {
            client,
            rated_model_id,
            persona,
            temperature,
            max_tokens,
        }
    }

    fn system_prompt(&self, obs: &Observation) -> String {
        let mut s = format!(
            "{}\n\n{}\n\n{}",
            prompts::RULES_SUMMARY,
            prompts::role_brief(obs),
            prompts::OUTPUT_CONTRACT
        );
        if let Some(p) = &self.persona {
            s.push_str(&format!("\n\nPlay style directive: {p}"));
        }
        s
    }

    async fn ask(&self, system: String, user: String) -> Result<String, AgentError> {
        let messages = [ChatMessage::system(system), ChatMessage::user(user)];
        let outcome = self
            .client
            .chat(&messages, self.temperature, self.max_tokens)
            .await
            .map_err(|e| AgentError::Transport(e.to_string()))?;
        Ok(outcome.content)
    }
}

#[async_trait]
impl SeatAgent for LlmSeatAgent {
    fn kind(&self) -> &'static str {
        "llm"
    }
    fn model_id(&self) -> String {
        self.rated_model_id.clone()
    }
    fn scaffold_version(&self) -> String {
        golden::scaffold_version(self.persona.as_deref(), self.temperature)
    }
    fn usage(&self) -> TokenUsage {
        self.client.total_usage()
    }

    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        let mut user = format!(
            "{}\n== YOUR DECISION ==\n{}\nRespond with exactly this JSON shape:\n{}",
            prompts::render_observation(obs),
            prompts::decision_ask(obs, decision),
            prompts::decision_schema(decision),
        );
        if let Some(f) = feedback {
            user.push_str(&format!(
                "\n\nYour previous reply was rejected: {f}\nCorrect the problem and answer again with valid JSON."
            ));
        }
        let content = self.ask(self.system_prompt(obs), user).await?;
        let value = extract_json(&content)
            .ok_or_else(|| AgentError::Malformed("no JSON object found in reply".into()))?;
        parse_action(&value, decision)
    }
}

/// Pull the first parseable JSON object out of a completion (tolerates code
/// fences and prose around it). Every `{` is a candidate start.
fn extract_json(content: &str) -> Option<Value> {
    let trimmed = content.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }
    let mut search_from = 0;
    while let Some(offset) = trimmed[search_from..].find('{') {
        let start = search_from + offset;
        if let Some(v) = extract_balanced(trimmed, start) {
            return Some(v);
        }
        search_from = start + 1;
    }
    None
}

fn extract_balanced(text: &str, start: usize) -> Option<Value> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'"' if !escape => in_str = !in_str,
            b'\\' if in_str => {
                escape = !escape;
                continue;
            }
            b'{' if !in_str => depth += 1,
            b'}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&text[start..=i]).ok();
                }
            }
            _ => {}
        }
        escape = false;
    }
    None
}

/// Strict validation: known keys only, and the action kind must answer the
/// pending decision. Whether the clue or guess is *legal* is deliberately not
/// checked here — the engine is the sole authority and its rejection text is
/// what the rethink loop feeds back (PRD-codenames-evals §6).
fn parse_action(value: &Value, decision: &DecisionPoint) -> Result<AgentReply, AgentError> {
    let obj = value
        .as_object()
        .ok_or_else(|| AgentError::Malformed("reply must be a JSON object".into()))?;
    const ALLOWED: [&str; 4] = ["action", "word", "number", "thought_process"];
    if let Some(unknown) = obj.keys().find(|k| !ALLOWED.contains(&k.as_str())) {
        return Err(AgentError::Malformed(format!(
            "unknown field \"{unknown}\" (additionalProperties are rejected)"
        )));
    }
    let thought = obj
        .get("thought_process")
        .and_then(Value::as_str)
        .map(String::from);
    let mut clean = obj.clone();
    clean.remove("thought_process");
    let action: Action = serde_json::from_value(Value::Object(clean))
        .map_err(|e| AgentError::Malformed(format!("does not match the action schema: {e}")))?;

    let matches_kind = matches!(
        (&action, decision),
        (Action::GiveClue { .. }, DecisionPoint::GiveClue { .. })
            | (
                Action::Guess { .. } | Action::Pass,
                DecisionPoint::GuessOrPass { .. }
            )
    );
    if !matches_kind {
        return Err(AgentError::Malformed(format!(
            "action does not answer the pending decision ({})",
            decision.kind()
        )));
    }
    Ok(AgentReply { action, thought })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::types::{Clue, Team, SEAT_A_OPERATIVE, SEAT_A_SPYMASTER};

    fn clue_decision() -> DecisionPoint {
        DecisionPoint::GiveClue {
            seat: SEAT_A_SPYMASTER,
            team: Team::A,
            max_number: 9,
        }
    }

    fn guess_decision(guesses_used: u8) -> DecisionPoint {
        DecisionPoint::GuessOrPass {
            seat: SEAT_A_OPERATIVE,
            team: Team::A,
            clue: Clue {
                word: "signal".into(),
                number: 2,
            },
            guesses_used,
            guesses_remaining: 3 - guesses_used,
        }
    }

    #[test]
    fn extracts_json_from_fenced_and_chatty_replies() {
        let content = "Sure:\n```json\n{\"action\":\"pass\"}\n```";
        assert_eq!(extract_json(content).unwrap()["action"], "pass");
        let prose = "I think {this} is right: {\"action\":\"guess\",\"word\":\"anchor\"} — done.";
        assert_eq!(extract_json(prose).unwrap()["word"], "anchor");
        assert!(extract_json("no json here").is_none());
    }

    #[test]
    fn parses_a_clue_and_strips_the_thought() {
        let v: Value = serde_json::from_str(
            r#"{"action":"give_clue","word":"Ocean","number":3,"thought_process":"whale, boat, tide"}"#,
        )
        .unwrap();
        let reply = parse_action(&v, &clue_decision()).unwrap();
        assert_eq!(
            reply.action,
            Action::GiveClue {
                word: "Ocean".into(),
                number: 3
            },
            "normalization is the engine's job, not the parser's"
        );
        assert_eq!(reply.thought.as_deref(), Some("whale, boat, tide"));
    }

    #[test]
    fn rejects_unknown_fields_and_kind_mismatch() {
        let v: Value =
            serde_json::from_str(r#"{"action":"guess","word":"anchor","confidence":0.9}"#).unwrap();
        assert!(matches!(
            parse_action(&v, &guess_decision(0)),
            Err(AgentError::Malformed(m)) if m.contains("confidence")
        ));

        let v: Value = serde_json::from_str(r#"{"action":"pass"}"#).unwrap();
        assert!(matches!(
            parse_action(&v, &clue_decision()),
            Err(AgentError::Malformed(m)) if m.contains("give-clue")
        ));

        let v: Value = serde_json::from_str(r#"{"action":"give_clue","word":"ocean"}"#).unwrap();
        assert!(matches!(
            parse_action(&v, &clue_decision()),
            Err(AgentError::Malformed(_)),
        ));
        assert!(parse_action(&Value::String("pass".into()), &clue_decision()).is_err());
    }

    /// A pass before the mandatory first guess parses fine: it is an *illegal
    /// move*, not malformed output, and the engine must be the one to say so.
    #[test]
    fn a_premature_pass_parses_and_is_left_to_the_engine() {
        let v: Value = serde_json::from_str(r#"{"action":"pass"}"#).unwrap();
        let reply = parse_action(&v, &guess_decision(0)).unwrap();
        assert_eq!(reply.action, Action::Pass);
        assert!(reply.thought.is_none());
        assert!(parse_action(&v, &guess_decision(1)).is_ok());
    }
}
