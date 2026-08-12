//! Game and match orchestration for Codenames.

mod game_loop;
mod match_runner;
mod pools;
pub(crate) mod record;
pub mod schedule;

pub use game_loop::{run_game, GameConfig};
pub use match_runner::{
    advance_match, finalize_match, play_plan, MatchProgress, MatchSpec, StoredSpec,
};
pub use pools::{
    rules_from_config, vectors_from_config, wordlist_from_config, AgentFactory, AgentSpec,
};
pub use record::{GameRecord, SeatRecord, RULES_VERSION};
pub use schedule::{
    arena_schedule, controlled_schedule, planned_distribution, rotation_imbalance_note, GamePlan,
    MAX_SCHEDULED_GAMES,
};

pub use crate::codenames::agents::CodenamesAgentKind;
