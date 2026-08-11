//! Persistent Codenames game records: everything a replay, metric, or rating
//! needs, self-contained as one JSON document.
//!
//! `seed` is a `u64` here exactly as in the Catan and Secret Hitler records;
//! the hex-text convention (`{:016x}`) belongs to the storage layer, which
//! lands with the `codenames_*` migration in a later phase.

use crate::codenames::events::EventRecord;
use crate::codenames::types::{EndReason, Role, Seat, Team};
use crate::game_core::{Reliability, ThoughtRecord};
use crate::llm::TokenUsage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::codenames::types::RULES_VERSION;

/// One seat of one game. Rating entities are keyed `(model_id, role)`
/// (PRD-codenames-evals US-CN09), so both live here.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SeatRecord {
    /// Seat index: 0 = A spymaster, 1 = A operative, 2 = B spymaster,
    /// 3 = B operative.
    pub seat: Seat,
    pub role: Role,
    pub team: Team,
    pub model_id: String,
    pub agent_kind: String,
    pub scaffold_version: String,
    pub temperature: Option<f32>,
    pub is_anchor: bool,
    pub won: bool,
    pub reliability: Reliability,
    pub thoughts: Vec<ThoughtRecord>,
    pub usage: TokenUsage,
}

/// The complete, replayable record of one game.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GameRecord {
    pub game_id: String,
    pub seed: u64,
    pub rules_version: String,
    /// SHA-256 of the pool the grid was drawn from: records from different
    /// wordlists are not comparable.
    pub wordlist_hash: String,
    /// Schedule provenance: `controlled/<board>/<side>/<role>/r<rep>` or
    /// `arena/g<idx>`; `adhoc` for one-off games.
    pub schedule_label: String,
    pub winner: Team,
    pub end_reason: EndReason,
    /// Team turns played (each turn = one clue plus its guesses).
    pub turns: u32,
    pub seats: Vec<SeatRecord>,
    pub events: Vec<EventRecord>,
    pub duration_ms: u64,
}

impl GameRecord {
    /// The seats of one team, in seat order (spymaster first).
    pub fn team_seats(&self, team: Team) -> Vec<&SeatRecord> {
        self.seats.iter().filter(|s| s.team == team).collect()
    }
}
