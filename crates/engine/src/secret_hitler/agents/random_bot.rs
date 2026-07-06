//! Random-legal baseline: uniformly random over the legal action set. The
//! documented lower bound for the anchor pool — exploitable, no persuasion,
//! never speaks. Seeded, so games against it are reproducible.

use super::{AgentError, AgentReply, BeliefReport, RoleProbs, SeatAgent};
use crate::secret_hitler::actions::{Action, DecisionPoint};
use crate::secret_hitler::observation::Observation;
use crate::secret_hitler::types::Role;
use async_trait::async_trait;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::BTreeMap;

pub struct RandomLegalBot {
    rng: ChaCha8Rng,
}

impl RandomLegalBot {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
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
        "bot-v1".into()
    }

    async fn decide(
        &mut self,
        _obs: &Observation,
        decision: &DecisionPoint,
        _feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        let rng = &mut self.rng;
        let action = match decision {
            DecisionPoint::Nominate { eligible, .. } => Action::Nominate {
                target: eligible[rng.gen_range(0..eligible.len())],
            },
            DecisionPoint::Vote { .. } => Action::Vote {
                ja: rng.gen_bool(0.5),
            },
            DecisionPoint::Discard { tiles, .. } => Action::Discard {
                policy: tiles[rng.gen_range(0..tiles.len())],
            },
            DecisionPoint::Enact {
                tiles, can_veto, ..
            } => {
                if *can_veto && rng.gen_bool(0.2) {
                    Action::ProposeVeto
                } else {
                    Action::Enact {
                        policy: tiles[rng.gen_range(0..tiles.len())],
                    }
                }
            }
            DecisionPoint::VetoConsent { .. } => Action::VetoConsent {
                approve: rng.gen_bool(0.5),
            },
            DecisionPoint::UsePower { targets, .. } => Action::UsePower {
                target: targets[rng.gen_range(0..targets.len())],
            },
        };
        Ok(AgentReply {
            action,
            thought: None,
        })
    }

    async fn beliefs(&mut self, obs: &Observation) -> Result<Option<BeliefReport>, AgentError> {
        // Uninformative priors — the calibration floor.
        let prior = super::prior_for_observer(obs.role == Role::Liberal);
        let assessments: BTreeMap<_, RoleProbs> = obs
            .public
            .alive
            .iter()
            .filter(|&&s| s != obs.seat)
            .map(|&s| (s, prior))
            .collect();
        Ok(Some(BeliefReport { assessments }))
    }
}
