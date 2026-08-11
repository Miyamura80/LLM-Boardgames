//! Catan harness configuration (split from lib.rs; see PRD-catan-evals).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Catan eval harness settings (see `docs/PRD-catan-evals.md`). Bot kinds are
/// plain strings here and validated by the catan module at the game boundary
/// (decision #10: config does not accrete per-game bot taxonomies).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CatanConfig {
    /// Rethink attempts per decision before the forced legal default.
    #[serde(default = "super::default_retry_budget")]
    pub retry_budget: u32,
    /// Actions per turn before `end_turn` is forced.
    #[serde(default = "default_catan_action_cap")]
    pub action_cap: u32,
    /// Player-trade windows the active player may open per turn.
    #[serde(default = "default_catan_trade_windows")]
    pub trade_windows_cap: u8,
    /// Free-form `say` actions per turn.
    #[serde(default = "default_catan_say_cap")]
    pub say_cap: u8,
    /// Hard game-length backstop (turns); highest VP wins a capped game.
    #[serde(default = "default_catan_max_turns")]
    pub max_turns: u32,
    /// Hard cap on one public message, in characters.
    #[serde(default = "super::default_utterance_cap")]
    pub utterance_char_cap: usize,
    /// Max completion tokens per agent call.
    #[serde(default = "super::default_agent_max_tokens")]
    pub agent_max_tokens: u32,
    /// Sampling temperature for LLM seats (part of the scaffold version).
    #[serde(default = "super::default_agent_temperature")]
    pub agent_temperature: f32,
    /// Conservatism factor k in the reported score μ − kσ.
    #[serde(default = "super::default_rating_k")]
    pub rating_k: f64,
    /// Named anchor pools: exactly 3 frozen anchors per pool (controlled mode
    /// seats 1 candidate + 3 anchors).
    #[serde(default)]
    pub pools: HashMap<String, Vec<CatanAnchorSpec>>,
    /// Named arena model sets: 4 seat specs per set.
    #[serde(default)]
    pub model_sets: HashMap<String, Vec<String>>,
}

impl Default for CatanConfig {
    fn default() -> Self {
        Self {
            retry_budget: super::default_retry_budget(),
            action_cap: default_catan_action_cap(),
            trade_windows_cap: default_catan_trade_windows(),
            say_cap: default_catan_say_cap(),
            max_turns: default_catan_max_turns(),
            utterance_char_cap: super::default_utterance_cap(),
            agent_max_tokens: super::default_agent_max_tokens(),
            agent_temperature: super::default_agent_temperature(),
            rating_k: super::default_rating_k(),
            pools: HashMap::new(),
            model_sets: HashMap::new(),
        }
    }
}

fn default_catan_action_cap() -> u32 {
    12
}
fn default_catan_trade_windows() -> u8 {
    3
}
fn default_catan_say_cap() -> u8 {
    2
}
fn default_catan_max_turns() -> u32 {
    200
}

/// One frozen Catan anchor seat. `kind` is validated by the catan module
/// (`random-legal` | `greedy` | `llm`).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CatanAnchorSpec {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub persona: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
}
