//! Deterministic, deliberately-simple rule-based agents (US-005).
//!
//! These are a cheap, reproducible **floor** and CI smoke test — not the
//! leaderboard. They play legal, coherent, mechanical moves with **no**
//! discussion and no bluffing: a President keeps a tile that helps its own
//! faction, a Chancellor enacts one that does, powers target the first legal
//! seat (a Fascist avoids shooting a known teammate). They are exploitable by
//! design and exercise zero persuasion.

use crate::eval::agent::{Agent, AgentError};
use crate::game::{Action, Decision, DecisionKind, Faction, Observation, Policy, Role};
use async_trait::async_trait;
use std::collections::BTreeMap;

/// A scripted baseline seat.
#[derive(Debug, Clone)]
pub struct BaselineAgent {
    name: String,
}

impl Default for BaselineAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl BaselineAgent {
    pub fn new() -> Self {
        Self {
            name: "baseline/v1".to_string(),
        }
    }

    /// A baseline with a distinct identity (so a table of baselines has distinct
    /// rated entities — used to exercise the match runner/rating).
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    fn own_faction(role: Role) -> Faction {
        role.faction()
    }

    /// Index of the first tile matching `want`, else 0 (a legal fallback).
    fn pick_tile(tiles: &[Policy], want: Policy) -> usize {
        tiles.iter().position(|&t| t == want).unwrap_or(0)
    }
}

#[async_trait]
impl Agent for BaselineAgent {
    fn name(&self) -> String {
        self.name.clone()
    }

    async fn act(
        &self,
        obs: &Observation,
        decision: &Decision,
        _feedback: Option<&str>,
    ) -> Result<Action, AgentError> {
        let faction = Self::own_faction(obs.your_role);
        let teammates = obs.known_team.as_ref();

        let action = match &decision.kind {
            DecisionKind::Nominate { eligible, .. } => {
                // Fascist prefers a known teammate as Chancellor; else first legal.
                let choice = teammates
                    .and_then(|t| {
                        eligible
                            .iter()
                            .find(|s| t.fascists.contains(s) || **s == t.hitler)
                            .copied()
                    })
                    .unwrap_or(eligible[0]);
                Action::Nominate(choice)
            }
            // Vote: report only this seat's own vote. Baselines vote Ja to keep
            // the game progressing (deliberately naive — no suspicion model).
            DecisionKind::Vote { .. } => Action::CastVotes(BTreeMap::from([(obs.you, true)])),
            DecisionKind::PresidentDiscard { drawn, .. } => {
                // Discard the tile that helps the *other* faction.
                let discard_target = match faction {
                    Faction::Liberal => Policy::Fascist,
                    Faction::Fascist => Policy::Liberal,
                };
                Action::Discard(Self::pick_tile(drawn, discard_target))
            }
            DecisionKind::ChancellorEnact { hand, .. } => {
                let enact_target = match faction {
                    Faction::Liberal => Policy::Liberal,
                    Faction::Fascist => Policy::Fascist,
                };
                Action::Enact(Self::pick_tile(hand, enact_target))
            }
            // Baselines never initiate/accept a veto (keeps the floor simple).
            DecisionKind::VetoConsent { .. } => Action::VetoConsent(false),
            DecisionKind::Investigate { eligible, .. } => Action::Investigate(eligible[0]),
            DecisionKind::SpecialElection { eligible, .. } => Action::SpecialElection(eligible[0]),
            DecisionKind::Execution { eligible, .. } => {
                // A Fascist avoids executing a known teammate.
                let choice = teammates
                    .map(|t| {
                        eligible
                            .iter()
                            .find(|s| !t.fascists.contains(s) && **s != t.hitler)
                            .copied()
                            .unwrap_or(eligible[0])
                    })
                    .unwrap_or(eligible[0]);
                Action::Execute(choice)
            }
        };
        Ok(action)
    }
}
