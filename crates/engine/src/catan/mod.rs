//! Settlers of Catan (4-player base game) — the second game on the harness.
//! See `docs/PRD-catan-evals.md`.
//!
//! Layering mirrors `secret_hitler`:
//!
//! ```text
//! types / board / events / state / transitions / longest_road  – rules engine
//! actions / observation                                        – agent contract
//! agents/                                                      – bots + LLM seats
//! runner/                                                      – schedules + loop
//! metrics / rating / store                                     – eval outputs
//! ```

pub mod actions;
pub mod agents;
pub mod board;
pub mod events;
pub mod longest_road;
pub mod observation;
mod render;
pub mod runner;
pub mod sites;
pub mod state;
pub mod testkit;
mod transitions;
pub mod types;

use crate::game_core::{EngineState, IllegalMove, Seat};

impl state::GameState {
    /// Recompute and settle the Longest Road title (exposed for tests and
    /// metrics; the engine calls this after every relevant placement).
    pub fn recompute_longest_road(&mut self) {
        transitions::build::award_longest_road(self);
    }

    /// Settle the Largest Army title for a seat (exposed for tests).
    pub fn settle_largest_army(&mut self, seat: Seat) {
        transitions::build::award_largest_army(self, seat);
    }
}

/// Lets the shared `game_core` rethink loop drive the Catan engine. Pure
/// delegation to the inherent methods — behavior is defined there.
impl EngineState for state::GameState {
    type Action = actions::Action;
    type Observation = observation::Observation;
    type Decision = actions::DecisionPoint;

    fn pending_decisions(&self) -> Vec<Self::Decision> {
        state::GameState::pending_decisions(self)
    }
    fn observe(&self, seat: Seat) -> Self::Observation {
        state::GameState::observe(self, seat)
    }
    fn apply(&mut self, seat: Seat, action: Self::Action) -> Result<(), IllegalMove> {
        state::GameState::apply(self, seat, action)
    }
    fn forced_default(&mut self, decision: &Self::Decision) -> Self::Action {
        state::GameState::forced_default(self, decision)
    }
    fn push_forced_default(&mut self, seat: Seat, kind: &str) {
        state::GameState::push_forced_default(self, seat, kind)
    }
    fn is_over(&self) -> bool {
        state::GameState::is_over(self)
    }
}
