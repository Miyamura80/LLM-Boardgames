//! A deliberately simple scripted anchor: settle on pips, build by fixed
//! priority, rob the leader, accept only card-favorable trades. A weak floor
//! for the rating pool — documented as exploitable, not the leaderboard.

use super::{AgentError, AgentReply, SeatAgent};
use crate::catan::actions::{Action, DecisionPoint};
use crate::catan::board::{graph, BoardLayout, VertexId};
use crate::catan::observation::Observation;
use crate::catan::types::*;
use async_trait::async_trait;

pub struct GreedyBuilderBot;

impl GreedyBuilderBot {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GreedyBuilderBot {
    fn default() -> Self {
        Self::new()
    }
}

/// Total production pips of the hexes touching a vertex.
fn vertex_pips(layout: &BoardLayout, v: VertexId) -> u32 {
    graph().vertex_hexes[v as usize]
        .iter()
        .filter_map(|&h| layout.numbers[h as usize])
        .map(|n| BoardLayout::pips(n) as u32)
        .sum()
}

fn best_by_pips(layout: &BoardLayout, sites: &[VertexId]) -> Option<VertexId> {
    sites
        .iter()
        .copied()
        .max_by_key(|&v| (vertex_pips(layout, v), std::cmp::Reverse(v)))
}

/// Largest-holdings-first discard (mirrors the forced default: predictable).
fn discard_largest(hand: ResourceSet, count: u8) -> ResourceSet {
    let mut hand = hand;
    let mut out = ResourceSet::default();
    for _ in 0..count {
        let r = RESOURCES
            .into_iter()
            .max_by_key(|&r| hand.get(r))
            .expect("five resources");
        hand.remove(r, 1);
        out.add(r, 1);
    }
    out
}

impl GreedyBuilderBot {
    fn turn_action(&self, obs: &Observation) -> Action {
        let me = obs
            .players
            .iter()
            .find(|p| p.seat == obs.seat)
            .expect("own seat in players");
        // Priority: city > settlement > dev card > road toward the best open
        // vertex > 4:1/port bank trade toward a settlement > end.
        if me.cities_left > 0 && obs.hand.contains(&city_cost()) {
            let own_settlements: Vec<VertexId> = obs
                .occupancy
                .buildings
                .iter()
                .filter(|&&(_, owner, is_city)| owner == obs.seat && !is_city)
                .map(|&(v, _, _)| v)
                .collect();
            if let Some(v) = best_by_pips(&obs.layout, &own_settlements) {
                return Action::BuildCity { vertex: v };
            }
        }
        if me.settlements_left > 0 && obs.hand.contains(&settlement_cost()) {
            if let Some(v) = best_by_pips(&obs.layout, &obs.settlement_sites()) {
                return Action::BuildSettlement { vertex: v };
            }
        }
        if obs.hand.contains(&dev_cost()) && obs.dev_deck_remaining > 0 {
            return Action::BuyDev;
        }
        if !obs.dev_played_this_turn && obs.devs_playable.contains(&DevCard::Knight) {
            // Clear our own hexes / pressure the leader.
            return Action::PlayKnight;
        }
        if me.roads_left > 0 && obs.hand.contains(&road_cost()) && obs.settlement_sites().is_empty()
        {
            // Extend toward the highest-pip reachable vertex.
            let g = graph();
            let sites = obs.road_sites();
            let best = sites.iter().copied().max_by_key(|&e| {
                let (a, b) = g.edge_ends(e);
                vertex_pips(&obs.layout, a).max(vertex_pips(&obs.layout, b))
            });
            if let Some(edge) = best {
                return Action::BuildRoad { edge };
            }
        }
        // Bank-trade surplus toward settlement ingredients.
        let need = settlement_cost();
        for give in RESOURCES {
            let rate = obs.bank_rate(give);
            if obs.hand.get(give) >= rate + need.get(give) {
                if let Some(want) = RESOURCES
                    .into_iter()
                    .find(|&w| need.get(w) > obs.hand.get(w) && w != give && obs.bank.get(w) > 0)
                {
                    return Action::BankTrade {
                        give,
                        receive: want,
                    };
                }
            }
        }
        Action::EndTurn
    }

    /// Rob the public-VP leader's best hex (never one we touch ourselves).
    fn robber_target(&self, obs: &Observation) -> u8 {
        let g = graph();
        let leader = obs
            .players
            .iter()
            .filter(|p| p.seat != obs.seat)
            .max_by_key(|p| (p.public_vp, p.hand_count))
            .map(|p| p.seat)
            .unwrap_or((obs.seat + 1) % PLAYER_COUNT);
        let buildings = obs.buildings_array();
        let mut best: Option<(u32, u8)> = None;
        for h in 0..crate::catan::board::HEX_COUNT as u8 {
            if h == obs.robber {
                continue;
            }
            let vs = &g.hex_vertices[h as usize];
            let leader_here = vs
                .iter()
                .any(|&v| matches!(buildings[v as usize], Some((o, _)) if o == leader));
            let me_here = vs
                .iter()
                .any(|&v| matches!(buildings[v as usize], Some((o, _)) if o == obs.seat));
            if !leader_here || me_here {
                continue;
            }
            let pips = obs.layout.numbers[h as usize]
                .map(|n| BoardLayout::pips(n) as u32)
                .unwrap_or(0);
            if best.map(|(bp, _)| pips > bp).unwrap_or(true) {
                best = Some((pips, h));
            }
        }
        best.map(|(_, h)| h).unwrap_or_else(|| {
            (0..crate::catan::board::HEX_COUNT as u8)
                .find(|&h| h != obs.robber)
                .expect("some other hex")
        })
    }
}

#[async_trait]
impl SeatAgent for GreedyBuilderBot {
    fn kind(&self) -> &'static str {
        "bot:greedy"
    }
    fn model_id(&self) -> String {
        "bot:greedy".into()
    }
    fn scaffold_version(&self) -> String {
        "bot-greedy-v1".into()
    }

    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        _feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        let action = match decision {
            DecisionPoint::SetupSettlement { .. } => Action::PlaceSetupSettlement {
                vertex: best_by_pips(&obs.layout, &obs.setup_settlement_sites())
                    .expect("open setup site exists"),
            },
            DecisionPoint::SetupRoad { at, .. } => {
                // Point the road at the neighbor vertex with the best pips.
                let g = graph();
                let edge = obs
                    .setup_road_sites(*at)
                    .into_iter()
                    .max_by_key(|&e| {
                        let (a, b) = g.edge_ends(e);
                        let far = if a == *at { b } else { a };
                        vertex_pips(&obs.layout, far)
                    })
                    .expect("setup road site exists");
                Action::PlaceSetupRoad { edge }
            }
            DecisionPoint::PreRoll { .. } => Action::Roll,
            DecisionPoint::DiscardHalf { count, .. } => Action::Discard {
                resources: discard_largest(obs.hand, *count),
            },
            DecisionPoint::MoveRobber { .. } => Action::MoveRobber {
                hex: self.robber_target(obs),
            },
            DecisionPoint::ChooseVictim { victims, .. } => {
                // Steal from the strongest victim.
                let victim = victims
                    .iter()
                    .copied()
                    .max_by_key(|&v| {
                        obs.players
                            .iter()
                            .find(|p| p.seat == v)
                            .map(|p| (p.public_vp, p.hand_count))
                            .unwrap_or((0, 0))
                    })
                    .expect("victims non-empty");
                Action::StealFrom { seat: victim }
            }
            DecisionPoint::TurnAction { .. } => self.turn_action(obs),
            DecisionPoint::FreeRoad { .. } => Action::BuildRoad {
                edge: *obs.road_sites().first().expect("free-road implies a site"),
            },
            DecisionPoint::RespondTrade { offer, .. } => {
                // Accept only if we gain strictly more cards than we pay and
                // can actually pay.
                let gain = offer.give.total();
                let cost = offer.receive.total();
                if gain > cost && obs.hand.contains(&offer.receive) {
                    Action::AcceptTrade { message: None }
                } else {
                    Action::RejectTrade { message: None }
                }
            }
            DecisionPoint::ResolveTrade { accepters, .. } => Action::ResolveTrade {
                partner: accepters.first().copied(),
            },
        };
        Ok(AgentReply {
            action,
            thought: None,
        })
    }
}
