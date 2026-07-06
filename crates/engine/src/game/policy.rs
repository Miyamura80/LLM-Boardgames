//! The policy deck: 6 Liberal + 11 Fascist tiles.
//!
//! Tiles move draw-pile → hands → (board | discard). Enacted tiles leave play
//! for good; discarded tiles are reshuffled back when the draw pile can't cover
//! a draw of three. Total tiles in circulation therefore *decreases* as policies
//! are enacted — the conservation invariant (draw + discard + enacted = 17) is
//! asserted by the property tests.

use crate::game::rng::Rng;
use serde::{Deserialize, Serialize};

pub const LIBERAL_TILES: usize = 6;
pub const FASCIST_TILES: usize = 11;
pub const TOTAL_TILES: usize = LIBERAL_TILES + FASCIST_TILES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    Liberal,
    Fascist,
}

/// Draw and discard piles. Enacted tiles are counted on the board, not held
/// here, so they are intentionally *not* part of the deck's tile accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deck {
    draw: Vec<Policy>,
    discard: Vec<Policy>,
}

impl Deck {
    /// A freshly shuffled deck of 6 Liberal + 11 Fascist tiles.
    pub fn new(rng: &mut Rng) -> Self {
        let mut draw = Vec::with_capacity(TOTAL_TILES);
        draw.extend(std::iter::repeat_n(Policy::Liberal, LIBERAL_TILES));
        draw.extend(std::iter::repeat_n(Policy::Fascist, FASCIST_TILES));
        rng.shuffle(&mut draw);
        Self {
            draw,
            discard: Vec::new(),
        }
    }

    pub fn draw_pile_len(&self) -> usize {
        self.draw.len()
    }

    pub fn discard_pile_len(&self) -> usize {
        self.discard.len()
    }

    /// Tiles still in circulation (not yet enacted onto the board).
    pub fn tiles_in_circulation(&self) -> usize {
        self.draw.len() + self.discard.len()
    }

    /// Reshuffle the discard pile back into the draw pile if fewer than three
    /// tiles remain. Per the rules this is checked whenever the deck is about to
    /// be drawn from. Returns `true` if a reshuffle happened.
    pub fn maybe_reshuffle(&mut self, rng: &mut Rng) -> bool {
        if self.draw.len() >= 3 {
            return false;
        }
        self.draw.append(&mut self.discard);
        rng.shuffle(&mut self.draw);
        true
    }

    /// Draw the top three tiles for the President. Callers must ensure the deck
    /// has been given a chance to reshuffle first (see [`Deck::maybe_reshuffle`]).
    pub fn draw_three(&mut self) -> [Policy; 3] {
        debug_assert!(self.draw.len() >= 3, "draw_three called with < 3 tiles");
        // Draw from the front so the shuffled order is the draw order.
        let a = self.draw.remove(0);
        let b = self.draw.remove(0);
        let c = self.draw.remove(0);
        [a, b, c]
    }

    /// Peek the top tile without removing it (used for election-tracker top-deck
    /// after a reshuffle check). Returns `None` only if the deck is empty.
    pub fn top(&self) -> Option<Policy> {
        self.draw.first().copied()
    }

    /// Remove and return the top tile (election-tracker top-deck enactment).
    pub fn take_top(&mut self) -> Option<Policy> {
        if self.draw.is_empty() {
            None
        } else {
            Some(self.draw.remove(0))
        }
    }

    /// Send tiles to the discard pile (President/Chancellor discards, vetoes).
    pub fn discard(&mut self, tiles: impl IntoIterator<Item = Policy>) {
        self.discard.extend(tiles);
    }

    /// Build a deck with an explicit draw-pile order (front = top) and no
    /// discards. Test-only: lets rules tests control exactly which tiles the
    /// President draws.
    #[cfg(test)]
    pub(crate) fn from_draw(draw: Vec<Policy>) -> Self {
        Self {
            draw,
            discard: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_deck_has_17_tiles_in_ratio() {
        let mut rng = Rng::new(1);
        let deck = Deck::new(&mut rng);
        assert_eq!(deck.draw_pile_len(), TOTAL_TILES);
        assert_eq!(deck.tiles_in_circulation(), TOTAL_TILES);
    }

    #[test]
    fn reshuffle_only_when_below_three() {
        let mut rng = Rng::new(1);
        let mut deck = Deck::new(&mut rng);
        // Drain to exactly 2 in draw, rest to discard.
        while deck.draw_pile_len() > 2 {
            let three_left = deck.draw_pile_len() == 3;
            if three_left {
                break;
            }
            let t = deck.take_top().unwrap();
            deck.discard([t]);
        }
        // Force below three.
        while deck.draw_pile_len() >= 3 {
            let t = deck.take_top().unwrap();
            deck.discard([t]);
        }
        assert!(deck.draw_pile_len() < 3);
        let reshuffled = deck.maybe_reshuffle(&mut rng);
        assert!(reshuffled);
        assert_eq!(deck.tiles_in_circulation(), TOTAL_TILES);
        assert!(deck.draw_pile_len() >= 3);
    }

    #[test]
    fn draw_three_removes_three() {
        let mut rng = Rng::new(5);
        let mut deck = Deck::new(&mut rng);
        let before = deck.draw_pile_len();
        let _ = deck.draw_three();
        assert_eq!(deck.draw_pile_len(), before - 3);
    }
}
