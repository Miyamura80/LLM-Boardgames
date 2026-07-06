//! The per-game record: the replayable transcript plus everything the metric
//! and rating layers consume (US-008/010). Purely engine-derived + declared
//! private beliefs — no subjective grading.

use crate::eval::agent::Beliefs;
use crate::game::{Faction, GameLog, Role, WinReason};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which agent sat in which seat, and the role it was dealt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeatAssignment {
    pub seat: usize,
    pub agent: String,
    pub role: Role,
}

/// Reliability counters, kept strictly separate from play-quality metrics
/// (FR-5). Forced-default actions are excluded from play-quality scoring.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reliability {
    pub malformed_outputs: u32,
    pub illegal_moves: u32,
    pub forced_defaults: u32,
}

/// A private belief elicitation at a checkpoint (after each enacted policy, plus
/// a final game-end snapshot). Never revealed to other seats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefSnapshot {
    /// Monotonic checkpoint index (0 = after first enacted policy, …).
    pub checkpoint: u32,
    pub seat: usize,
    pub beliefs: Beliefs,
}

/// Token/cost accounting for the LLM calls in one game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub calls: u64,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.calls += other.calls;
    }
    pub fn total_tokens(&self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// Everything produced by playing one game to a terminal state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRecord {
    pub seed: u64,
    pub first_president: usize,
    pub seats: Vec<SeatAssignment>,
    pub winner: Faction,
    pub win_reason: WinReason,
    /// Full transcript (public + per-seat private events).
    pub log: GameLog,
    /// Per-seat reliability counters.
    pub reliability: BTreeMap<usize, Reliability>,
    pub beliefs: Vec<BeliefSnapshot>,
    pub usage: Usage,
}

impl GameRecord {
    pub fn role_of(&self, seat: usize) -> Role {
        self.seats[seat].role
    }
    pub fn agent_of(&self, seat: usize) -> &str {
        &self.seats[seat].agent
    }
}
