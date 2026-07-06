//! The `Agent` abstraction: anything that can occupy a seat — a scripted
//! baseline or an LLM. Agents are asked to *decide* one [`Decision`] at a time
//! and (optionally) to *speak* during discussion and to *report private beliefs*
//! for the suspicion-accuracy metric.
//!
//! The parse → legal-check → rethink → forced-default loop (FR-4) lives in the
//! runner, not here: an agent just returns its best proposed [`Action`]; the
//! runner validates it against the engine and re-prompts on illegal moves.

use crate::game::{Action, Decision, Observation};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Why an agent failed to produce a usable action this attempt. Distinct from an
/// *illegal* move (which is a well-formed action the engine rejects): these are
/// reliability failures counted separately from play quality (FR-5).
#[derive(Debug, Clone, thiserror::Error)]
pub enum AgentError {
    /// Output could not be parsed / did not satisfy the action schema.
    #[error("malformed output: {0}")]
    Malformed(String),
    /// Transient provider/API failure (already retry-wrapped by the client).
    #[error("api error: {0}")]
    Api(String),
}

/// A private belief snapshot: for each *other* living seat, the probability the
/// agent assigns to that seat being on the Fascist team. Scored (Brier) against
/// ground truth for the suspicion-accuracy metric (US-010). Never shown to other
/// players; does not affect game state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Beliefs {
    pub fascist_prob: BTreeMap<usize, f64>,
}

/// A seat occupant.
#[async_trait]
pub trait Agent: Send + Sync {
    /// Stable identifier for the rated unit ("model + scaffold").
    fn name(&self) -> String;

    /// Decide an action for `decision`, given this seat's `observation`.
    ///
    /// For a simultaneous `Vote` decision the agent returns a
    /// [`Action::CastVotes`] carrying **only its own seat's** vote; the runner
    /// aggregates. `feedback` carries the reason a previous attempt was illegal
    /// (rethink); it is `None` on the first attempt.
    async fn act(
        &self,
        observation: &Observation,
        decision: &Decision,
        feedback: Option<&str>,
    ) -> Result<Action, AgentError>;

    /// A discussion utterance for the current round, or `None`/empty to pass.
    /// Baselines stay silent.
    async fn discuss(&self, _observation: &Observation, _round: u8) -> Option<String> {
        None
    }

    /// Private belief elicitation at a checkpoint. `None` = agent abstains
    /// (baselines); such abstentions are excluded from suspicion scoring.
    async fn beliefs(&self, _observation: &Observation) -> Option<Beliefs> {
        None
    }

    /// Token usage this agent accumulated (LLM agents override with interior
    /// mutability). Read once at game end and summed into the record.
    fn take_usage(&self) -> crate::eval::record::Usage {
        crate::eval::record::Usage::default()
    }
}
