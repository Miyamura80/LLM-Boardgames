//! The authoritative Secret Hitler game engine (v1: 7 players).
//!
//! This module tree is the single source of truth for game state and rules
//! (FR-1). It has no transport or LLM dependency — agents (LLM or scripted)
//! interact only through [`Observation`]s and [`Action`]s, and every metric
//! downstream derives from [`GameState`] and its [`GameLog`].
//!
//! ```text
//!   GameState (rules + seeded RNG)
//!      │  current_decision() ─► Decision (whose seat, legal options)
//!      │  observation(seat)  ─► Observation (public + this seat's private view)
//!      ▼  apply(Action)      ─► Vec<Event>  |  Err(IllegalAction)
//!   GameLog (public + per-seat private events = the transcript spine)
//! ```

pub mod action;
pub mod board;
pub mod config;
pub mod log;
pub mod observation;
pub mod policy;
pub mod rng;
pub mod roles;
pub mod state;

pub use action::{Action, Decision, DecisionKind, IllegalAction};
pub use board::{Board, Power};
pub use config::{GameConfig, NUM_PLAYERS};
pub use log::{Event, GameLog, LogEntry, WinReason};
pub use observation::{KnownTeam, Observation, PlayerView};
pub use policy::{Deck, Policy};
pub use rng::Rng;
pub use roles::{Faction, Party, Role};
pub use state::{GameState, Player};

#[cfg(test)]
mod tests;
