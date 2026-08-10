//! The minimal agent contract the generic rethink loop drives. Per-game seat
//! traits (discussion, beliefs, negotiation, identity metadata) extend this in
//! their own modules.

use super::state::EngineState;
use async_trait::async_trait;

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
pub struct Reply<A> {
    pub action: A,
    pub thought: Option<String>,
}

/// Anything that can answer a decision point for game `S`. `feedback` carries
/// the parse/illegal-move reason from the previous failed attempt (the rethink
/// loop).
#[async_trait]
pub trait DecisionAgent<S: EngineState>: Send {
    async fn decide(
        &mut self,
        obs: &S::Observation,
        decision: &S::Decision,
        feedback: Option<&str>,
    ) -> Result<Reply<S::Action>, AgentError>;
}
