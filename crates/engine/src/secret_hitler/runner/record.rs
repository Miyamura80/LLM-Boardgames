//! Persistent game records: everything a replay, metric, or rating needs.
//! Mirrors the data schema in `docs/rating-design.md` / the review's §26.

use crate::llm::TokenUsage;
use crate::secret_hitler::agents::BeliefReport;
use crate::secret_hitler::events::EventRecord;
use crate::secret_hitler::types::{Party, Role, Seat, WinCondition};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One policy decision a player personally made, with the tiles they held —
/// the luck-controlled input for the goal-aligned enactment metric.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PolicyChoice {
    pub round: u32,
    pub as_president: bool,
    pub tiles_held: Vec<Party>,
    /// The tile discarded (president) or enacted (chancellor).
    pub chosen: Party,
    /// Forced-default choices are excluded from play-quality metrics.
    pub forced: bool,
}

/// One execution decision.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionChoice {
    pub round: u32,
    pub target: Seat,
    pub target_party: Party,
    pub target_was_hitler: bool,
    pub forced: bool,
}

/// Reliability counters, kept strictly separate from play-quality metrics.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
pub struct Reliability {
    pub malformed_outputs: u32,
    pub illegal_moves: u32,
    pub forced_defaults: u32,
    pub transport_failures: u32,
}

/// One seat of one game.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SeatRecord {
    pub seat: Seat,
    pub model_id: String,
    pub agent_kind: String,
    pub scaffold_version: String,
    pub temperature: Option<f32>,
    pub role: Role,
    pub is_anchor: bool,
    pub survived: bool,
    pub won: bool,
    pub reliability: Reliability,
    pub policy_choices: Vec<PolicyChoice>,
    pub executions: Vec<ExecutionChoice>,
    pub usage: TokenUsage,
}

/// A private belief snapshot at a checkpoint (after the Nth enacted policy;
/// `checkpoint == u8::MAX` marks the final game-end snapshot).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BeliefSnapshot {
    pub checkpoint: u8,
    pub seat: Seat,
    pub report: BeliefReport,
}

/// The complete, replayable record of one game.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GameRecord {
    pub game_id: String,
    pub seed: u64,
    pub player_count: u8,
    pub rules_version: String,
    /// Schedule provenance: "controlled" cell (candidate/role/seat/rep) or
    /// "arena" game index.
    pub schedule_label: String,
    pub winner: Party,
    pub win_condition: WinCondition,
    pub rounds: u32,
    pub roles: Vec<Role>,
    pub seats: Vec<SeatRecord>,
    pub beliefs: Vec<BeliefSnapshot>,
    pub events: Vec<EventRecord>,
    pub duration_ms: u64,
    pub discussion_rounds: u8,
}

pub const RULES_VERSION: &str = "sh-7p-v1";
