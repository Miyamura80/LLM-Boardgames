//! Schedule cell seeds. Stable across Rust versions (std's DefaultHasher is
//! not): the mirrored-seed and resume contracts both depend on this function
//! never changing output.

use sha2::{Digest, Sha256};

/// Candidate-independent seed for one schedule cell: a pure function of the
/// match seed, a schedule tag, and three cell coordinates — so different
/// candidates run through a cell see identical decks, boards, and dice.
pub fn cell_seed(match_seed: u64, tag: &str, a: u64, b: u64, c: u64) -> u64 {
    let mut h = Sha256::new();
    h.update(match_seed.to_le_bytes());
    h.update(tag.as_bytes());
    h.update(a.to_le_bytes());
    h.update(b.to_le_bytes());
    h.update(c.to_le_bytes());
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().expect("sha256 yields 32 bytes"))
}
