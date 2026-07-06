//! Black-box test seams: crate-visible constructors that let the rules suite
//! control roles, deck order, and board position deterministically. As a child
//! module of `state`, this retains access to `GameState`'s private fields.

use super::{GameState, Phase, Player};
use crate::game::board::Board;
use crate::game::config::{GameConfig, NUM_PLAYERS};
use crate::game::log::GameLog;
use crate::game::policy::{Deck, Policy};
use crate::game::rng::Rng;
use crate::game::roles::Role;
use std::collections::BTreeMap;

impl GameState {
    pub(crate) fn test_game(
        roles: [Role; NUM_PLAYERS],
        draw: Vec<Policy>,
        first_president: usize,
    ) -> Self {
        let config = GameConfig::new(0).with_first_president(first_president);
        let rng = Rng::new(config.seed);
        let aux = rng.fork(0xF0);
        let players = roles
            .iter()
            .enumerate()
            .map(|(seat, &role)| Player {
                seat,
                role,
                alive: true,
            })
            .collect();
        let mut state = Self {
            config,
            rng,
            aux,
            players,
            deck: Deck::from_draw(draw),
            board: Board::default(),
            election_tracker: 0,
            president: first_president,
            chancellor: None,
            last_president: None,
            last_chancellor: None,
            phase: Phase::Nomination,
            pending_special: None,
            special_return: None,
            current_is_special: false,
            investigated: Vec::new(),
            investigation_results: BTreeMap::new(),
            winner: None,
            log: GameLog::default(),
        };
        state.emit_setup();
        state
    }

    /// Jump the enacted-policy counters (to reach a threshold quickly).
    pub(crate) fn test_set_board(&mut self, liberal: u8, fascist: u8) {
        self.board.liberal = liberal;
        self.board.fascist = fascist;
    }

    /// Tiles still in draw+discard piles (not yet enacted). For the tile
    /// conservation invariant.
    pub(crate) fn deck_in_circulation(&self) -> usize {
        self.deck.tiles_in_circulation()
    }
}
