//! Persistent Catan game records: everything a replay, metric, or rating
//! needs, self-contained as one JSON document.

use crate::catan::events::EventRecord;
use crate::catan::types::Seat;
use crate::game_core::{Reliability, ThoughtRecord};
use crate::llm::TokenUsage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const RULES_VERSION: &str = "catan-4p-v1";

/// One seat of one game.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SeatRecord {
    /// Seat index = turn order = the rating layer's conditioning variable.
    pub seat: Seat,
    pub model_id: String,
    pub agent_kind: String,
    pub scaffold_version: String,
    pub temperature: Option<f32>,
    pub is_anchor: bool,
    /// Final total VP including hidden VP cards.
    pub final_vp: u8,
    /// Final board-visible VP.
    pub public_vp: u8,
    /// Competition placement 1–4 (ties share the better rank).
    pub placement: u8,
    pub won: bool,
    pub knights_played: u8,
    pub reliability: Reliability,
    pub thoughts: Vec<ThoughtRecord>,
    pub usage: TokenUsage,
}

/// The complete, replayable record of one game.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GameRecord {
    pub game_id: String,
    pub seed: u64,
    pub player_count: u8,
    pub rules_version: String,
    /// Schedule provenance: `controlled/<board>/s<seat>/r<rep>` or
    /// `arena/g<idx>`.
    pub schedule_label: String,
    pub winner: Seat,
    /// Final total VP per seat (hidden VP included).
    pub final_vps: Vec<u8>,
    pub turns: u32,
    pub seats: Vec<SeatRecord>,
    pub events: Vec<EventRecord>,
    pub duration_ms: u64,
}

/// Competition placements from final VP (ties share the better rank):
/// `[10, 7, 7, 5]` → `[1, 2, 2, 4]`.
pub fn placements(final_vps: &[u8]) -> Vec<u8> {
    final_vps
        .iter()
        .map(|&vp| 1 + final_vps.iter().filter(|&&other| other > vp).count() as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::placements;

    #[test]
    fn placements_share_rank_on_ties() {
        assert_eq!(placements(&[10, 7, 7, 5]), vec![1, 2, 2, 4]);
        assert_eq!(placements(&[4, 4, 4, 4]), vec![1, 1, 1, 1]);
        assert_eq!(placements(&[2, 3, 4, 5]), vec![4, 3, 2, 1]);
    }
}
