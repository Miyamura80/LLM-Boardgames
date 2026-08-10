//! Game and match orchestration for Catan.

mod game_loop;
mod pools;
mod record;

pub use game_loop::{run_game, GameConfig};
pub use pools::{AgentFactory, AgentSpec, CatanAgentKind};
pub use record::{placements, GameRecord, SeatRecord, RULES_VERSION};
