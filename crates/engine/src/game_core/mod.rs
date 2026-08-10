//! Game-agnostic engine machinery shared by every game in the harness.
//!
//! Extraction rule (PRD-catan-evals §6): code lives here **only** if the
//! per-game implementations would be textually identical up to the associated
//! types — the agent-driving contracts and the rethink loop, the
//! visibility-filtered event log, reliability accounting, and schedule seeds.
//! Rules, prompts, metrics, rating math, and report rendering stay in each
//! game's own module.

mod agent;
mod events;
mod rethink;
mod seeds;
mod state;

pub use agent::{AgentError, DecisionAgent, Reply};
pub use events::{EventRecord, Visibility};
pub use rethink::{resolve_decision, DecisionOutcome, Reliability};
pub use seeds::cell_seed;
pub use state::{DecisionOps, EngineState, IllegalMove, Seat};
