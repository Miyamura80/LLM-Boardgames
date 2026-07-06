//! The evaluation layer on top of the game engine: agents (baseline + LLM), the
//! single-game runner, the match runner, objective metrics, and the Weng-Lin
//! rating. Pure eval logic — the only outbound integration is the LLM client,
//! kept behind the [`Agent`] trait.

pub mod agent;
pub mod baseline;
pub mod llm;
pub mod record;
pub mod runner;

pub use agent::{Agent, AgentError, Beliefs};
pub use baseline::BaselineAgent;
pub use record::{BeliefSnapshot, GameRecord, Reliability, SeatAssignment, Usage};
pub use runner::{run_game, RunnerConfig};

#[cfg(test)]
mod tests;
