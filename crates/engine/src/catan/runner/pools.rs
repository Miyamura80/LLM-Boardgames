//! Catan seat specifications and the agent factory. Bot kinds are validated
//! here at the game boundary (PRD-catan-evals decision #10 — config does not
//! accrete per-game bot taxonomies).

use crate::catan::agents::{GreedyBuilderBot, LlmSeatAgent, RandomLegalBot, SeatAgent};
use crate::llm::{ChatClient, ProviderKeys, RetryPolicy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The closed set of Catan seat-agent kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CatanAgentKind {
    RandomLegal,
    Greedy,
    Llm,
}

impl CatanAgentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CatanAgentKind::RandomLegal => "random-legal",
            CatanAgentKind::Greedy => "greedy",
            CatanAgentKind::Llm => "llm",
        }
    }

    /// Parse a config-provided kind string, failing before tokens are spent.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "random-legal" => Ok(Self::RandomLegal),
            "greedy" => Ok(Self::Greedy),
            "llm" => Ok(Self::Llm),
            other => Err(format!(
                "unknown catan agent kind '{other}' (expected random-legal | greedy | llm)"
            )),
        }
    }
}

/// One seat's player specification (candidate or anchor).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentSpec {
    /// Display name (anchor name from the pool, or the model string).
    pub name: String,
    pub kind: CatanAgentKind,
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
            kind: CatanAgentKind::Llm,
            model: Some(model.to_string()),
            persona: None,
            temperature: None,
            is_anchor: false,
        }
    }

    pub fn bot(kind: CatanAgentKind) -> Self {
        Self {
            name: format!("bot:{}", kind.as_str()),
            kind,
            model: None,
            persona: None,
            temperature: None,
            is_anchor: false,
        }
    }

    pub fn from_config(a: &app_config::CatanAnchorSpec) -> Result<Self, String> {
        Ok(Self {
            name: a.name.clone(),
            kind: CatanAgentKind::parse(&a.kind)?,
            model: a.model.clone(),
            persona: a.persona.clone(),
            temperature: a.temperature,
            is_anchor: true,
        })
    }

    /// The identity the rating layer scores: model string for LLM seats,
    /// `bot:<kind>` for scripted seats; anchors stay distinguishable.
    pub fn model_id(&self) -> String {
        match self.kind {
            CatanAgentKind::Llm if self.is_anchor => {
                let model = self.model.as_deref().unwrap_or(&self.name);
                format!("anchor:{}:{}", self.name, model)
            }
            CatanAgentKind::Llm => self.model.clone().unwrap_or_else(|| self.name.clone()),
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
    pub message_char_cap: usize,
}

impl AgentFactory {
    pub fn from_app_config(cfg: &app_config::AppConfig) -> Self {
        Self {
            keys: ProviderKeys::from_app_config(cfg),
            retry: RetryPolicy::from_app_config(cfg),
            temperature: cfg.catan.agent_temperature,
            max_tokens: cfg.catan.agent_max_tokens,
            message_char_cap: cfg.catan.utterance_char_cap,
        }
    }

    /// Instantiate the agent for one seat. `seat_seed` seeds bot RNGs so full
    /// bot games replay deterministically.
    pub fn build(&self, spec: &AgentSpec, seat_seed: u64) -> Result<Box<dyn SeatAgent>, String> {
        match spec.kind {
            CatanAgentKind::RandomLegal => Ok(Box::new(RandomLegalBot::new(seat_seed))),
            CatanAgentKind::Greedy => Ok(Box::new(GreedyBuilderBot::new())),
            CatanAgentKind::Llm => {
                let model = spec
                    .model
                    .as_deref()
                    .ok_or_else(|| format!("seat '{}' is kind=llm but has no model", spec.name))?;
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
                    self.message_char_cap,
                )))
            }
        }
    }
}
