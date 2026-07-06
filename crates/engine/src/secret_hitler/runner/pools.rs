//! Seat specifications and the agent factory: turns a frozen anchor/candidate
//! spec into a live [`SeatAgent`].

use crate::llm::{ChatClient, ProviderKeys, RetryPolicy};
use crate::secret_hitler::agents::{
    BayesHistoryBot, HeuristicBot, LlmSeatAgent, RandomLegalBot, SeatAgent,
};
use app_config::AgentKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One seat's player specification (candidate or anchor).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentSpec {
    /// Display name (anchor name from the pool, or the model string).
    pub name: String,
    pub kind: AgentKind,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub persona: Option<String>,
    /// Frozen sampling temperature; `None` uses the configured default.
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub is_anchor: bool,
}

impl AgentSpec {
    pub fn llm(model: &str) -> Self {
        Self {
            name: model.to_string(),
            kind: AgentKind::Llm,
            model: Some(model.to_string()),
            persona: None,
            temperature: None,
            is_anchor: false,
        }
    }

    pub fn bot(kind: AgentKind) -> Self {
        Self {
            name: format!("bot:{}", kind.as_str()),
            kind,
            model: None,
            persona: None,
            temperature: None,
            is_anchor: false,
        }
    }

    pub fn from_anchor(a: &app_config::AnchorSpec) -> Self {
        Self {
            name: a.name.clone(),
            kind: a.kind,
            model: a.model.clone(),
            persona: a.persona.clone(),
            temperature: a.temperature,
            is_anchor: true,
        }
    }

    /// The identity the rating layer scores: model string for LLM seats,
    /// `bot:<kind>` for scripted seats (persona anchors stay distinguishable
    /// via their own model+persona scaffold hash).
    pub fn model_id(&self) -> String {
        match self.kind {
            // LLM anchors are distinct rated entities per pool slot: the same
            // base model with a different persona/temperature is a different
            // (frozen) player, and an anchor must never merge with a
            // candidate that happens to share its model string.
            AgentKind::Llm if self.is_anchor => {
                let model = self.model.as_deref().unwrap_or(&self.name);
                format!("anchor:{}:{}", self.name, model)
            }
            AgentKind::Llm => self.model.clone().unwrap_or_else(|| self.name.clone()),
            kind => format!("bot:{}", kind.as_str()),
        }
    }
}

/// Builds live agents; carries provider keys and per-call settings.
#[derive(Clone)]
pub struct AgentFactory {
    pub keys: ProviderKeys,
    pub retry: RetryPolicy,
    pub temperature: f32,
    pub max_tokens: u32,
    pub utterance_char_cap: usize,
}

impl AgentFactory {
    pub fn from_app_config(cfg: &app_config::AppConfig) -> Self {
        Self {
            keys: ProviderKeys::from_app_config(cfg),
            retry: RetryPolicy::from_app_config(cfg),
            temperature: cfg.secret_hitler.agent_temperature,
            max_tokens: cfg.secret_hitler.agent_max_tokens,
            utterance_char_cap: cfg.secret_hitler.utterance_char_cap,
        }
    }

    /// Instantiate the agent for one seat. `seat_seed` seeds bot RNGs so full
    /// bot games replay deterministically.
    pub fn build(&self, spec: &AgentSpec, seat_seed: u64) -> Result<Box<dyn SeatAgent>, String> {
        match spec.kind {
            AgentKind::RandomLegal => Ok(Box::new(RandomLegalBot::new(seat_seed))),
            AgentKind::Heuristic => Ok(Box::new(HeuristicBot::new())),
            AgentKind::BayesHistory => Ok(Box::new(BayesHistoryBot::new())),
            AgentKind::Llm => {
                let model = spec.model.as_deref().ok_or_else(|| {
                    format!("anchor '{}' is kind=llm but has no model", spec.name)
                })?;
                // Catch config typos before a game burns tokens on them:
                // providers disagree on how they handle out-of-range values.
                let temperature = spec.temperature.unwrap_or(self.temperature);
                if !(0.0..=2.0).contains(&temperature) {
                    return Err(format!(
                        "seat '{}' has temperature {temperature}, outside the supported 0.0..=2.0",
                        spec.name
                    ));
                }
                let client = Arc::new(ChatClient::new(
                    model,
                    self.keys.clone(),
                    self.retry.clone(),
                ));
                Ok(Box::new(LlmSeatAgent::new(
                    client,
                    spec.model_id(),
                    spec.persona.clone(),
                    temperature,
                    self.max_tokens,
                    self.utterance_char_cap,
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn factory() -> AgentFactory {
        AgentFactory {
            keys: ProviderKeys::default(),
            retry: RetryPolicy::default(),
            temperature: 0.5,
            max_tokens: 256,
            utterance_char_cap: 240,
        }
    }

    fn llm_anchor(temperature: Option<f32>) -> AgentSpec {
        AgentSpec {
            name: "aggressive".into(),
            kind: AgentKind::Llm,
            model: Some("gemini/gemini-3-flash-preview".into()),
            persona: Some("accuse loudly".into()),
            temperature,
            is_anchor: true,
        }
    }

    /// The rated identity on the live agent must be the anchor-prefixed key,
    /// not the raw model string — otherwise an anchor merges with a candidate
    /// that happens to share its model.
    #[test]
    fn built_anchor_keeps_its_rating_identity() {
        let spec = llm_anchor(None);
        let agent = factory().build(&spec, 7).unwrap();
        assert_eq!(agent.model_id(), spec.model_id());
        assert_eq!(
            agent.model_id(),
            "anchor:aggressive:gemini/gemini-3-flash-preview"
        );
    }

    #[test]
    fn out_of_range_temperature_is_rejected() {
        for bad in [-0.1_f32, 2.5, f32::NAN] {
            assert!(factory().build(&llm_anchor(Some(bad)), 7).is_err());
        }
        assert!(factory().build(&llm_anchor(Some(1.2)), 7).is_ok());
    }
}
