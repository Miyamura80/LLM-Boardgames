//! Decisions the engine asks for, the actions agents return, and the
//! deterministic forced-legal-default for each (FR-4).
//!
//! The engine is a state machine: at any non-terminal, non-discussion point it
//! exposes exactly one [`Decision`] (whose seat, what kind, which options are
//! legal). Agents answer with an [`Action`]. Illegal actions are rejected with a
//! typed [`IllegalAction`] the harness surfaces in a rethink re-prompt; on
//! exhaustion the harness applies [`Decision::forced_default`].

use crate::game::policy::Policy;
use crate::game::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What the engine is currently waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionKind {
    /// President nominates a Chancellor from `eligible`.
    Nominate {
        president: usize,
        eligible: Vec<usize>,
    },
    /// All living players vote Ja/Nein simultaneously on `(president, nominee)`.
    Vote {
        president: usize,
        nominee: usize,
        voters: Vec<usize>,
    },
    /// President discards one of three drawn tiles (indices 0..3).
    PresidentDiscard {
        president: usize,
        drawn: [Policy; 3],
    },
    /// Chancellor enacts one of two tiles (indices 0..2), or proposes a veto if
    /// `veto_available`.
    ChancellorEnact {
        chancellor: usize,
        hand: [Policy; 2],
        veto_available: bool,
    },
    /// President decides whether to consent to the Chancellor's veto.
    VetoConsent { president: usize },
    /// President investigates a target's loyalty.
    Investigate {
        president: usize,
        eligible: Vec<usize>,
    },
    /// President appoints the next Presidential candidate.
    SpecialElection {
        president: usize,
        eligible: Vec<usize>,
    },
    /// President executes a player.
    Execution {
        president: usize,
        eligible: Vec<usize>,
    },
}

/// The full pending decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub kind: DecisionKind,
}

impl Decision {
    /// Whether this is the simultaneous-vote decision (all living seats act).
    pub fn is_vote(&self) -> bool {
        matches!(self.kind, DecisionKind::Vote { .. })
    }

    /// The single seat that must act, or `None` for the simultaneous `Vote`.
    pub fn actor(&self) -> Option<usize> {
        match &self.kind {
            DecisionKind::Nominate { president, .. }
            | DecisionKind::PresidentDiscard { president, .. }
            | DecisionKind::VetoConsent { president }
            | DecisionKind::Investigate { president, .. }
            | DecisionKind::SpecialElection { president, .. }
            | DecisionKind::Execution { president, .. } => Some(*president),
            DecisionKind::ChancellorEnact { chancellor, .. } => Some(*chancellor),
            DecisionKind::Vote { .. } => None,
        }
    }

    /// The deterministic forced legal default (FR-4). `aux` is a seeded
    /// sub-stream so policy tie-breaks don't perturb deck order.
    pub fn forced_default(&self, aux: &mut Rng) -> Action {
        match &self.kind {
            DecisionKind::Nominate { eligible, .. } => Action::Nominate(eligible[0]),
            DecisionKind::Vote { voters, .. } => {
                // vote → Nein for every voter.
                Action::CastVotes(voters.iter().map(|&s| (s, false)).collect())
            }
            DecisionKind::PresidentDiscard { drawn, .. } => Action::Discard(aux.below(drawn.len())),
            DecisionKind::ChancellorEnact { hand, .. } => Action::Enact(aux.below(hand.len())),
            DecisionKind::VetoConsent { .. } => Action::VetoConsent(false),
            DecisionKind::Investigate { eligible, .. } => Action::Investigate(eligible[0]),
            DecisionKind::SpecialElection { eligible, .. } => Action::SpecialElection(eligible[0]),
            DecisionKind::Execution { eligible, .. } => Action::Execute(eligible[0]),
        }
    }
}

/// An action an agent returns in response to a [`Decision`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Nominate(usize),
    /// Full ballot: every living voter → Ja(`true`)/Nein(`false`).
    CastVotes(BTreeMap<usize, bool>),
    /// Index into the President's three drawn tiles to discard.
    Discard(usize),
    /// Index into the Chancellor's two tiles to enact.
    Enact(usize),
    /// Chancellor proposes a veto (only legal when veto is available).
    ProposeVeto,
    /// President consents (`true`) or refuses (`false`) a veto.
    VetoConsent(bool),
    Investigate(usize),
    SpecialElection(usize),
    Execute(usize),
}

/// Why an action was rejected. The message is fed back into a rethink re-prompt.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IllegalAction {
    #[error("wrong action type for the current decision")]
    WrongActionType,
    #[error("target seat {0} is not a legal choice here")]
    IllegalTarget(usize),
    #[error("policy index {0} is out of range")]
    IndexOutOfRange(usize),
    #[error("veto is not available (needs 5 Fascist policies)")]
    VetoUnavailable,
    #[error("ballot must cover exactly the living voters, once each")]
    MalformedBallot,
    #[error("the game is already over")]
    GameOver,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_vote_is_all_nein() {
        let d = Decision {
            kind: DecisionKind::Vote {
                president: 0,
                nominee: 1,
                voters: vec![0, 1, 2],
            },
        };
        let mut aux = Rng::new(1);
        match d.forced_default(&mut aux) {
            Action::CastVotes(v) => assert!(v.values().all(|&ja| !ja)),
            other => panic!("expected CastVotes, got {other:?}"),
        }
    }

    #[test]
    fn forced_nominate_is_first_eligible() {
        let d = Decision {
            kind: DecisionKind::Nominate {
                president: 0,
                eligible: vec![3, 5, 6],
            },
        };
        let mut aux = Rng::new(1);
        assert_eq!(d.forced_default(&mut aux), Action::Nominate(3));
    }
}
