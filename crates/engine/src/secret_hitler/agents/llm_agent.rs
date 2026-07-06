//! LLM-backed seat agent: renders the observation into a prompt, demands
//! strict JSON, and maps replies onto engine actions. Parse failures surface
//! as [`AgentError::Malformed`] so the game loop can re-prompt within the
//! retry budget (the rethink loop).

use super::prompts;
use super::{AgentError, AgentReply, BeliefReport, RoleProbs, SeatAgent, SpeechReply};
use crate::llm::{ChatClient, ChatMessage, TokenUsage};
use crate::secret_hitler::actions::{Action, DecisionPoint};
use crate::secret_hitler::observation::Observation;
use crate::secret_hitler::types::Seat;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct LlmSeatAgent {
    client: Arc<ChatClient>,
    /// Rating identity recorded on the seat — `anchor:{name}:{model}` for
    /// anchors, the raw model string for candidates. Distinct from the
    /// client's model string, which only addresses the provider API.
    rated_model_id: String,
    persona: Option<String>,
    temperature: f32,
    max_tokens: u32,
    /// Hard cap on utterance length, in characters (enforced by truncation).
    utterance_char_cap: usize,
}

impl LlmSeatAgent {
    pub fn new(
        client: Arc<ChatClient>,
        rated_model_id: String,
        persona: Option<String>,
        temperature: f32,
        max_tokens: u32,
        utterance_char_cap: usize,
    ) -> Self {
        Self {
            client,
            rated_model_id,
            persona,
            temperature,
            max_tokens,
            utterance_char_cap,
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
        prompts::scaffold_version(self.persona.as_deref(), self.temperature)
    }
    fn can_speak(&self) -> bool {
        true
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
            "{}\n== YOUR DECISION ==\nYou must decide now: {}\nRespond with exactly this JSON shape:\n{}",
            prompts::render_observation(obs),
            decision_ask(decision),
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

    async fn speak(
        &mut self,
        obs: &Observation,
        discussion_round: u8,
    ) -> Result<SpeechReply, AgentError> {
        let user = format!(
            "{}\n== DISCUSSION ==\nSimultaneous discussion round {} before the vote. All players write at once; nobody sees this round's messages until everyone has written. Keep it under {} characters.\nRespond with exactly one of:\n{}",
            prompts::render_observation(obs),
            discussion_round + 1,
            self.utterance_char_cap,
            prompts::SPEECH_SCHEMA,
        );
        let content = self.ask(self.system_prompt(obs), user).await?;
        let value = extract_json(&content)
            .ok_or_else(|| AgentError::Malformed("no JSON object found in reply".into()))?;
        // Same strictness as decisions: unknown fields are rejected.
        if let Some(obj) = value.as_object() {
            const ALLOWED: [&str; 3] = ["message", "pass", "thought_process"];
            if let Some(unknown) = obj.keys().find(|k| !ALLOWED.contains(&k.as_str())) {
                return Err(AgentError::Malformed(format!(
                    "unknown field \"{unknown}\" (additionalProperties are rejected)"
                )));
            }
        }
        if value.get("pass").and_then(Value::as_bool) == Some(true) {
            return Ok(SpeechReply { text: None });
        }
        let msg = value
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AgentError::Malformed("expected {\"message\": string} or {\"pass\": true}".into())
            })?;
        let mut text = msg.trim().to_string();
        if text.is_empty() {
            return Ok(SpeechReply { text: None });
        }
        if text.chars().count() > self.utterance_char_cap {
            text = text.chars().take(self.utterance_char_cap).collect();
        }
        Ok(SpeechReply { text: Some(text) })
    }

    async fn beliefs(&mut self, obs: &Observation) -> Result<Option<BeliefReport>, AgentError> {
        let user = format!(
            "{}\n== PRIVATE BELIEF CHECK ==\n{}",
            prompts::render_observation(obs),
            prompts::BELIEFS_INSTRUCTION,
        );
        let content = self.ask(self.system_prompt(obs), user).await?;
        let value = extract_json(&content)
            .ok_or_else(|| AgentError::Malformed("no JSON object found in reply".into()))?;
        let beliefs = value
            .get("beliefs")
            .and_then(Value::as_object)
            .ok_or_else(|| AgentError::Malformed("missing \"beliefs\" object".into()))?;

        let mut assessments: BTreeMap<Seat, RoleProbs> = BTreeMap::new();
        for (k, v) in beliefs {
            let Ok(seat) = k.trim_start_matches("Player").trim().parse::<Seat>() else {
                continue;
            };
            let get = |field: &str| v.get(field).and_then(Value::as_f64).unwrap_or(0.0);
            assessments.insert(
                seat,
                RoleProbs {
                    liberal: get("liberal"),
                    fascist: get("fascist"),
                    hitler: get("hitler"),
                }
                .normalized(),
            );
        }
        // Fill any missing living opponents so scoring is total: known
        // teammates by their actual role, everyone else by the role prior
        // (which assumes the seat is NOT covered by private knowledge).
        let known: BTreeMap<Seat, crate::secret_hitler::types::Role> =
            obs.known_teammates.iter().copied().collect();
        let prior = super::prior_for_observer(obs.role);
        for &s in obs.public.alive.iter().filter(|&&s| s != obs.seat) {
            let fallback = known
                .get(&s)
                .map(|&role| RoleProbs::certain(role))
                .unwrap_or(prior);
            assessments.entry(s).or_insert(fallback);
        }
        assessments.retain(|s, _| *s != obs.seat && obs.public.alive.contains(s));
        Ok(Some(BeliefReport { assessments }))
    }
}

fn decision_ask(decision: &DecisionPoint) -> String {
    match decision {
        DecisionPoint::Nominate { eligible, .. } => {
            format!("As President, nominate a Chancellor from {eligible:?}.")
        }
        DecisionPoint::Vote { nominee, .. } => {
            format!("Vote Ja or Nein on electing Player {nominee} as Chancellor.")
        }
        DecisionPoint::Discard { tiles, .. } => {
            format!("As President you drew {tiles:?}. Discard one tile (hidden); the other two go to the Chancellor.")
        }
        DecisionPoint::Enact { tiles, can_veto, .. } => {
            let mut s = format!("As Chancellor you received {tiles:?}. Enact one tile.");
            if *can_veto {
                s.push_str(" Veto is unlocked: you may instead propose discarding both.");
            }
            s
        }
        DecisionPoint::VetoConsent { .. } => {
            "The Chancellor proposed a veto. Approve (discard both tiles, tracker advances) or decline (Chancellor must enact).".into()
        }
        DecisionPoint::UsePower { power, targets, .. } => {
            format!("Use your presidential power {power:?} on one of {targets:?}.")
        }
    }
}

/// Pull the first parseable JSON object out of a completion (tolerates code
/// fences and prose around it, tolerates nothing inside it). Every `{` is a
/// candidate start: prose containing a stray brace before the real payload
/// must not eat the reply.
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

/// Parse the brace-balanced block starting at `start`, if it is valid JSON.
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

/// Strict validation: known keys only, action type must match the decision.
fn parse_action(value: &Value, decision: &DecisionPoint) -> Result<AgentReply, AgentError> {
    let obj = value
        .as_object()
        .ok_or_else(|| AgentError::Malformed("reply must be a JSON object".into()))?;
    const ALLOWED: [&str; 6] = [
        "action",
        "target",
        "policy",
        "ja",
        "approve",
        "thought_process",
    ];
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
        (Action::Nominate { .. }, DecisionPoint::Nominate { .. })
            | (Action::Vote { .. }, DecisionPoint::Vote { .. })
            | (Action::Discard { .. }, DecisionPoint::Discard { .. })
            | (Action::Enact { .. }, DecisionPoint::Enact { .. })
            | (
                Action::ProposeVeto,
                DecisionPoint::Enact { can_veto: true, .. }
            )
            | (
                Action::VetoConsent { .. },
                DecisionPoint::VetoConsent { .. }
            )
            | (Action::UsePower { .. }, DecisionPoint::UsePower { .. })
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
    use crate::secret_hitler::types::Party;

    #[test]
    fn extracts_json_from_fenced_reply() {
        let content = "Here you go:\n```json\n{\"action\":\"vote\",\"ja\":true}\n```";
        let v = extract_json(content).unwrap();
        assert_eq!(v["action"], "vote");
    }

    #[test]
    fn rejects_unknown_fields() {
        let v: Value = serde_json::from_str(r#"{"action":"vote","ja":true,"extra":1}"#).unwrap();
        let d = DecisionPoint::Vote {
            seat: 0,
            nominee: 1,
        };
        assert!(matches!(
            parse_action(&v, &d),
            Err(AgentError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_action_kind_mismatch() {
        let v: Value = serde_json::from_str(r#"{"action":"vote","ja":true}"#).unwrap();
        let d = DecisionPoint::Discard {
            president: 0,
            tiles: vec![Party::Liberal],
        };
        assert!(matches!(
            parse_action(&v, &d),
            Err(AgentError::Malformed(_))
        ));
    }

    #[test]
    fn accepts_valid_action_with_thought() {
        let v: Value =
            serde_json::from_str(r#"{"action":"nominate","target":3,"thought_process":"trust 3"}"#)
                .unwrap();
        let d = DecisionPoint::Nominate {
            president: 0,
            eligible: vec![1, 2, 3],
        };
        let reply = parse_action(&v, &d).unwrap();
        assert_eq!(reply.action, Action::Nominate { target: 3 });
        assert_eq!(reply.thought.as_deref(), Some("trust 3"));
    }

    #[test]
    fn veto_only_allowed_when_unlocked() {
        let v: Value = serde_json::from_str(r#"{"action":"propose_veto"}"#).unwrap();
        let locked = DecisionPoint::Enact {
            chancellor: 0,
            tiles: vec![Party::Fascist, Party::Fascist],
            can_veto: false,
        };
        assert!(parse_action(&v, &locked).is_err());
        let unlocked = DecisionPoint::Enact {
            chancellor: 0,
            tiles: vec![Party::Fascist, Party::Fascist],
            can_veto: true,
        };
        assert!(parse_action(&v, &unlocked).is_ok());
    }
}
