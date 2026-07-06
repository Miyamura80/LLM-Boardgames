//! Simple mechanical baseline: legal, coherent, deliberately weak. Follows a
//! fixed party-line strategy with no memory and no discussion.

use super::{prefer_tile, AgentError, AgentReply, BeliefReport, RoleProbs, SeatAgent};
use crate::secret_hitler::actions::{Action, DecisionPoint};
use crate::secret_hitler::observation::Observation;
use crate::secret_hitler::types::{Party, Role, Seat};
use async_trait::async_trait;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct HeuristicBot;

impl HeuristicBot {
    pub fn new() -> Self {
        Self
    }

    fn known_fascists(obs: &Observation) -> Vec<Seat> {
        obs.known_teammates.iter().map(|(s, _)| *s).collect()
    }
}

#[async_trait]
impl SeatAgent for HeuristicBot {
    fn kind(&self) -> &'static str {
        "bot:heuristic"
    }
    fn model_id(&self) -> String {
        "bot:heuristic".into()
    }
    fn scaffold_version(&self) -> String {
        "bot-v1".into()
    }

    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        _feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        let my_party = obs.party;
        let action = match decision {
            // Fascists prefer a teammate; everyone else takes the first
            // eligible seat in table order.
            DecisionPoint::Nominate { eligible, .. } => {
                let teammates = Self::known_fascists(obs);
                let pick = eligible
                    .iter()
                    .find(|s| teammates.contains(s))
                    .or_else(|| eligible.first())
                    .copied()
                    .expect("eligible list is never empty");
                Action::Nominate { target: pick }
            }
            // Mechanical Ja keeps games moving (documented exploitable floor).
            DecisionPoint::Vote { .. } => Action::Vote { ja: true },
            DecisionPoint::Discard { tiles, .. } => {
                let discard = match my_party {
                    Party::Liberal => prefer_tile(tiles, Party::Fascist),
                    Party::Fascist => prefer_tile(tiles, Party::Liberal),
                };
                Action::Discard { policy: discard }
            }
            DecisionPoint::Enact { tiles, .. } => {
                let enact = match my_party {
                    Party::Liberal => prefer_tile(tiles, Party::Liberal),
                    Party::Fascist => prefer_tile(tiles, Party::Fascist),
                };
                Action::Enact { policy: enact }
            }
            // A Liberal president trusts a chancellor who claims a forced
            // fascist hand; approve. Fascists decline to keep tiles flowing.
            DecisionPoint::VetoConsent { .. } => Action::VetoConsent {
                approve: my_party == Party::Liberal,
            },
            DecisionPoint::UsePower { targets, .. } => {
                let teammates = Self::known_fascists(obs);
                // Fascists aim powers at non-teammates; Liberals take the
                // first target in seat order.
                let pick = targets
                    .iter()
                    .find(|s| my_party == Party::Fascist && !teammates.contains(s))
                    .or_else(|| targets.first())
                    .copied()
                    .expect("targets list is never empty");
                Action::UsePower { target: pick }
            }
        };
        Ok(AgentReply {
            action,
            thought: None,
        })
    }

    async fn beliefs(&mut self, obs: &Observation) -> Result<Option<BeliefReport>, AgentError> {
        // Fascists report what they actually know; Liberals report priors.
        let mut assessments: BTreeMap<Seat, RoleProbs> = BTreeMap::new();
        let known: BTreeMap<Seat, Role> = obs.known_teammates.iter().copied().collect();
        for &s in obs.public.alive.iter().filter(|&&s| s != obs.seat) {
            let probs = match known.get(&s) {
                Some(&role) => RoleProbs::certain(role),
                // Everyone a regular Fascist doesn't know is Liberal.
                None if obs.role == Role::Fascist => RoleProbs::certain(Role::Liberal),
                None => super::prior_for_observer(obs.role),
            };
            assessments.insert(s, probs.normalized());
        }
        Ok(Some(BeliefReport { assessments }))
    }
}
