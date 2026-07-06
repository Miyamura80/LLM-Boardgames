//! Core value types for the 7-player Secret Hitler ruleset.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Seat index at the table, `0..PLAYER_COUNT`.
pub type Seat = u8;

/// v1 is fixed at the 7-player ruleset (4 Liberals, 3 Fascists incl. Hitler).
pub const PLAYER_COUNT: u8 = 7;
pub const LIBERAL_COUNT: u8 = 4;
/// Regular Fascists (Hitler excluded).
pub const REGULAR_FASCIST_COUNT: u8 = 2;

/// Policy deck composition.
pub const LIBERAL_POLICIES: u8 = 6;
pub const FASCIST_POLICIES: u8 = 11;

/// Policies needed to win.
pub const LIBERAL_WIN_POLICIES: u8 = 5;
pub const FASCIST_WIN_POLICIES: u8 = 6;

/// Fascist policies on the board before "Hitler elected Chancellor" wins.
pub const HITLER_CHANCELLOR_THRESHOLD: u8 = 3;

/// Failed elections before the top policy is auto-enacted.
pub const ELECTION_TRACKER_LIMIT: u8 = 3;

/// Party membership – what an Investigate Loyalty reveals, and what a policy
/// tile advances. Hitler's party card reads Fascist.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, PartialOrd, Ord,
)]
pub enum Party {
    Liberal,
    Fascist,
}

impl Party {
    pub fn as_str(&self) -> &'static str {
        match self {
            Party::Liberal => "Liberal",
            Party::Fascist => "Fascist",
        }
    }
}

/// Secret role dealt to a seat.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, PartialOrd, Ord,
)]
pub enum Role {
    Liberal,
    Fascist,
    Hitler,
}

impl Role {
    pub fn party(&self) -> Party {
        match self {
            Role::Liberal => Party::Liberal,
            Role::Fascist | Role::Hitler => Party::Fascist,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Liberal => "Liberal",
            Role::Fascist => "Fascist",
            Role::Hitler => "Hitler",
        }
    }
}

/// Presidential powers on the 7–8-player Fascist board, keyed by the number of
/// Fascist policies enacted **by a government** (top-decked policies grant no
/// power).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Power {
    InvestigateLoyalty,
    SpecialElection,
    Execution,
}

/// The 7–8-player board: 2nd Fascist policy → Investigate, 3rd → Special
/// Election, 4th and 5th → Execution. Veto unlocks after the 5th.
pub fn power_for_fascist_policy(count: u8) -> Option<Power> {
    match count {
        2 => Some(Power::InvestigateLoyalty),
        3 => Some(Power::SpecialElection),
        4 | 5 => Some(Power::Execution),
        _ => None,
    }
}

/// How the game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum WinCondition {
    /// Five Liberal policies enacted.
    LiberalPolicies,
    /// Hitler was executed.
    HitlerExecuted,
    /// Six Fascist policies enacted.
    FascistPolicies,
    /// Hitler elected Chancellor with ≥3 Fascist policies on the board.
    HitlerChancellor,
}

impl WinCondition {
    pub fn winner(&self) -> Party {
        match self {
            WinCondition::LiberalPolicies | WinCondition::HitlerExecuted => Party::Liberal,
            WinCondition::FascistPolicies | WinCondition::HitlerChancellor => Party::Fascist,
        }
    }
}
