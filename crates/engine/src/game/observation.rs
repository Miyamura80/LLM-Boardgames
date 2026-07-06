//! Per-seat role-conditioned observations (US-003).
//!
//! An [`Observation`] is everything a seat is *entitled* to know: public board
//! state, the public + own-private event history, and any private knowledge the
//! seat legitimately holds (own role; for a regular Fascist, teammates + Hitler;
//! own investigation results; own current tiles). It is assembled by
//! `GameState::observation`, which only ever reads that seat's slice — a Liberal
//! or Hitler observation *cannot* contain another player's role by construction.

use crate::game::action::Decision;
use crate::game::board::Board;
use crate::game::log::Event;
use crate::game::roles::{Party, Role};
use serde::{Deserialize, Serialize};

/// What a seat knows about a single player (never their role, unless it's a
/// teammate the seat is entitled to see — surfaced separately in `known_team`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerView {
    pub seat: usize,
    pub alive: bool,
}

/// A regular Fascist's private knowledge: who the Fascists are and which is
/// Hitler. Hitler never gets this (blind at 7 players); Liberals never get this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownTeam {
    /// Seats of the two regular Fascists (may include the observer).
    pub fascists: Vec<usize>,
    pub hitler: usize,
}

/// The full role-conditioned view for one seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub you: usize,
    pub your_role: Role,
    /// Present only for regular Fascists.
    pub known_team: Option<KnownTeam>,
    pub players: Vec<PlayerView>,
    pub board: Board,
    pub election_tracker: u8,
    pub president: Option<usize>,
    /// Chancellor candidates barred by term limits right now.
    pub term_limited: Vec<usize>,
    pub veto_unlocked: bool,
    /// This seat's own Investigate Loyalty results (target → party).
    pub your_investigations: Vec<(usize, Party)>,
    /// Public events + this seat's private events, in order.
    pub history: Vec<Event>,
    /// The decision this seat must answer now, if any. For a simultaneous vote,
    /// every living seat receives the same `Vote` decision.
    pub pending: Option<Decision>,
}

impl Observation {
    /// True if this observation exposes no other player's secret role anywhere
    /// (used by isolation tests). `known_team` is *entitled* knowledge and is
    /// checked separately.
    pub fn leaks_foreign_role(&self) -> bool {
        self.history.iter().any(|e| match e {
            Event::RoleAssigned { .. } => false, // own role only (private_to = you)
            Event::FascistTeamRevealed { .. } => false, // entitled (regular Fascist)
            Event::InvestigationResult { .. } => false, // own investigation only
            _ => false,
        })
    }
}
