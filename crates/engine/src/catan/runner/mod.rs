//! Game and match orchestration for Catan.

mod game_loop;
mod match_runner;
mod pools;
mod record;
mod schedule;

pub use game_loop::{run_game, GameConfig};
pub use match_runner::{advance_match, finalize_match, play_plan, MatchProgress, MatchSpec};
pub use pools::{AgentFactory, AgentSpec, CatanAgentKind};
pub use record::{placements, GameRecord, SeatRecord, RULES_VERSION};
pub use schedule::{arena_schedule, controlled_schedule, GamePlan};
