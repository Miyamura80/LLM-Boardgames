//! Agent-facing decision points and actions, plus the deterministic forced
//! legal defaults applied when an agent exhausts its retry budget (FR-4).

use super::state::{GameState, Phase};
use super::types::{Party, Power, Seat};
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A decision the engine is waiting on. Votes are simultaneous: while ballots
/// are being collected every living seat that has not voted has a pending
/// `Vote` decision, and nothing is revealed until all are in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum DecisionPoint {
    Nominate {
        president: Seat,
        eligible: Vec<Seat>,
    },
    Vote {
        seat: Seat,
        nominee: Seat,
    },
    /// President holds 3 tiles, discards 1 (by party — unambiguous since tiles
    /// only differ by party).
    Discard {
        president: Seat,
        tiles: Vec<Party>,
    },
    /// Chancellor holds 2 tiles, enacts 1; may propose a veto once when the
    /// 5th Fascist policy has unlocked it.
    Enact {
        chancellor: Seat,
        tiles: Vec<Party>,
        can_veto: bool,
    },
    VetoConsent {
        president: Seat,
    },
    UsePower {
        president: Seat,
        power: Power,
        targets: Vec<Seat>,
    },
}

impl DecisionPoint {
    /// The seat that must act.
    pub fn seat(&self) -> Seat {
        match self {
            DecisionPoint::Nominate { president, .. } => *president,
            DecisionPoint::Vote { seat, .. } => *seat,
            DecisionPoint::Discard { president, .. } => *president,
            DecisionPoint::Enact { chancellor, .. } => *chancellor,
            DecisionPoint::VetoConsent { president } => *president,
            DecisionPoint::UsePower { president, .. } => *president,
        }
    }

    /// Short kebab name for logs and reliability counters.
    pub fn kind(&self) -> &'static str {
        match self {
            DecisionPoint::Nominate { .. } => "nominate",
            DecisionPoint::Vote { .. } => "vote",
            DecisionPoint::Discard { .. } => "discard",
            DecisionPoint::Enact { .. } => "enact",
            DecisionPoint::VetoConsent { .. } => "veto-consent",
            DecisionPoint::UsePower { .. } => "use-power",
        }
    }
}

/// An agent's move at a decision point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Nominate { target: Seat },
    Vote { ja: bool },
    Discard { policy: Party },
    Enact { policy: Party },
    ProposeVeto,
    VetoConsent { approve: bool },
    UsePower { target: Seat },
}

pub use crate::game_core::IllegalMove;

impl crate::game_core::DecisionOps for DecisionPoint {
    fn seat(&self) -> Seat {
        DecisionPoint::seat(self)
    }
    fn kind(&self) -> &'static str {
        DecisionPoint::kind(self)
    }
}

impl GameState {
    /// Every decision the engine is currently waiting on. Multiple entries
    /// exist only while simultaneous ballots are being collected.
    pub fn pending_decisions(&self) -> Vec<DecisionPoint> {
        match &self.phase {
            Phase::Nomination => vec![DecisionPoint::Nominate {
                president: self.presidency,
                eligible: self.eligible_chancellors(),
            }],
            Phase::Election { nominee, votes } => self
                .alive_seats()
                .into_iter()
                .filter(|s| !votes.contains_key(s))
                .map(|seat| DecisionPoint::Vote {
                    seat,
                    nominee: *nominee,
                })
                .collect(),
            Phase::LegislativePresident { tiles } => vec![DecisionPoint::Discard {
                president: self.presidency,
                tiles: tiles.clone(),
            }],
            Phase::LegislativeChancellor {
                tiles,
                veto_proposed,
            } => vec![DecisionPoint::Enact {
                chancellor: self.gov_chancellor.expect("chancellor set in legislative"),
                tiles: tiles.clone(),
                can_veto: self.veto_unlocked() && !veto_proposed,
            }],
            Phase::VetoConsent { .. } => vec![DecisionPoint::VetoConsent {
                president: self.presidency,
            }],
            Phase::ExecutiveAction { power } => vec![DecisionPoint::UsePower {
                president: self.presidency,
                power: *power,
                targets: self.power_targets(*power),
            }],
            Phase::GameOver => Vec::new(),
        }
    }

    pub fn veto_unlocked(&self) -> bool {
        self.fascist_policies >= 5
    }

    /// The deterministic forced legal default for a decision (FR-4):
    /// vote → Nein; nomination → first eligible by seat; policy choice →
    /// seeded-random tile; power target → first legal target; veto consent →
    /// decline. Tile randomness draws from the game RNG so a seeded run stays
    /// reproducible.
    pub fn forced_default(&mut self, decision: &DecisionPoint) -> Action {
        match decision {
            DecisionPoint::Nominate { eligible, .. } => Action::Nominate {
                target: *eligible
                    .first()
                    .expect("7-player games always have an eligible chancellor"),
            },
            DecisionPoint::Vote { .. } => Action::Vote { ja: false },
            DecisionPoint::Discard { tiles, .. } => Action::Discard {
                policy: tiles[self.rng.gen_range(0..tiles.len())],
            },
            DecisionPoint::Enact { tiles, .. } => Action::Enact {
                policy: tiles[self.rng.gen_range(0..tiles.len())],
            },
            DecisionPoint::VetoConsent { .. } => Action::VetoConsent { approve: false },
            DecisionPoint::UsePower { targets, .. } => Action::UsePower {
                target: *targets
                    .first()
                    .expect("powers always have a legal target at 7 players"),
            },
        }
    }
}
