//! Agent-facing decision points and actions (atomic, act-until-pass — one
//! action per LLM call), plus the deterministic forced legal defaults applied
//! when an agent exhausts its retry budget.

use super::board::{EdgeId, HexId, VertexId};
use super::events::TradeOffer;
use super::state::{GameState, Phase};
use super::types::{Resource, ResourceSet, Seat, RESOURCES};
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::game_core::IllegalMove;

/// A decision the engine is waiting on. Discards after a 7 are simultaneous;
/// everything else is strictly sequential.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum DecisionPoint {
    SetupSettlement {
        seat: Seat,
        round: u8,
    },
    SetupRoad {
        seat: Seat,
        /// The just-placed settlement this road must attach to.
        at: VertexId,
    },
    /// Before rolling: roll, or play a Knight first.
    PreRoll {
        seat: Seat,
        can_play_knight: bool,
    },
    DiscardHalf {
        seat: Seat,
        count: u8,
    },
    MoveRobber {
        seat: Seat,
    },
    ChooseVictim {
        seat: Seat,
        victims: Vec<Seat>,
    },
    /// The post-roll action loop: build/trade/play/say/end.
    TurnAction {
        seat: Seat,
    },
    /// Free placements from Road Building.
    FreeRoad {
        seat: Seat,
        remaining: u8,
    },
    RespondTrade {
        seat: Seat,
        proposer: Seat,
        offer: TradeOffer,
    },
    ResolveTrade {
        seat: Seat,
        accepters: Vec<Seat>,
        counters: Vec<(Seat, TradeOffer)>,
    },
}

impl DecisionPoint {
    /// The seat that must act.
    pub fn seat(&self) -> Seat {
        match self {
            DecisionPoint::SetupSettlement { seat, .. }
            | DecisionPoint::SetupRoad { seat, .. }
            | DecisionPoint::PreRoll { seat, .. }
            | DecisionPoint::DiscardHalf { seat, .. }
            | DecisionPoint::MoveRobber { seat }
            | DecisionPoint::ChooseVictim { seat, .. }
            | DecisionPoint::TurnAction { seat }
            | DecisionPoint::FreeRoad { seat, .. }
            | DecisionPoint::RespondTrade { seat, .. }
            | DecisionPoint::ResolveTrade { seat, .. } => *seat,
        }
    }

    /// Short kebab name for logs and reliability counters.
    pub fn kind(&self) -> &'static str {
        match self {
            DecisionPoint::SetupSettlement { .. } => "setup-settlement",
            DecisionPoint::SetupRoad { .. } => "setup-road",
            DecisionPoint::PreRoll { .. } => "pre-roll",
            DecisionPoint::DiscardHalf { .. } => "discard",
            DecisionPoint::MoveRobber { .. } => "move-robber",
            DecisionPoint::ChooseVictim { .. } => "choose-victim",
            DecisionPoint::TurnAction { .. } => "turn",
            DecisionPoint::FreeRoad { .. } => "free-road",
            DecisionPoint::RespondTrade { .. } => "respond-trade",
            DecisionPoint::ResolveTrade { .. } => "resolve-trade",
        }
    }
}

impl crate::game_core::DecisionOps for DecisionPoint {
    fn seat(&self) -> Seat {
        DecisionPoint::seat(self)
    }
    fn kind(&self) -> &'static str {
        DecisionPoint::kind(self)
    }
}

/// An agent's move at a decision point. Trade counters are stated from the
/// **responder's** perspective (what *you* give and receive); the engine
/// converts to the proposer's perspective for events and execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    PlaceSetupSettlement {
        vertex: VertexId,
    },
    PlaceSetupRoad {
        edge: EdgeId,
    },
    Roll,
    PlayKnight,
    Discard {
        resources: ResourceSet,
    },
    MoveRobber {
        hex: HexId,
    },
    StealFrom {
        seat: Seat,
    },
    BuildRoad {
        edge: EdgeId,
    },
    BuildSettlement {
        vertex: VertexId,
    },
    BuildCity {
        vertex: VertexId,
    },
    BuyDev,
    PlayRoadBuilding,
    PlayYearOfPlenty {
        first: Resource,
        second: Resource,
    },
    PlayMonopoly {
        resource: Resource,
    },
    /// Trade with the bank at the seat's best rate (4:1, 3:1, or 2:1).
    BankTrade {
        give: Resource,
        receive: Resource,
    },
    ProposeTrade {
        give: ResourceSet,
        receive: ResourceSet,
        /// `None` = open offer to all other players.
        to: Option<Seat>,
        message: Option<String>,
    },
    AcceptTrade {
        message: Option<String>,
    },
    RejectTrade {
        message: Option<String>,
    },
    CounterTrade {
        give: ResourceSet,
        receive: ResourceSet,
        message: Option<String>,
    },
    /// Close the window: execute with `partner` (an accepter or a
    /// counter-offerer) or cancel with `None`.
    ResolveTrade {
        partner: Option<Seat>,
    },
    Say {
        message: String,
    },
    EndTurn,
}

impl GameState {
    /// Every decision the engine is currently waiting on. Multiple entries
    /// exist only while simultaneous discards are being collected.
    pub fn pending_decisions(&self) -> Vec<DecisionPoint> {
        match &self.phase {
            Phase::Setup {
                placements,
                road_for: None,
            } => vec![DecisionPoint::SetupSettlement {
                seat: Self::setup_seat(*placements),
                round: placements / super::types::PLAYER_COUNT,
            }],
            Phase::Setup {
                placements,
                road_for: Some(v),
            } => vec![DecisionPoint::SetupRoad {
                seat: Self::setup_seat(*placements),
                at: *v,
            }],
            Phase::PreRoll => vec![DecisionPoint::PreRoll {
                seat: self.active,
                can_play_knight: self.can_play_knight(self.active),
            }],
            Phase::Discard { pending } => pending
                .iter()
                .map(|&(seat, count)| DecisionPoint::DiscardHalf { seat, count })
                .collect(),
            Phase::MoveRobber => vec![DecisionPoint::MoveRobber { seat: self.active }],
            Phase::ChooseVictim { victims } => vec![DecisionPoint::ChooseVictim {
                seat: self.active,
                victims: victims.clone(),
            }],
            Phase::Turn => vec![DecisionPoint::TurnAction { seat: self.active }],
            Phase::FreeRoad => vec![DecisionPoint::FreeRoad {
                seat: self.active,
                remaining: self.free_roads,
            }],
            // Sequential responses in seat order: only the head responds now.
            Phase::TradeResponse {
                offer, to_respond, ..
            } => vec![DecisionPoint::RespondTrade {
                seat: to_respond[0],
                proposer: self.active,
                offer: *offer,
            }],
            Phase::ResolveTrade {
                accepters,
                counters,
                ..
            } => vec![DecisionPoint::ResolveTrade {
                seat: self.active,
                accepters: accepters.clone(),
                counters: counters.clone(),
            }],
            Phase::GameOver => Vec::new(),
        }
    }

    pub fn can_play_knight(&self, seat: Seat) -> bool {
        !self.dev_played_this_turn
            && self.players[seat as usize]
                .devs_playable
                .contains(&super::types::DevCard::Knight)
    }

    /// The deterministic forced legal default for a decision: placements →
    /// seeded-random legal site; discard → largest holdings first; robber →
    /// seeded-random legal hex/victim; trade response → reject; resolve →
    /// cancel; turn → end turn.
    pub fn forced_default(&mut self, decision: &DecisionPoint) -> Action {
        match decision {
            DecisionPoint::SetupSettlement { .. } => {
                let sites = self.setup_settlement_sites();
                Action::PlaceSetupSettlement {
                    vertex: sites[self.rng.gen_range(0..sites.len())],
                }
            }
            DecisionPoint::SetupRoad { at, .. } => {
                let sites = self.setup_road_sites(*at);
                Action::PlaceSetupRoad {
                    edge: sites[self.rng.gen_range(0..sites.len())],
                }
            }
            DecisionPoint::PreRoll { .. } => Action::Roll,
            DecisionPoint::DiscardHalf { seat, count } => {
                // Shed from the largest holdings first (deterministic; ties
                // resolve in canonical resource order).
                let mut hand = self.players[*seat as usize].resources;
                let mut out = ResourceSet::default();
                for _ in 0..*count {
                    let r = RESOURCES
                        .into_iter()
                        .max_by_key(|&r| hand.get(r))
                        .expect("five resources");
                    hand.remove(r, 1);
                    out.add(r, 1);
                }
                Action::Discard { resources: out }
            }
            DecisionPoint::MoveRobber { .. } => {
                let options: Vec<HexId> = (0..super::board::HEX_COUNT as HexId)
                    .filter(|&h| h != self.robber)
                    .collect();
                Action::MoveRobber {
                    hex: options[self.rng.gen_range(0..options.len())],
                }
            }
            DecisionPoint::ChooseVictim { victims, .. } => Action::StealFrom {
                seat: victims[self.rng.gen_range(0..victims.len())],
            },
            DecisionPoint::TurnAction { .. } => Action::EndTurn,
            DecisionPoint::FreeRoad { seat, .. } => {
                let sites = self.road_sites(*seat);
                Action::BuildRoad {
                    edge: sites[self.rng.gen_range(0..sites.len())],
                }
            }
            DecisionPoint::RespondTrade { .. } => Action::RejectTrade { message: None },
            DecisionPoint::ResolveTrade { .. } => Action::ResolveTrade { partner: None },
        }
    }
}
