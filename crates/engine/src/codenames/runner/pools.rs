//! Codenames seat specifications and the agent factory. Bot kinds are
//! validated here at the game boundary (PRD-codenames-evals US-CN06; Catan
//! decision #10 — config does not accrete per-game bot taxonomies).

use crate::codenames::agents::{
    CodenamesAgentKind, EmbeddingGreedyBot, LlmSeatAgent, RandomLegalBot, SeatAgent, VectorTable,
};
use crate::codenames::types::GameConfig as RulesConfig;
use crate::codenames::wordlist::Wordlist;
use crate::llm::{ChatClient, ProviderKeys, RetryPolicy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One seat's player specification (candidate or anchor).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentSpec {
    /// Display name (anchor name from the pool, or the model string).
    pub name: String,
    pub kind: CodenamesAgentKind,
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
            kind: CodenamesAgentKind::Llm,
            model: Some(model.to_string()),
            persona: None,
            temperature: None,
            is_anchor: false,
        }
    }

    pub fn bot(kind: CodenamesAgentKind) -> Self {
        Self {
            name: format!("bot:{}", kind.as_str()),
            kind,
            model: None,
            persona: None,
            temperature: None,
            is_anchor: false,
        }
    }

    pub fn from_config(a: &app_config::CodenamesAnchorSpec) -> Result<Self, String> {
        Ok(Self {
            name: a.name.clone(),
            kind: CodenamesAgentKind::parse(&a.kind)?,
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
            CodenamesAgentKind::Llm if self.is_anchor => {
                let model = self.model.as_deref().unwrap_or(&self.name);
                format!("anchor:{}:{}", self.name, model)
            }
            CodenamesAgentKind::Llm => self.model.clone().unwrap_or_else(|| self.name.clone()),
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
    /// The word-vector table backing `codenames-embedding` seats. Loaded once
    /// per run and shared across every seat that asks for it (`Arc`), because a
    /// real vector subset is megabytes and four seats must not each hold one.
    /// `None` when no `codenames.vectors_path` is configured — then the kind is
    /// rejected at [`Self::build`] rather than silently substituted.
    pub vectors: Option<Arc<VectorTable>>,
}

impl AgentFactory {
    /// Build the factory from config, loading the vector table eagerly so a
    /// broken `vectors_path` fails here, before any tokens are spent (the same
    /// posture as [`wordlist_from_config`]).
    pub fn from_app_config(cfg: &app_config::AppConfig) -> Result<Self, String> {
        Ok(Self {
            keys: ProviderKeys::from_app_config(cfg),
            retry: RetryPolicy::from_app_config(cfg),
            temperature: cfg.codenames.agent_temperature,
            max_tokens: cfg.codenames.agent_max_tokens,
            vectors: vectors_from_config(&cfg.codenames)?,
        })
    }

    /// Instantiate the agent for one seat. `seat_seed` seeds bot RNGs so full
    /// bot games replay deterministically.
    pub fn build(&self, spec: &AgentSpec, seat_seed: u64) -> Result<Box<dyn SeatAgent>, String> {
        match spec.kind {
            CodenamesAgentKind::Random => Ok(Box::new(RandomLegalBot::new(seat_seed))),
            // The embedding anchor needs a vector table. Without one the
            // boundary rejects the seat rather than silently substituting
            // another agent, so a record never names an anchor that did not
            // play. The bot itself is deterministic and ignores `seat_seed`.
            CodenamesAgentKind::Embedding => match &self.vectors {
                Some(vectors) => Ok(Box::new(EmbeddingGreedyBot::new(vectors.clone()))),
                None => Err(format!(
                    "seat '{}' asks for the codenames-embedding anchor, but no \
                     codenames.vectors_path is configured (set one, or use codenames-random \
                     or an llm seat)",
                    spec.name
                )),
            },
            CodenamesAgentKind::Llm => {
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
                )))
            }
        }
    }
}

/// The rules knobs a game runs under, from config.
pub fn rules_from_config(cfg: &app_config::CodenamesConfig) -> RulesConfig {
    RulesConfig {
        clue_word_max_len: cfg.clue_word_max_len,
    }
}

/// The configured pool: the `wordlist_path` override if set, else the vendored
/// curation. A broken override fails here, before any tokens are spent.
pub fn wordlist_from_config(cfg: &app_config::CodenamesConfig) -> Result<Wordlist, String> {
    match &cfg.wordlist_path {
        None => Ok(Wordlist::default_embedded()),
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("codenames wordlist_path '{path}': {e}"))?;
            Wordlist::parse(&text).map_err(|e| format!("codenames wordlist_path '{path}': {e}"))
        }
    }
}

/// The configured vector table for the embedding anchor, loaded once. `None`
/// when no path is set; a set-but-broken path is an error, never a silent
/// downgrade to "no anchor available".
pub fn vectors_from_config(
    cfg: &app_config::CodenamesConfig,
) -> Result<Option<Arc<VectorTable>>, String> {
    match &cfg.vectors_path {
        None => Ok(None),
        Some(path) => VectorTable::from_path(path)
            .map(|t| Some(Arc::new(t)))
            .map_err(|e| format!("codenames vectors_path '{path}': {e}")),
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
            vectors: None,
        }
    }

    /// `Box<dyn SeatAgent>` is not `Debug`, so `expect_err` is unavailable.
    fn build_err(spec: &AgentSpec) -> String {
        match factory().build(spec, 1) {
            Ok(_) => panic!("expected seat '{}' to be rejected", spec.name),
            Err(e) => e,
        }
    }

    #[test]
    fn bot_seats_build_and_carry_their_rated_identity() {
        let spec = AgentSpec::bot(CodenamesAgentKind::Random);
        assert_eq!(spec.model_id(), "bot:codenames-random");
        let agent = factory().build(&spec, 7).expect("random bot builds");
        assert_eq!(agent.kind(), "bot:codenames-random");
        assert_eq!(agent.model_id(), "bot:codenames-random");
    }

    /// Without a configured vector table the embedding anchor is rejected by
    /// name instead of quietly becoming a different agent; with one it builds
    /// and shares the table.
    #[test]
    fn the_embedding_anchor_needs_a_configured_vector_table() {
        let spec = AgentSpec::bot(CodenamesAgentKind::Embedding);
        let err = build_err(&spec);
        assert!(
            err.contains("no codenames.vectors_path is configured"),
            "{err}"
        );
        assert!(err.contains("codenames-random"), "{err}");

        let table = VectorTable::parse("alfa 1 0 0\nbravo 0 1 0\n").expect("tiny table parses");
        let f = AgentFactory {
            vectors: Some(Arc::new(table)),
            ..factory()
        };
        let agent = f.build(&spec, 1).expect("embedding anchor builds");
        assert_eq!(agent.model_id(), "bot:codenames-embedding");
    }

    /// A configured-but-broken path is a load error, not a silent `None`.
    #[test]
    fn a_broken_vectors_path_fails_loudly() {
        let cfg = app_config::CodenamesConfig {
            vectors_path: Some("/nonexistent/codenames_vectors.txt".into()),
            ..app_config::CodenamesConfig::default()
        };
        let err = vectors_from_config(&cfg).expect_err("missing file");
        assert!(err.contains("vectors_path"), "{err}");
        assert!(vectors_from_config(&app_config::CodenamesConfig::default())
            .expect("no path configured")
            .is_none());
    }

    #[test]
    fn llm_seats_need_a_model_and_a_sane_temperature() {
        let mut spec = AgentSpec::llm("gemini/gemini-3-flash-preview");
        assert!(factory().build(&spec, 0).is_ok());
        assert_eq!(spec.model_id(), "gemini/gemini-3-flash-preview");

        spec.temperature = Some(9.0);
        let err = build_err(&spec);
        assert!(err.contains("outside the supported"), "{err}");

        let bare = AgentSpec {
            name: "nameless".into(),
            kind: CodenamesAgentKind::Llm,
            model: None,
            persona: None,
            temperature: None,
            is_anchor: false,
        };
        assert!(
            build_err(&bare).contains("no model"),
            "{}",
            build_err(&bare)
        );
    }

    #[test]
    fn anchor_specs_come_from_config_and_stay_distinguishable() {
        let cfg = app_config::CodenamesAnchorSpec {
            name: "frozen-flash".into(),
            kind: "llm".into(),
            model: Some("gemini/gemini-3-flash-preview".into()),
            persona: None,
            temperature: Some(0.2),
        };
        let spec = AgentSpec::from_config(&cfg).expect("valid anchor");
        assert!(spec.is_anchor);
        assert_eq!(
            spec.model_id(),
            "anchor:frozen-flash:gemini/gemini-3-flash-preview"
        );

        let bad = app_config::CodenamesAnchorSpec {
            kind: "random-legal".into(),
            ..cfg
        };
        assert!(AgentSpec::from_config(&bad)
            .expect_err("catan's kind name")
            .contains("unknown codenames agent kind"));
    }

    #[test]
    fn config_supplies_the_rules_knobs_and_the_default_pool() {
        let cfg = app_config::CodenamesConfig::default();
        assert_eq!(
            rules_from_config(&cfg).clue_word_max_len,
            cfg.clue_word_max_len
        );
        let list = wordlist_from_config(&cfg).expect("vendored pool");
        assert_eq!(
            list.content_hash(),
            Wordlist::default_embedded().content_hash()
        );

        let missing = app_config::CodenamesConfig {
            wordlist_path: Some("/nonexistent/codenames_words.txt".into()),
            ..app_config::CodenamesConfig::default()
        };
        assert!(wordlist_from_config(&missing)
            .expect_err("missing file")
            .contains("wordlist_path"));
    }
}
