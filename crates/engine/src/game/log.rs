//! The game event log — the transcript spine.
//!
//! Every state change emits an [`Event`]. Events are split by visibility:
//! **public** events every living player witnessed, and **private** events
//! scoped to a single seat (role knowledge, drawn tiles, investigation results).
//! Observations (`observation.rs`) are assembled as *public log + this seat's
//! private events*, so information isolation is enforced by construction rather
//! than by prompt etiquette.

use crate::game::board::Power;
use crate::game::policy::Policy;
use crate::game::roles::Party;
use serde::{Deserialize, Serialize};

/// A single thing that happened in the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    // ---- public ---------------------------------------------------------
    GameStarted {
        seats: usize,
    },
    PresidencyBegan {
        president: usize,
        special_election: bool,
    },
    ChancellorNominated {
        president: usize,
        nominee: usize,
    },
    /// A discussion utterance. Filled by the runner's simultaneous-reveal loop.
    Utterance {
        seat: usize,
        round: u8,
        text: String,
    },
    VotesCast {
        votes: Vec<(usize, bool)>,
        ja: usize,
        needed: usize,
        passed: bool,
    },
    GovernmentElected {
        president: usize,
        chancellor: usize,
    },
    ElectionFailed {
        tracker: u8,
    },
    /// Election tracker hit 3 → top policy auto-enacted, no power granted.
    ChaosPolicyEnacted {
        policy: Policy,
        liberal: u8,
        fascist: u8,
    },
    PolicyEnacted {
        policy: Policy,
        president: usize,
        chancellor: usize,
        liberal: u8,
        fascist: u8,
    },
    PowerGranted {
        president: usize,
        power: Power,
    },
    /// The *fact* of an investigation is public; the party result is private.
    LoyaltyInvestigated {
        president: usize,
        target: usize,
    },
    SpecialElectionCalled {
        president: usize,
        appointed: usize,
    },
    PlayerExecuted {
        president: usize,
        target: usize,
    },
    VetoProposed {
        chancellor: usize,
    },
    VetoResolved {
        president: usize,
        consented: bool,
    },
    GameOver {
        winner: crate::game::roles::Faction,
        reason: WinReason,
    },

    // ---- private (seat-scoped) -----------------------------------------
    RoleAssigned {
        role: crate::game::roles::Role,
    },
    /// Sent only to regular Fascists: their teammates and which one is Hitler.
    /// Hitler receives no such event (Hitler is blind at 7 players).
    FascistTeamRevealed {
        fascists: Vec<usize>,
        hitler: usize,
    },
    DrewPolicies {
        policies: Vec<Policy>,
    },
    ReceivedPolicies {
        policies: Vec<Policy>,
    },
    InvestigationResult {
        target: usize,
        party: Party,
    },
}

/// Why the game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinReason {
    /// Five Liberal policies enacted.
    LiberalPolicies,
    /// Six Fascist policies enacted.
    FascistPolicies,
    /// Hitler was executed.
    HitlerExecuted,
    /// Hitler was elected Chancellor with ≥3 Fascist policies on the board.
    HitlerChancellor,
}

/// A logged event tagged with visibility. Private events carry the owning seat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub event: Event,
    /// `None` = public (all seats); `Some(seat)` = private to that seat.
    pub private_to: Option<usize>,
}

/// The full ordered log for a game.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameLog {
    pub entries: Vec<LogEntry>,
}

impl GameLog {
    pub fn public(&mut self, event: Event) {
        self.entries.push(LogEntry {
            event,
            private_to: None,
        });
    }

    pub fn private(&mut self, seat: usize, event: Event) {
        self.entries.push(LogEntry {
            event,
            private_to: Some(seat),
        });
    }

    /// Public events in order.
    pub fn public_events(&self) -> impl Iterator<Item = &Event> {
        self.entries
            .iter()
            .filter(|e| e.private_to.is_none())
            .map(|e| &e.event)
    }

    /// Events a given seat is entitled to see: all public events plus that
    /// seat's own private events, in original order.
    pub fn visible_to(&self, seat: usize) -> Vec<Event> {
        self.entries
            .iter()
            .filter(|e| e.private_to.is_none() || e.private_to == Some(seat))
            .map(|e| e.event.clone())
            .collect()
    }
}
