//! Codenames harness configuration (own file, per PRD-codenames-evals
//! decision 11; follows the `catan.rs` split-file pattern).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Codenames eval harness settings (see `docs/PRD-codenames-evals.md`). Bot
/// kinds are plain strings here and validated by the codenames module at the
/// game boundary (Catan decision #10: config does not accrete per-game bot
/// taxonomies).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CodenamesConfig {
    /// Rethink attempts per decision before the forced legal default.
    #[serde(default = "super::default_retry_budget")]
    pub retry_budget: u32,
    /// Max completion tokens per agent call.
    #[serde(default = "super::default_agent_max_tokens")]
    pub agent_max_tokens: u32,
    /// Sampling temperature for LLM seats (part of the scaffold version).
    #[serde(default = "super::default_agent_temperature")]
    pub agent_temperature: f32,
    /// Conservatism factor k in the reported score μ − kσ.
    #[serde(default = "super::default_rating_k")]
    pub rating_k: f64,
    /// Path to an alternate wordlist, overriding the vendored curation
    /// (`None` → the vendored ~400-word list, whose hash is recorded per game).
    #[serde(default)]
    pub wordlist_path: Option<String>,
    /// Path to the word-vector table backing `codenames-embedding` seats
    /// (GloVe/word2vec text format). `None` → the embedding anchor is
    /// unavailable and asking for it is a load-time error, never a silent
    /// substitution. Shaped exactly like [`Self::wordlist_path`].
    #[serde(default)]
    pub vectors_path: Option<String>,
    /// Hard cap on a single clue word, in characters (tune-later: decision 12).
    #[serde(default = "default_codenames_clue_word_max_len")]
    pub clue_word_max_len: usize,
    /// Named anchor pools: exactly 3 frozen anchors per pool (controlled mode
    /// seats 1 candidate + 3 anchors across the four fixed seats).
    #[serde(default)]
    pub pools: HashMap<String, Vec<CodenamesAnchorSpec>>,
    /// Named arena model sets: 4 seat specs per set.
    #[serde(default)]
    pub model_sets: HashMap<String, Vec<String>>,
}

impl Default for CodenamesConfig {
    fn default() -> Self {
        Self {
            retry_budget: super::default_retry_budget(),
            agent_max_tokens: super::default_agent_max_tokens(),
            agent_temperature: super::default_agent_temperature(),
            rating_k: super::default_rating_k(),
            wordlist_path: None,
            vectors_path: None,
            clue_word_max_len: default_codenames_clue_word_max_len(),
            pools: HashMap::new(),
            model_sets: HashMap::new(),
        }
    }
}

fn default_codenames_clue_word_max_len() -> usize {
    20
}

/// One frozen Codenames anchor seat. `kind` is validated by the codenames
/// module (`random-legal` | `embedding-greedy` | `llm`).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CodenamesAnchorSpec {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub persona: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
}
