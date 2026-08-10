//! The seat-agent contract: anything that can occupy a seat — scripted bots or
//! LLM players — implements [`SeatAgent`].
//!
//! Agents never see [`GameState`](super::state::GameState); they receive only
//! their role-conditioned [`Observation`], so information asymmetry is enforced
//! by construction.

mod bayes_bot;
mod heuristic_bot;
mod llm_agent;
mod prompts;
mod random_bot;

pub use bayes_bot::BayesHistoryBot;
pub use heuristic_bot::HeuristicBot;
pub use llm_agent::LlmSeatAgent;
pub use prompts::scaffold_version;
pub use random_bot::RandomLegalBot;

use super::actions::{Action, DecisionPoint};
use super::observation::Observation;
use super::types::Seat;
use crate::llm::TokenUsage;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use crate::game_core::AgentError;

/// A decision reply: the action plus the model's stated reasoning.
pub type AgentReply = crate::game_core::Reply<Action>;

/// A discussion utterance; `None` text is an explicit pass.
#[derive(Debug, Clone)]
pub struct SpeechReply {
    pub text: Option<String>,
}

/// Probability the subject holds each role. Normalized on ingestion.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct RoleProbs {
    pub liberal: f64,
    pub fascist: f64,
    pub hitler: f64,
}

impl RoleProbs {
    /// Certainty about a known role (Fascists reporting their teammates).
    pub fn certain(role: super::types::Role) -> Self {
        use super::types::Role;
        match role {
            Role::Liberal => Self {
                liberal: 1.0,
                fascist: 0.0,
                hitler: 0.0,
            },
            Role::Fascist => Self {
                liberal: 0.0,
                fascist: 1.0,
                hitler: 0.0,
            },
            Role::Hitler => Self {
                liberal: 0.0,
                fascist: 0.0,
                hitler: 1.0,
            },
        }
    }

    pub fn normalized(mut self) -> Self {
        let clamp = |x: f64| if x.is_finite() { x.max(0.0) } else { 0.0 };
        self.liberal = clamp(self.liberal);
        self.fascist = clamp(self.fascist);
        self.hitler = clamp(self.hitler);
        let sum = self.liberal + self.fascist + self.hitler;
        if sum <= 0.0 {
            return Self {
                liberal: 1.0 / 3.0,
                fascist: 1.0 / 3.0,
                hitler: 1.0 / 3.0,
            };
        }
        self.liberal /= sum;
        self.fascist /= sum;
        self.hitler /= sum;
        self
    }
}

/// A private belief snapshot over every other living player.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BeliefReport {
    pub assessments: BTreeMap<Seat, RoleProbs>,
}

/// One seat's player. `&mut self` lets stateful agents accumulate memory;
/// distinct seats run concurrently (each agent is owned by one seat).
#[async_trait]
pub trait SeatAgent: Send {
    /// Agent family, e.g. `bot:random-legal` or `llm`.
    fn kind(&self) -> &'static str;
    /// Rated model identity, e.g. `gemini/gemini-3-flash-preview` for LLM
    /// seats or `bot:heuristic` for scripted seats.
    fn model_id(&self) -> String;
    /// Version hash of prompt templates + persona + temperature ("scaffold").
    fn scaffold_version(&self) -> String;
    /// Whether this agent participates in discussion.
    fn can_speak(&self) -> bool {
        false
    }
    /// Decide at a decision point. `feedback` carries the parse/illegal-move
    /// reason from the previous failed attempt (the rethink loop).
    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError>;
    /// Produce one discussion utterance (or pass).
    async fn speak(
        &mut self,
        _obs: &Observation,
        _discussion_round: u8,
    ) -> Result<SpeechReply, AgentError> {
        Ok(SpeechReply { text: None })
    }
    /// Private belief elicitation. `None` means the agent family does not
    /// produce calibrated beliefs (excluded from suspicion metrics).
    async fn beliefs(&mut self, _obs: &Observation) -> Result<Option<BeliefReport>, AgentError> {
        Ok(None)
    }
    /// Cumulative token usage (zero for bots).
    fn usage(&self) -> TokenUsage {
        TokenUsage::default()
    }
}

/// Lets the shared `game_core` rethink loop drive any boxed SH seat agent.
#[async_trait]
impl crate::game_core::DecisionAgent<super::state::GameState> for Box<dyn SeatAgent> {
    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        SeatAgent::decide(&mut **self, obs, decision, feedback).await
    }
}

/// The tile of `want` if held, else whatever is held — the shared mechanical
/// policy preference for bot agents.
pub(crate) fn prefer_tile(
    tiles: &[super::types::Party],
    want: super::types::Party,
) -> super::types::Party {
    if tiles.contains(&want) {
        want
    } else {
        tiles[0]
    }
}

/// Baseline prior for a seat OUTSIDE the observer's private knowledge,
/// before any evidence — consistent with each role's 7-player information
/// set (4 Liberals, 2 regular Fascists, 1 Hitler at the table).
///
/// Callers must overlay `Observation::known_teammates` first: for a regular
/// Fascist every unknown seat really is Liberal, but applying this prior to a
/// known teammate would mislabel them.
pub fn prior_for_observer(role: super::types::Role) -> RoleProbs {
    use super::types::Role;
    match role {
        // A Liberal's 6 unknowns: 3L / 2F / 1H.
        Role::Liberal => RoleProbs {
            liberal: 3.0 / 6.0,
            fascist: 2.0 / 6.0,
            hitler: 1.0 / 6.0,
        },
        // A regular Fascist knows every role: unknowns are Liberal.
        Role::Fascist => RoleProbs::certain(Role::Liberal),
        // Hitler's 6 unknowns: 4L / 2F / no second Hitler.
        Role::Hitler => RoleProbs {
            liberal: 4.0 / 6.0,
            fascist: 2.0 / 6.0,
            hitler: 0.0,
        },
    }
    .normalized()
}
