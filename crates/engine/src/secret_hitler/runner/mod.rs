//! Match running: the game loop (rethink + forced defaults + discussion +
//! beliefs), seat/agent specs, schedules, and match orchestration.

pub mod game_loop;
pub mod match_runner;
pub mod pools;
pub mod record;
pub mod schedule;

pub use game_loop::{run_game, GameConfig};
pub use match_runner::{advance_match, finalize_match, MatchProgress, MatchSpec};
pub use pools::{AgentFactory, AgentSpec};
pub use schedule::{arena_schedule, controlled_schedule, GamePlan};
