//! Engine rules suite (US-002), split into `rules` (scenario coverage) and
//! `properties` (randomized invariants) to stay under the file-length budget.
//! Shared black-box helpers live here.

use super::*;
use Role::{Fascist as RF, Hitler as RH, Liberal as RL};

mod properties;
mod rules;

/// Seats 0–3 Liberal, 4–5 Fascist, 6 Hitler.
pub(super) const LAYOUT: [Role; 7] = [RL, RL, RL, RL, RF, RF, RH];

pub(super) fn ballot(g: &GameState, ja: bool) -> Action {
    Action::CastVotes(g.living_seats().into_iter().map(|s| (s, ja)).collect())
}

pub(super) fn elect(g: &mut GameState, chancellor: usize) {
    g.apply(Action::Nominate(chancellor)).unwrap();
    let b = ballot(g, true);
    g.apply(b).unwrap();
}
