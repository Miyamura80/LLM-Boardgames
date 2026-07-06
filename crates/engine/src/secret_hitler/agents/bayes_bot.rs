//! Bayesian-history baseline: tracks votes and policy outcomes in the public
//! transcript and converts them into suspicion scores. Stateless between calls
//! (recomputed from the observation each time), so it is trivially resumable.

use super::{prefer_tile, AgentError, AgentReply, BeliefReport, RoleProbs, SeatAgent};
use crate::secret_hitler::actions::{Action, DecisionPoint};
use crate::secret_hitler::events::{government_enactments, GameEvent};
use crate::secret_hitler::observation::Observation;
use crate::secret_hitler::types::{Party, Role, Seat};
use async_trait::async_trait;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct BayesHistoryBot;

impl BayesHistoryBot {
    pub fn new() -> Self {
        Self
    }

    /// Suspicion score per seat: higher = more fascist-looking. Evidence:
    /// membership in governments that enacted Fascist policies (chancellor
    /// weighted highest), Ja votes for those governments, and the observer's
    /// own private investigation results.
    fn suspicion(obs: &Observation) -> BTreeMap<Seat, f64> {
        let mut score: BTreeMap<Seat, f64> = BTreeMap::new();
        for gov in government_enactments(&obs.history) {
            let sign = match gov.policy {
                Party::Fascist => 1.0,
                Party::Liberal => -1.0,
            };
            *score.entry(gov.chancellor).or_default() += 2.0 * sign;
            *score.entry(gov.president).or_default() += 1.0 * sign;
            for &(s, ja) in &gov.votes {
                if ja {
                    *score.entry(s).or_default() += 0.5 * sign;
                }
            }
        }
        for rec in &obs.history {
            if let GameEvent::InvestigationResult { target, party } = &rec.event {
                // Private, definitive evidence for this observer.
                *score.entry(*target).or_default() += if *party == Party::Fascist {
                    100.0
                } else {
                    -100.0
                };
            }
        }
        score
    }

    fn ranked(obs: &Observation, candidates: &[Seat], most_suspicious_first: bool) -> Vec<Seat> {
        let score = Self::suspicion(obs);
        let mut ranked: Vec<Seat> = candidates.to_vec();
        ranked.sort_by(|a, b| {
            let sa = score.get(a).copied().unwrap_or(0.0);
            let sb = score.get(b).copied().unwrap_or(0.0);
            if most_suspicious_first {
                sb.partial_cmp(&sa).unwrap()
            } else {
                sa.partial_cmp(&sb).unwrap()
            }
        });
        ranked
    }
}

#[async_trait]
impl SeatAgent for BayesHistoryBot {
    fn kind(&self) -> &'static str {
        "bot:bayes-history"
    }
    fn model_id(&self) -> String {
        "bot:bayes-history".into()
    }
    fn scaffold_version(&self) -> String {
        // v2: government fold now shares the canonical helper, which correctly
        // excludes top-decked policies from government attribution.
        "bot-v2".into()
    }

    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        _feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        let liberalish = obs.party == Party::Liberal;
        let action = match decision {
            // Liberals nominate the least suspicious; Fascists ride the same
            // rule to stay camouflaged (deliberately simple anchor).
            DecisionPoint::Nominate { eligible, .. } => Action::Nominate {
                target: Self::ranked(obs, eligible, false)[0],
            },
            DecisionPoint::Vote { seat: _, nominee } => {
                let score = Self::suspicion(obs);
                let s = score.get(nominee).copied().unwrap_or(0.0)
                    + score.get(&obs.public.president).copied().unwrap_or(0.0);
                let ja = if liberalish {
                    s <= 0.0
                } else {
                    s >= 0.0 || obs.public.fascist_policies >= 3
                };
                Action::Vote { ja }
            }
            DecisionPoint::Discard { tiles, .. } => Action::Discard {
                policy: prefer_tile(
                    tiles,
                    if liberalish {
                        Party::Fascist
                    } else {
                        Party::Liberal
                    },
                ),
            },
            DecisionPoint::Enact { tiles, .. } => Action::Enact {
                policy: prefer_tile(
                    tiles,
                    if liberalish {
                        Party::Liberal
                    } else {
                        Party::Fascist
                    },
                ),
            },
            DecisionPoint::VetoConsent { .. } => Action::VetoConsent {
                approve: liberalish,
            },
            DecisionPoint::UsePower { targets, .. } => {
                // Liberals aim at the most suspicious; Fascists at the most
                // trusted (removing credible Liberals).
                Action::UsePower {
                    target: Self::ranked(obs, targets, liberalish)[0],
                }
            }
        };
        Ok(AgentReply {
            action,
            thought: None,
        })
    }

    async fn beliefs(&mut self, obs: &Observation) -> Result<Option<BeliefReport>, AgentError> {
        let score = Self::suspicion(obs);
        let known: BTreeMap<Seat, Role> = obs.known_teammates.iter().copied().collect();
        let prior = super::prior_for_observer(obs.role == Role::Liberal);
        let mut assessments = BTreeMap::new();
        for &s in obs.public.alive.iter().filter(|&&s| s != obs.seat) {
            let probs = match known.get(&s) {
                Some(&role @ (Role::Fascist | Role::Hitler)) => RoleProbs::certain(role),
                _ => {
                    // Squash suspicion into a fascist-probability shift.
                    let x = score.get(&s).copied().unwrap_or(0.0);
                    let shift = 1.0 / (1.0 + (-x / 2.0).exp()); // 0..1, 0.5 = neutral
                    let fasc_mass = (prior.fascist + prior.hitler) * 2.0 * shift;
                    let fasc_mass = fasc_mass.clamp(0.02, 0.95);
                    RoleProbs {
                        liberal: 1.0 - fasc_mass,
                        fascist: fasc_mass * 2.0 / 3.0,
                        hitler: fasc_mass / 3.0,
                    }
                }
            };
            assessments.insert(s, probs.normalized());
        }
        Ok(Some(BeliefReport { assessments }))
    }
}
