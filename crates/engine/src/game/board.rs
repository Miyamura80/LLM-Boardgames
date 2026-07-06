//! The policy board: enacted-policy counters, win tracks, and the 7–8-player
//! Fascist presidential-power ladder.
//!
//! Powers are indexed by the number of Fascist policies **just enacted by an
//! elected government**. A top-decked policy from the election tracker advances
//! the counter but grants no power (handled by the caller, not here).

use serde::{Deserialize, Serialize};

/// Liberal policies needed to win.
pub const LIBERAL_WIN_TRACK: u8 = 5;
/// Fascist policies needed to win outright.
pub const FASCIST_WIN_TRACK: u8 = 6;
/// Fascist policies at/after which electing Hitler Chancellor wins for Fascists.
pub const HITLER_CHANCELLOR_THRESHOLD: u8 = 3;
/// Fascist policies at/after which the Veto Power is unlocked.
pub const VETO_UNLOCK_THRESHOLD: u8 = 5;

/// A presidential power granted by reaching a Fascist-board square.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    /// Reveal a target's party membership, privately, to the President.
    InvestigateLoyalty,
    /// Appoint the next Presidential candidate.
    SpecialElection,
    /// Execute a player (Hitler → immediate Liberal win).
    Execution,
}

/// The 7–8-player Fascist power ladder, indexed by the Fascist-policy count that
/// was just reached (1-based). No power at 1; Veto unlocks at 5 alongside a
/// second Execution.
///
/// `[_, none, investigate, special, execute, execute, none]`
pub fn power_for_fascist_count(count: u8) -> Option<Power> {
    match count {
        2 => Some(Power::InvestigateLoyalty),
        3 => Some(Power::SpecialElection),
        4 => Some(Power::Execution),
        5 => Some(Power::Execution),
        _ => None,
    }
}

/// Enacted-policy tallies plus the veto-unlock check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub liberal: u8,
    pub fascist: u8,
}

impl Board {
    pub fn veto_unlocked(&self) -> bool {
        self.fascist >= VETO_UNLOCK_THRESHOLD
    }

    pub fn liberals_complete(&self) -> bool {
        self.liberal >= LIBERAL_WIN_TRACK
    }

    pub fn fascists_complete(&self) -> bool {
        self.fascist >= FASCIST_WIN_TRACK
    }

    /// Whether electing Hitler Chancellor now wins for the Fascists.
    pub fn hitler_chancellor_wins(&self) -> bool {
        self.fascist >= HITLER_CHANCELLOR_THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_ladder_matches_seven_player_board() {
        assert_eq!(power_for_fascist_count(1), None);
        assert_eq!(power_for_fascist_count(2), Some(Power::InvestigateLoyalty));
        assert_eq!(power_for_fascist_count(3), Some(Power::SpecialElection));
        assert_eq!(power_for_fascist_count(4), Some(Power::Execution));
        assert_eq!(power_for_fascist_count(5), Some(Power::Execution));
        assert_eq!(power_for_fascist_count(6), None);
    }

    #[test]
    fn veto_unlocks_at_five() {
        assert!(!Board {
            liberal: 0,
            fascist: 4
        }
        .veto_unlocked());
        assert!(Board {
            liberal: 0,
            fascist: 5
        }
        .veto_unlocked());
    }

    #[test]
    fn hitler_zone_opens_at_three() {
        assert!(!Board {
            liberal: 0,
            fascist: 2
        }
        .hitler_chancellor_wins());
        assert!(Board {
            liberal: 0,
            fascist: 3
        }
        .hitler_chancellor_wins());
    }
}
