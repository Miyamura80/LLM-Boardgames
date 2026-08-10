//! The Catan seat-agent contract. Unlike Secret Hitler there is no separate
//! discussion/belief surface: table talk rides inside trade actions and `say`,
//! so `decide` is the whole interface.

mod greedy_bot;
mod llm_agent;
pub mod prompts;
mod random_bot;

pub use greedy_bot::GreedyBuilderBot;
pub use llm_agent::LlmSeatAgent;
pub use prompts::scaffold_version;
pub use random_bot::RandomLegalBot;

use super::actions::{Action, DecisionPoint};
use super::observation::Observation;
use super::state::GameState;
use crate::llm::TokenUsage;
use async_trait::async_trait;

pub use crate::game_core::AgentError;

/// A decision reply: the action plus the model's stated reasoning.
pub type AgentReply = crate::game_core::Reply<Action>;

/// One seat's player. Agents never see [`GameState`]; they receive only their
/// visibility-filtered [`Observation`].
#[async_trait]
pub trait SeatAgent: Send {
    /// Agent family, e.g. `bot:random-legal` or `llm`.
    fn kind(&self) -> &'static str;
    /// Rated model identity, e.g. `gemini/gemini-3-flash-preview` for LLM
    /// seats or `bot:greedy` for scripted seats.
    fn model_id(&self) -> String;
    /// Version hash of prompt templates + persona + temperature ("scaffold").
    fn scaffold_version(&self) -> String;
    /// Decide at a decision point. `feedback` carries the parse/illegal-move
    /// reason from the previous failed attempt (the rethink loop).
    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError>;
    /// Cumulative token usage (zero for bots).
    fn usage(&self) -> TokenUsage {
        TokenUsage::default()
    }
}

/// Lets the shared `game_core` rethink loop drive any boxed Catan seat agent.
#[async_trait]
impl crate::game_core::DecisionAgent<GameState> for Box<dyn SeatAgent> {
    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        SeatAgent::decide(&mut **self, obs, decision, feedback).await
    }
}
