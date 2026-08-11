//! Game orchestration for Codenames.
//!
//! The match layer (mirrored-board schedules, `codenames_run_match`, ratings)
//! is build-order step 4 and lands in its own phase; this module stops at one
//! game.

mod game_loop;
mod pools;
mod record;

pub use game_loop::{run_game, GameConfig};
pub use pools::{rules_from_config, wordlist_from_config, AgentFactory, AgentSpec};
pub use record::{GameRecord, SeatRecord, RULES_VERSION};

pub use crate::codenames::agents::CodenamesAgentKind;
