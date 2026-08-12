//! Codenames (standard 2-team game, 4 seats) — the third game on the harness.
//! See `docs/PRD-codenames-evals.md`.
//!
//! Layering mirrors `catan`:
//!
//! ```text
//! types / wordlist / board / events / state / transitions  – rules engine
//! actions / observation                                    – agent contract
//! agents                                                   – bots + LLM seats
//! runner                                                   – one game, one match
//! metrics / rating / store                                 – the eval layer
//! testkit                                                  – scripted games
//! ```
//!
//! The game's only hidden information is the key card, and it lives in state:
//! spymaster observations derive it in `observe()`, so every event is
//! `Public` and `game_core` needs no team visibility (PRD §9 decision 8).

pub mod actions;
pub mod agents;
pub mod board;
pub mod events;
pub mod metrics;
pub mod observation;
pub mod rating;
pub mod runner;
pub mod state;
pub mod store;
mod store_codec;
pub mod testkit;
mod transitions;
pub mod types;
pub mod wordlist;

pub use types::RULES_VERSION;

use crate::game_core::{EngineState, IllegalMove, Seat};

/// Lets the shared `game_core` rethink loop drive the Codenames engine. Pure
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

#[cfg(test)]
mod tests {
    use super::actions::DecisionPoint;
    use super::types::*;
    use super::{state, testkit};
    use crate::game_core::EngineState;

    /// Every game terminates, and exactly one seat is ever on the clock. The
    /// last seed plays on the vendored wordlist, so a curation pass that
    /// broke forced defaults would fail here.
    #[test]
    fn forced_default_games_terminate_with_a_single_pending_seat() {
        for seed in 0..12u64 {
            let mut state = if seed == 11 {
                state::GameState::new(seed)
            } else {
                testkit::scripted_game(seed)
            };
            let mut steps = 0;
            while !state.is_over() {
                let pending = state.pending_decisions();
                assert_eq!(pending.len(), 1, "codenames is fully sequential");
                let decision = pending[0].clone();
                let expected_seat = match &decision {
                    DecisionPoint::GiveClue { team, .. } => team.spymaster(),
                    DecisionPoint::GuessOrPass { team, .. } => team.operative(),
                };
                assert_eq!(decision.seat(), expected_seat);
                assert_eq!(seat_team(decision.seat()), state.active);

                let action = state.forced_default(&decision);
                state
                    .apply(decision.seat(), action)
                    .expect("forced defaults are legal by construction");
                steps += 1;
                assert!(steps < 500, "game failed to terminate");
            }
            // Revealed-card accounting is conserved, and the winner is real.
            let revealed = state.board.cards.iter().filter(|c| c.revealed).count();
            assert!((1..=CARD_COUNT).contains(&revealed));
            assert!(state.winner.is_some() && state.end_reason.is_some());
            assert!(state.pending_decisions().is_empty());
        }
    }

    /// The shared loop drives Codenames through the trait, not the inherent
    /// methods — this pins the delegation above to the `game_core` contract.
    #[test]
    fn the_engine_state_contract_is_implemented() {
        fn pending<E: EngineState>(state: &E) -> usize {
            state.pending_decisions().len()
        }
        let state = testkit::scripted_game(1);
        assert_eq!(pending(&state), 1);
    }
}
