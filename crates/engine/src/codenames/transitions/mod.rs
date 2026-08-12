//! `GameState::apply` — the single mutation entry point and sole legality
//! authority. Dispatches on (phase, action); every arm either mutates and
//! returns `Ok`, or returns an [`IllegalMove`] whose message is fed back to
//! the agent verbatim on the rethink re-prompt.

pub(crate) mod clue;
mod guess;

use super::actions::{Action, DecisionPoint, IllegalMove};
use super::events::CodenamesEvent;
use super::state::{GameState, Phase};
use super::types::{Seat, Team};
use crate::game_core::Visibility;

impl GameState {
    pub fn apply(&mut self, seat: Seat, action: Action) -> Result<(), IllegalMove> {
        if self.is_over() {
            return Err(IllegalMove("the game is over".into()));
        }
        let phase_name = self.phase.name();
        if !self
            .pending_decisions()
            .iter()
            .any(|d: &DecisionPoint| d.seat() == seat)
        {
            return Err(IllegalMove(format!(
                "seat {seat} has no pending decision in phase {phase_name}"
            )));
        }

        match (self.phase.clone(), action) {
            (Phase::Clue, Action::GiveClue { word, number }) => {
                clue::give_clue(self, seat, &word, number)
            }
            (Phase::Guess { clue, guesses_used }, Action::Guess { word }) => {
                guess::guess(self, seat, &word, &clue, guesses_used)
            }
            (Phase::Guess { guesses_used, .. }, Action::Pass) => {
                guess::pass(self, seat, guesses_used)
            }
            (_, a) => Err(IllegalMove(format!(
                "{} is not a valid action in phase {phase_name}",
                action_name(&a)
            ))),
        }
    }
}

/// The action's JSON tag, for error messages that match what the agent wrote.
fn action_name(a: &Action) -> &'static str {
    match a {
        Action::GiveClue { .. } => "give_clue",
        Action::Guess { .. } => "guess",
        Action::Pass => "pass",
    }
}

/// Hand play to the other team.
pub(super) fn end_turn(state: &mut GameState) {
    begin_turn(state, state.active.other());
}

/// Open `team`'s turn: the turn counter is the transcript's round index.
pub(super) fn begin_turn(state: &mut GameState, team: Team) {
    state.turn += 1;
    state.active = team;
    state.phase = Phase::Clue;
    let turn = state.turn;
    state.push_event(
        Visibility::Public,
        CodenamesEvent::TurnStarted { team, turn },
    );
}
