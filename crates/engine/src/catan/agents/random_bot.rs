//! Uniform-random legal play: the zero-token CI floor. Never trades, never
//! talks, rejects every offer.

use super::{AgentError, AgentReply, SeatAgent};
use crate::catan::actions::{Action, DecisionPoint};
use crate::catan::board::HEX_COUNT;
use crate::catan::observation::Observation;
use crate::catan::types::{DevCard, Resource, ResourceSet, RESOURCES};
use async_trait::async_trait;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub struct RandomLegalBot {
    rng: ChaCha8Rng,
}

impl RandomLegalBot {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> Option<T> {
        if options.is_empty() {
            None
        } else {
            Some(options[self.rng.gen_range(0..options.len())])
        }
    }

    /// A random legal discard of exactly `count` cards from `hand`.
    fn random_discard(&mut self, hand: ResourceSet, count: u8) -> ResourceSet {
        let mut hand = hand;
        let mut out = ResourceSet::default();
        for _ in 0..count {
            let held: Vec<Resource> = RESOURCES.into_iter().filter(|&r| hand.get(r) > 0).collect();
            let r = held[self.rng.gen_range(0..held.len())];
            hand.remove(r, 1);
            out.add(r, 1);
        }
        out
    }

    fn turn_action(&mut self, obs: &Observation) -> Action {
        // A build-leaning random policy that still ends turns reliably.
        match self.rng.gen_range(0..8u8) {
            0 => {
                let sites = obs.road_sites();
                match self.pick(&sites) {
                    Some(edge) if obs.hand.contains(&crate::catan::types::road_cost()) => {
                        Action::BuildRoad { edge }
                    }
                    _ => Action::EndTurn,
                }
            }
            1 => {
                let sites = obs.settlement_sites();
                match self.pick(&sites) {
                    Some(vertex) if obs.hand.contains(&crate::catan::types::settlement_cost()) => {
                        Action::BuildSettlement { vertex }
                    }
                    _ => Action::EndTurn,
                }
            }
            2 => {
                let own: Vec<u8> = obs
                    .occupancy
                    .buildings
                    .iter()
                    .filter(|&&(_, owner, is_city)| owner == obs.seat && !is_city)
                    .map(|&(v, _, _)| v)
                    .collect();
                match self.pick(&own) {
                    Some(vertex) if obs.hand.contains(&crate::catan::types::city_cost()) => {
                        Action::BuildCity { vertex }
                    }
                    _ => Action::EndTurn,
                }
            }
            3 if obs.hand.contains(&crate::catan::types::dev_cost())
                && obs.dev_deck_remaining > 0 =>
            {
                Action::BuyDev
            }
            4 => match self.pick(&obs.devs_playable) {
                Some(DevCard::Knight) => Action::PlayKnight,
                _ => Action::EndTurn,
            },
            _ => Action::EndTurn,
        }
    }
}

#[async_trait]
impl SeatAgent for RandomLegalBot {
    fn kind(&self) -> &'static str {
        "bot:random-legal"
    }
    fn model_id(&self) -> String {
        "bot:random-legal".into()
    }
    fn scaffold_version(&self) -> String {
        "bot-random-v1".into()
    }

    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        _feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        let action = match decision {
            DecisionPoint::SetupSettlement { .. } => {
                let sites = obs.setup_settlement_sites();
                Action::PlaceSetupSettlement {
                    vertex: self.pick(&sites).expect("open setup site exists"),
                }
            }
            DecisionPoint::SetupRoad { at, .. } => {
                let sites = obs.setup_road_sites(*at);
                Action::PlaceSetupRoad {
                    edge: self.pick(&sites).expect("setup road site exists"),
                }
            }
            DecisionPoint::PreRoll { .. } => Action::Roll,
            DecisionPoint::DiscardHalf { count, .. } => Action::Discard {
                resources: self.random_discard(obs.hand, *count),
            },
            DecisionPoint::MoveRobber { .. } => {
                let options: Vec<u8> = (0..HEX_COUNT as u8).filter(|&h| h != obs.robber).collect();
                Action::MoveRobber {
                    hex: self.pick(&options).expect("18 legal hexes"),
                }
            }
            DecisionPoint::ChooseVictim { victims, .. } => Action::StealFrom {
                seat: self.pick(victims).expect("victims non-empty"),
            },
            DecisionPoint::TurnAction { .. } => self.turn_action(obs),
            DecisionPoint::FreeRoad { .. } => {
                let sites = obs.road_sites();
                Action::BuildRoad {
                    edge: self.pick(&sites).expect("free-road phase implies a site"),
                }
            }
            DecisionPoint::RespondTrade { .. } => Action::RejectTrade { message: None },
            DecisionPoint::ResolveTrade { .. } => Action::ResolveTrade { partner: None },
        };
        Ok(AgentReply {
            action,
            thought: None,
        })
    }
}
