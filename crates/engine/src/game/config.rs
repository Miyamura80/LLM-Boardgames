//! Game configuration knobs. v1 is fixed at 7 players; only seed, first
//! president, and discussion budget are tunable.

use crate::game::roles::Role;
use serde::{Deserialize, Serialize};

/// Fixed player count for v1.
pub const NUM_PLAYERS: usize = 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// Master seed: deck shuffle, role assignment, forced-default tie-breaks.
    pub seed: u64,
    /// Seat that opens as the first Presidential candidate. `None` → seeded.
    #[serde(default)]
    pub first_president: Option<usize>,
    /// Simultaneous discussion rounds before each vote (runner-driven; the
    /// engine records utterances but does not gate on them). Default 3.
    #[serde(default = "default_discussion_rounds")]
    pub discussion_rounds: u8,
    /// Explicit seat→role assignment. `None` → dealt from the seed. The match
    /// scheduler sets this to rotate models through factions deterministically.
    #[serde(default)]
    pub role_assignment: Option<[Role; NUM_PLAYERS]>,
}

fn default_discussion_rounds() -> u8 {
    3
}

impl GameConfig {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            first_president: None,
            discussion_rounds: default_discussion_rounds(),
            role_assignment: None,
        }
    }

    pub fn with_first_president(mut self, seat: usize) -> Self {
        self.first_president = Some(seat);
        self
    }

    pub fn with_roles(mut self, roles: [Role; NUM_PLAYERS]) -> Self {
        self.role_assignment = Some(roles);
        self
    }
}
