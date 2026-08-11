//! LLM-backed Catan seat: renders the observation, demands strict JSON, maps
//! replies onto engine actions. Parse failures surface as
//! [`AgentError::Malformed`] so the rethink loop can re-prompt.

use super::{prompts, AgentError, AgentReply, SeatAgent};
use crate::catan::actions::{Action, DecisionPoint};
use crate::catan::observation::Observation;
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
    /// Hard cap on public message length, in characters (enforced by
    /// truncation).
    message_char_cap: usize,
}

impl LlmSeatAgent {
    pub fn new(
        client: Arc<ChatClient>,
        rated_model_id: String,
        persona: Option<String>,
        temperature: f32,
        max_tokens: u32,
        message_char_cap: usize,
    ) -> Self {
        Self {
            client,
            rated_model_id,
            persona,
            temperature,
            max_tokens,
            message_char_cap,
        }
    }

    fn system_prompt(&self, obs: &Observation) -> String {
        let mut s = format!(
            "{}\n\n{}\n\n{}",
            prompts::RULES_SUMMARY,
            prompts::seat_brief(obs),
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

    fn cap_messages(&self, action: &mut Action) {
        let cap = self.message_char_cap;
        let trim = |m: &mut Option<String>| {
            if let Some(text) = m {
                if text.chars().count() > cap {
                    *text = text.chars().take(cap).collect();
                }
            }
        };
        match action {
            Action::ProposeTrade { message, .. }
            | Action::AcceptTrade { message }
            | Action::RejectTrade { message }
            | Action::CounterTrade { message, .. } => trim(message),
            Action::Say { message } => {
                if message.chars().count() > cap {
                    *message = message.chars().take(cap).collect();
                }
            }
            _ => {}
        }
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
        let mut reply = parse_action(&value, decision)?;
        self.cap_messages(&mut reply.action);
        Ok(reply)
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

/// Strict validation: known keys only, action kind must answer the decision.
fn parse_action(value: &Value, decision: &DecisionPoint) -> Result<AgentReply, AgentError> {
    let obj = value
        .as_object()
        .ok_or_else(|| AgentError::Malformed("reply must be a JSON object".into()))?;
    const ALLOWED: [&str; 15] = [
        "action",
        "vertex",
        "edge",
        "hex",
        "seat",
        "resources",
        "give",
        "receive",
        "to",
        "message",
        "first",
        "second",
        "resource",
        "partner",
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
        (
            Action::PlaceSetupSettlement { .. },
            DecisionPoint::SetupSettlement { .. }
        ) | (
            Action::PlaceSetupRoad { .. },
            DecisionPoint::SetupRoad { .. }
        ) | (Action::Roll, DecisionPoint::PreRoll { .. })
            | (
                Action::PlayKnight,
                DecisionPoint::PreRoll {
                    can_play_knight: true,
                    ..
                }
            )
            | (Action::Discard { .. }, DecisionPoint::DiscardHalf { .. })
            | (Action::MoveRobber { .. }, DecisionPoint::MoveRobber { .. })
            | (Action::StealFrom { .. }, DecisionPoint::ChooseVictim { .. })
            | (Action::BuildRoad { .. }, DecisionPoint::FreeRoad { .. })
            | (
                Action::BuildRoad { .. }
                    | Action::BuildSettlement { .. }
                    | Action::BuildCity { .. }
                    | Action::BuyDev
                    | Action::PlayKnight
                    | Action::PlayRoadBuilding
                    | Action::PlayYearOfPlenty { .. }
                    | Action::PlayMonopoly { .. }
                    | Action::BankTrade { .. }
                    | Action::ProposeTrade { .. }
                    | Action::Say { .. }
                    | Action::EndTurn,
                DecisionPoint::TurnAction { .. }
            )
            | (
                Action::AcceptTrade { .. }
                    | Action::RejectTrade { .. }
                    | Action::CounterTrade { .. },
                DecisionPoint::RespondTrade { .. }
            )
            | (
                Action::ResolveTrade { .. },
                DecisionPoint::ResolveTrade { .. }
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
    use crate::catan::events::TradeOffer;
    use crate::catan::types::{Resource, ResourceSet};

    #[test]
    fn extracts_json_from_fenced_reply() {
        let content = "Sure:\n```json\n{\"action\":\"roll\"}\n```";
        assert_eq!(extract_json(content).unwrap()["action"], "roll");
    }

    #[test]
    fn rejects_unknown_fields_and_kind_mismatch() {
        let d = DecisionPoint::TurnAction { seat: 0 };
        let v: Value = serde_json::from_str(r#"{"action":"end_turn","bonus":1}"#).unwrap();
        assert!(matches!(
            parse_action(&v, &d),
            Err(AgentError::Malformed(_))
        ));
        let v: Value = serde_json::from_str(r#"{"action":"roll"}"#).unwrap();
        assert!(matches!(
            parse_action(&v, &d),
            Err(AgentError::Malformed(_))
        ));
    }

    #[test]
    fn knight_before_roll_needs_a_playable_knight() {
        let v: Value = serde_json::from_str(r#"{"action":"play_knight"}"#).unwrap();
        let no = DecisionPoint::PreRoll {
            seat: 0,
            can_play_knight: false,
        };
        assert!(parse_action(&v, &no).is_err());
        let yes = DecisionPoint::PreRoll {
            seat: 0,
            can_play_knight: true,
        };
        assert!(parse_action(&v, &yes).is_ok());
    }

    #[test]
    fn accepts_trade_actions_with_thought_and_resource_sets() {
        let d = DecisionPoint::TurnAction { seat: 0 };
        let v: Value = serde_json::from_str(
            r#"{"action":"propose_trade","give":{"brick":1},"receive":{"ore":1},"to":2,"message":"ore for brick?","thought_process":"need ore"}"#,
        )
        .unwrap();
        let reply = parse_action(&v, &d).unwrap();
        match reply.action {
            Action::ProposeTrade {
                give, receive, to, ..
            } => {
                assert_eq!(give, ResourceSet::of(Resource::Brick, 1));
                assert_eq!(receive, ResourceSet::of(Resource::Ore, 1));
                assert_eq!(to, Some(2));
            }
            a => panic!("unexpected action {a:?}"),
        }
        assert_eq!(reply.thought.as_deref(), Some("need ore"));

        let d = DecisionPoint::RespondTrade {
            seat: 1,
            proposer: 0,
            offer: TradeOffer {
                give: ResourceSet::of(Resource::Brick, 1),
                receive: ResourceSet::of(Resource::Ore, 1),
            },
        };
        let v: Value = serde_json::from_str(
            r#"{"action":"counter_trade","give":{"ore":1},"receive":{"brick":2}}"#,
        )
        .unwrap();
        assert!(parse_action(&v, &d).is_ok());
    }
}
