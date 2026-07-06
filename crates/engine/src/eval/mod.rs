//! The evaluation layer on top of the game engine: agents (baseline + LLM), the
//! single-game runner, the match runner, objective metrics, and the Weng-Lin
//! rating. Pure eval logic — the only outbound integration is the LLM client,
//! kept behind the [`Agent`] trait.

pub mod agent;
pub mod baseline;
pub mod llm;
pub mod match_runner;
pub mod metrics;
pub mod rating;
pub mod record;
pub mod runner;

pub use agent::{Agent, AgentError, Beliefs};
pub use baseline::BaselineAgent;
pub use match_runner::{
    outcome_from_records, plan_match, run_match, Distribution, Match, MatchConfig, MatchOutcome,
    SeatPlan,
};
pub use metrics::{aggregate, game_metrics, ModelRoleMetrics, PlayerMetrics};
pub use rating::{compute_ratings, Leaderboard, ModelRating, RoleRating};
pub use record::{BeliefSnapshot, GameRecord, Reliability, SeatAssignment, Usage};
pub use runner::{run_game, RunnerConfig};

#[cfg(test)]
mod tests;
