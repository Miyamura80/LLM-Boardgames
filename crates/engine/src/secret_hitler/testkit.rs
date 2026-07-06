//! Test support: deterministic game construction with scripted roles and deck
//! order. Used by the rules test suite; not part of the eval surface.

use super::deck::Deck;
use super::state::GameState;
use super::types::{Party, Role, PLAYER_COUNT};

impl GameState {
    /// Build a game with explicit roles and an explicit deck order.
    ///
    /// `deck_top_first` lists tiles in draw order (first element = first tile
    /// drawn). Roles must contain exactly 4 Liberals, 2 Fascists and 1 Hitler.
    pub fn new_scripted(
        seed: u64,
        roles: [Role; PLAYER_COUNT as usize],
        deck_top_first: &[Party],
    ) -> Self {
        let mut state = Self::new(seed);
        // Rewrite roles, then re-deal knowledge events to match.
        state.events.clear();
        for (i, role) in roles.iter().enumerate() {
            state.players[i].role = *role;
        }
        state.push_event(
            super::events::Visibility::Public,
            super::events::GameEvent::GameStarted {
                players: PLAYER_COUNT,
            },
        );
        state.redeal_knowledge_for_test();
        state.deck = Deck::from_tiles_top_first(deck_top_first);
        state
    }
}

impl Deck {
    /// Build a deck with an explicit draw order (first element drawn first).
    pub fn from_tiles_top_first(tiles_top_first: &[Party]) -> Self {
        let mut tiles: Vec<Party> = tiles_top_first.to_vec();
        tiles.reverse(); // storage keeps the top of the deck at the end
        Self::from_raw(tiles)
    }
}
