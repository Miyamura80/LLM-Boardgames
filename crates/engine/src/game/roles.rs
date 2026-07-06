//! Roles, party membership, and factions for the 7-player setup.
//!
//! At 7 players the table is **4 Liberals / 3 Fascists**, one of whom is Hitler.
//! Two concepts must never be conflated:
//! - **Party** is what a player's *membership card* says. Hitler's card reads
//!   Fascist — this is what Investigate Loyalty reveals.
//! - **Role** is the secret truth (Liberal / Fascist / Hitler), used only for
//!   ground-truth metric scoring, never leaked into another seat's observation.

use serde::{Deserialize, Serialize};

/// The winning teams. A game is always won by exactly one faction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Faction {
    Liberal,
    Fascist,
}

/// Party membership — what a player's card shows (Investigate Loyalty result).
/// Hitler's card is Fascist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Party {
    Liberal,
    Fascist,
}

/// The secret role. Ground truth; never appears in another seat's observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Liberal,
    Fascist,
    Hitler,
}

impl Role {
    /// The faction this role wins with.
    pub fn faction(self) -> Faction {
        match self {
            Role::Liberal => Faction::Liberal,
            Role::Fascist | Role::Hitler => Faction::Fascist,
        }
    }

    /// The party membership shown on the card (what Investigate reveals).
    pub fn party(self) -> Party {
        match self {
            Role::Liberal => Party::Liberal,
            Role::Fascist | Role::Hitler => Party::Fascist,
        }
    }

    pub fn is_hitler(self) -> bool {
        matches!(self, Role::Hitler)
    }
}

/// The fixed 7-player role multiset: 4 Liberal, 2 Fascist, 1 Hitler.
pub const SEVEN_PLAYER_ROLES: [Role; 7] = [
    Role::Liberal,
    Role::Liberal,
    Role::Liberal,
    Role::Liberal,
    Role::Fascist,
    Role::Fascist,
    Role::Hitler,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hitler_card_reads_fascist() {
        assert_eq!(Role::Hitler.party(), Party::Fascist);
        assert_eq!(Role::Hitler.faction(), Faction::Fascist);
    }

    #[test]
    fn seven_player_composition() {
        let libs = SEVEN_PLAYER_ROLES
            .iter()
            .filter(|r| r.faction() == Faction::Liberal)
            .count();
        let fasc = SEVEN_PLAYER_ROLES
            .iter()
            .filter(|r| r.faction() == Faction::Fascist)
            .count();
        let hitlers = SEVEN_PLAYER_ROLES.iter().filter(|r| r.is_hitler()).count();
        assert_eq!(libs, 4);
        assert_eq!(fasc, 3);
        assert_eq!(hitlers, 1);
    }
}
