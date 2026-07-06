//! Seedable policy deck: 6 Liberal + 11 Fascist tiles; the discard pile is
//! shuffled back in whenever fewer than 3 tiles remain before a draw.

use super::types::{Party, FASCIST_POLICIES, LIBERAL_POLICIES};
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deck {
    /// Draw pile; the top of the deck is the **end** of the vector.
    tiles: Vec<Party>,
    discard: Vec<Party>,
    /// Number of reshuffles performed (transcript/debug signal).
    pub reshuffles: u32,
}

impl Deck {
    /// Build and shuffle a fresh 17-tile deck from the game RNG.
    pub fn new(rng: &mut ChaCha8Rng) -> Self {
        let mut tiles = Vec::with_capacity((LIBERAL_POLICIES + FASCIST_POLICIES) as usize);
        tiles.extend(std::iter::repeat_n(
            Party::Liberal,
            LIBERAL_POLICIES as usize,
        ));
        tiles.extend(std::iter::repeat_n(
            Party::Fascist,
            FASCIST_POLICIES as usize,
        ));
        tiles.shuffle(rng);
        Self {
            tiles,
            discard: Vec::new(),
            reshuffles: 0,
        }
    }

    pub fn draw_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn discard_count(&self) -> usize {
        self.discard.len()
    }

    /// Reshuffle the discard pile into the draw pile if fewer than 3 tiles
    /// remain. Called before every draw, per the rulebook.
    fn maybe_reshuffle(&mut self, rng: &mut ChaCha8Rng) {
        if self.tiles.len() < 3 {
            self.tiles.append(&mut self.discard);
            self.tiles.shuffle(rng);
            self.reshuffles += 1;
        }
    }

    /// Draw the top `n` tiles (President's legislative draw is 3, an election-
    /// tracker top-deck is 1).
    pub fn draw(&mut self, n: usize, rng: &mut ChaCha8Rng) -> Vec<Party> {
        self.maybe_reshuffle(rng);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            // A 17-tile deck with ≤1 tile enacted per government can never be
            // exhausted mid-draw after the <3 reshuffle, but guard anyway.
            if self.tiles.is_empty() {
                self.maybe_reshuffle(rng);
            }
            if let Some(t) = self.tiles.pop() {
                out.push(t);
            }
        }
        out
    }

    pub fn discard_tile(&mut self, tile: Party) {
        self.discard.push(tile);
    }

    /// Total tiles across draw + discard (excludes enacted policies).
    pub fn total(&self) -> usize {
        self.tiles.len() + self.discard.len()
    }

    /// Build a deck from raw storage (top of the deck at the **end**). Test
    /// support — see `testkit`.
    pub(super) fn from_raw(tiles: Vec<Party>) -> Self {
        Self {
            tiles,
            discard: Vec::new(),
            reshuffles: 0,
        }
    }
}
