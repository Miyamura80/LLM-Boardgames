//! Seat specifications and the agent factory: turns a frozen anchor/candidate
//! spec into a live [`SeatAgent`].

use crate::llm::{ChatClient, ProviderKeys, RetryPolicy};
use crate::secret_hitler::agents::{
    BayesHistoryBot, HeuristicBot, LlmSeatAgent, RandomLegalBot, SeatAgent,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One seat's player specification (candidate or anchor).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentSpec {
    /// Display name (anchor name from the pool, or the model string).
    pub name: String,
    /// `random-legal` | `heuristic` | `bayes-history` | `llm`.
    pub kind: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub persona: Option<String>,
    #[serde(default)]
    pub is_anchor: bool,
}

impl AgentSpec {
    pub fn llm(model: &str) -> Self {
        Self {
            name: model.to_string(),
            kind: "llm".into(),
            model: Some(model.to_string()),
            persona: None,
            is_anchor: false,
        }
    }

    pub fn bot(kind: &str) -> Self {
        Self {
            name: format!("bot:{kind}"),
            kind: kind.to_string(),
            model: None,
            persona: None,
            is_anchor: false,
        }
    }

    pub fn from_anchor(a: &app_config::AnchorSpec) -> Self {
        Self {
            name: a.name.clone(),
            kind: a.kind.clone(),
            model: a.model.clone(),
            persona: a.persona.clone(),
            is_anchor: true,
        }
    }

    /// The identity the rating layer scores: model string for LLM seats,
    /// `bot:<kind>` for scripted seats (persona anchors stay distinguishable
    /// via their own model+persona scaffold hash).
    pub fn model_id(&self) -> String {
        match self.kind.as_str() {
            "llm" => self.model.clone().unwrap_or_else(|| self.name.clone()),
            k => format!("bot:{k}"),
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
        match spec.kind.as_str() {
            "random-legal" => Ok(Box::new(RandomLegalBot::new(seat_seed))),
            "heuristic" => Ok(Box::new(HeuristicBot::new())),
            "bayes-history" => Ok(Box::new(BayesHistoryBot::new())),
            "llm" => {
                let model = spec.model.as_deref().ok_or_else(|| {
                    format!("anchor '{}' is kind=llm but has no model", spec.name)
                })?;
                let client = Arc::new(ChatClient::new(
                    model,
                    self.keys.clone(),
                    self.retry.clone(),
                ));
                Ok(Box::new(LlmSeatAgent::new(
                    client,
                    spec.persona.clone(),
                    self.temperature,
                    self.max_tokens,
                    self.utterance_char_cap,
                )))
            }
            other => Err(format!("unknown agent kind: {other}")),
        }
    }
}
