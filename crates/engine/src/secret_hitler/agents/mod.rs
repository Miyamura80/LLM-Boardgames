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

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// Output could not be parsed into the required strict JSON schema.
    /// Counts toward the malformed-output reliability counter.
    #[error("malformed output: {0}")]
    Malformed(String),
    /// The model API failed even after transport-level retries.
    #[error("transport: {0}")]
    Transport(String),
}

/// A decision reply: the action plus the model's stated reasoning.
#[derive(Debug, Clone)]
pub struct AgentReply {
    pub action: Action,
    pub thought: Option<String>,
}

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

/// Baseline priors from one seat's perspective over the 6 other players,
/// before any evidence: 7-player game has 4 Liberals, 2 Fascists, 1 Hitler.
pub fn prior_for_observer(observer_is_liberal: bool) -> RoleProbs {
    // A Liberal looks at 6 others containing 3L/2F/1H; a Fascist's unknowns
    // are Liberals only, but bots that know roles report the truth instead.
    if observer_is_liberal {
        RoleProbs {
            liberal: 3.0 / 6.0,
            fascist: 2.0 / 6.0,
            hitler: 1.0 / 6.0,
        }
        .normalized()
    } else {
        RoleProbs {
            liberal: 1.0 / 3.0,
            fascist: 1.0 / 3.0,
            hitler: 1.0 / 3.0,
        }
        .normalized()
    }
}
