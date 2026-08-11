//! Core Codenames value types: teams, seat roles, card identities, clues, and
//! the per-game rules knobs. Standard 2-team game with four fixed seats
//! (PRD-codenames-evals §9 decision 1).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::game_core::Seat;

/// Bumped whenever a rule change makes recorded games incomparable.
pub const RULES_VERSION: &str = "codenames-v1";

/// The grid is 5×5; cards are addressed row-major.
pub const GRID_SIDE: usize = 5;
pub const CARD_COUNT: usize = GRID_SIDE * GRID_SIDE;
/// Key-card composition: the starting team gets one extra agent.
pub const STARTING_AGENTS: u8 = 9;
pub const SECOND_AGENTS: u8 = 8;
pub const BYSTANDERS: u8 = 7;
pub const ASSASSINS: u8 = 1;
pub const SEAT_COUNT: u8 = 4;

/// Fixed seat assignment for every game.
pub const SEAT_A_SPYMASTER: Seat = 0;
pub const SEAT_A_OPERATIVE: Seat = 1;
pub const SEAT_B_SPYMASTER: Seat = 2;
pub const SEAT_B_OPERATIVE: Seat = 3;

/// The two teams. `A` always starts and always holds nine agents, so the
/// starting advantage is mirrored by the schedule (side), never by the key.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Team {
    A,
    B,
}

pub const TEAMS: [Team; 2] = [Team::A, Team::B];

impl Team {
    pub fn other(self) -> Team {
        match self {
            Team::A => Team::B,
            Team::B => Team::A,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Team::A => "a",
            Team::B => "b",
        }
    }

    /// Index into per-team arrays (A = 0, B = 1).
    pub fn index(self) -> usize {
        match self {
            Team::A => 0,
            Team::B => 1,
        }
    }

    pub fn spymaster(self) -> Seat {
        match self {
            Team::A => SEAT_A_SPYMASTER,
            Team::B => SEAT_B_SPYMASTER,
        }
    }

    pub fn operative(self) -> Seat {
        match self {
            Team::A => SEAT_A_OPERATIVE,
            Team::B => SEAT_B_OPERATIVE,
        }
    }

    /// How many agents this team's key card carries.
    pub fn agent_count(self) -> u8 {
        match self {
            Team::A => STARTING_AGENTS,
            Team::B => SECOND_AGENTS,
        }
    }
}

/// The two roles within a team. Ordered so `(model_id, role)` can key the
/// rating table's `BTreeMap` (PRD-codenames-evals US-CN09).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Spymaster,
    Operative,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Spymaster => "spymaster",
            Role::Operative => "operative",
        }
    }
}

/// The team owning a seat. Panics on an out-of-range seat: the four seats are
/// fixed, and a silent fallback here could hand a key card to a stray index.
pub fn seat_team(seat: Seat) -> Team {
    match seat {
        SEAT_A_SPYMASTER | SEAT_A_OPERATIVE => Team::A,
        SEAT_B_SPYMASTER | SEAT_B_OPERATIVE => Team::B,
        _ => panic!("codenames has four seats (0..=3), got seat {seat}"),
    }
}

/// The role a seat plays. Panics on an out-of-range seat (see [`seat_team`]).
pub fn seat_role(seat: Seat) -> Role {
    match seat {
        SEAT_A_SPYMASTER | SEAT_B_SPYMASTER => Role::Spymaster,
        SEAT_A_OPERATIVE | SEAT_B_OPERATIVE => Role::Operative,
        _ => panic!("codenames has four seats (0..=3), got seat {seat}"),
    }
}

/// What a card really is, per the key card. Only spymasters (and, once a card
/// is revealed, everyone) ever learn this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CardIdentity {
    Agent { team: Team },
    Bystander,
    Assassin,
}

impl CardIdentity {
    pub fn as_str(&self) -> &'static str {
        match self {
            CardIdentity::Agent { team: Team::A } => "agent-a",
            CardIdentity::Agent { team: Team::B } => "agent-b",
            CardIdentity::Bystander => "bystander",
            CardIdentity::Assassin => "assassin",
        }
    }

    /// The team this card scores for, if it is an agent.
    pub fn agent_team(&self) -> Option<Team> {
        match self {
            CardIdentity::Agent { team } => Some(*team),
            _ => None,
        }
    }
}

/// One clue: a single word plus the number of cards it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Clue {
    /// Normalized by the engine: trimmed and lowercased.
    pub word: String,
    pub number: u8,
}

impl Clue {
    /// Total guesses the clue entitles the operative to (the official
    /// number + 1 bonus guess).
    pub fn guess_cap(&self) -> u8 {
        self.number.saturating_add(1)
    }
}

/// Why the game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EndReason {
    /// Every agent of the winning team was revealed.
    AgentsFound,
    /// The losing team revealed the assassin.
    Assassin,
}

impl EndReason {
    /// The stored/serialized spelling (kebab-case, matching `serde`).
    pub fn as_str(self) -> &'static str {
        match self {
            EndReason::AgentsFound => "agents-found",
            EndReason::Assassin => "assassin",
        }
    }
}

/// Per-game rules knobs (config-driven; the defaults keep tests hermetic).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GameConfig {
    /// Maximum characters in a clue word. Tune-later per PRD §9 item 12.
    pub clue_word_max_len: usize,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            clue_word_max_len: 20,
        }
    }
}
