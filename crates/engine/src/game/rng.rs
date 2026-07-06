//! A tiny, dependency-free, deterministic RNG (SplitMix64).
//!
//! The engine owns *all* randomness — deck shuffles, role assignment, and
//! forced-default tie-breaks — so that a game is bit-for-bit reproducible from
//! its seed (US-002). We roll our own instead of pulling in `rand` to keep the
//! stream stable across dependency upgrades: the same seed must reproduce the
//! same transcript forever.

use serde::{Deserialize, Serialize};

/// Deterministic PRNG. Cheap to clone; two clones from the same state produce
/// identical streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Avoid the all-zero fixed point of SplitMix64's mixing.
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Next raw 64-bit value (SplitMix64).
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform integer in `[0, n)`. `n` must be non-zero.
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0, "Rng::below(0) is undefined");
        // Rejection-free modulo is fine here: n is tiny (≤17 tiles, ≤7 seats),
        // so the modulo bias is negligible and reproducibility is what matters.
        (self.next_u64() % n as u64) as usize
    }

    /// In-place Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }

    /// Derive an independent sub-stream. Used so forced-default tie-breaks never
    /// perturb the deck/assignment stream (keeping baseline games reproducible
    /// even when an LLM game on the same seed hits a forced default).
    pub fn fork(&self, salt: u64) -> Rng {
        Rng::new(self.state ^ salt.wrapping_mul(0xD1B5_4A32_D192_ED03))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seed_diverges() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut rng = Rng::new(7);
        let mut v: Vec<usize> = (0..17).collect();
        rng.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..17).collect::<Vec<_>>());
    }

    #[test]
    fn shuffle_is_deterministic() {
        let mut a = Rng::new(99);
        let mut b = Rng::new(99);
        let mut va: Vec<u32> = (0..50).collect();
        let mut vb = va.clone();
        a.shuffle(&mut va);
        b.shuffle(&mut vb);
        assert_eq!(va, vb);
    }

    #[test]
    fn below_is_in_range() {
        let mut rng = Rng::new(3);
        for _ in 0..1000 {
            assert!(rng.below(7) < 7);
        }
    }
}
