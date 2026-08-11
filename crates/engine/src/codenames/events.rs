//! The Codenames transcript. Every event is `Public`: the only hidden
//! information in the game is the key card, and that is *state* derived into
//! spymaster observations (see [`super::observation`]), never an event — so
//! `Visibility` needs no team variant (PRD-codenames-evals §9 decision 8).

use super::types::{CardIdentity, Clue, EndReason, Seat, Team};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::game_core::Visibility;

/// One transcript entry (the shared envelope carrying a Codenames payload).
pub type EventRecord = crate::game_core::EventRecord<CodenamesEvent>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum CodenamesEvent {
    GameStarted {
        /// Always team A — the nine-agent side.
        starting_team: Team,
        /// SHA-256 of the pool the grid was drawn from, for record
        /// comparability.
        wordlist_hash: String,
        rules_version: String,
    },
    /// The 25 words in grid order, emitted once at setup. Public: the words
    /// are face-up from the start; only their identities are secret.
    BoardLaid {
        words: Vec<String>,
    },
    TurnStarted {
        team: Team,
        turn: u32,
    },
    ClueGiven {
        seat: Seat,
        team: Team,
        clue: Clue,
    },
    /// One card flipped. Carries the revealed identity (now public) and
    /// whether the reveal ended the guessing team's turn.
    GuessRevealed {
        seat: Seat,
        team: Team,
        word: String,
        identity: CardIdentity,
        ends_turn: bool,
    },
    /// The operative declined a further guess (legal only after the mandatory
    /// first guess).
    TurnPassed {
        seat: Seat,
        team: Team,
    },
    /// An agent exhausted its retry budget; the engine applied the
    /// deterministic legal default (metric-exempt, public).
    ForcedDefault {
        seat: Seat,
        decision: String,
    },
    GameEnded {
        winner: Team,
        reason: EndReason,
        turns: u32,
    },
}
