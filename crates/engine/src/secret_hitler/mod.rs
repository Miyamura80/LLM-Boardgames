//! Secret Hitler: authoritative 7-player game engine and eval harness.
//!
//! Layering (transport-free, per the engine crate's design rules):
//!
//! ```text
//!   types / deck / events / state / transitions   – the rules engine
//!   actions / observation                         – the agent contract
//!   agents/                                       – bots + LLM seats
//!   discussion / beliefs                          – runner-managed phases
//!   runner/                                       – schedules + game loop
//!   metrics / rating                              – objective eval outputs
//!   store/                                        – Postgres persistence
//! ```

pub mod actions;
pub mod agents;
pub mod deck;
pub mod events;
pub mod metrics;
pub mod observation;
pub mod rating;
pub mod runner;
pub mod state;
pub mod store;
pub mod testkit;
pub mod transitions;
pub mod types;
