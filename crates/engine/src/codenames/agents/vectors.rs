//! A word → vector lookup table in the standard GloVe / word2vec **text**
//! format, plus the cosine similarity the embedding anchor scores with.
//!
//! Deliberately source-agnostic: the table is just `word v1 v2 … vN` lines, so
//! the same code reads the hand-crafted test fixture
//! (`crates/engine/fixtures/codenames_test_vectors.txt`) and, later, a real
//! pre-trained subset. No vendored asset ships in this phase
//! (PRD-codenames-evals US-CN06: "checked in or fetched at build time").
//!
//! Parsing is strict for the same reason [`Wordlist`](super::super::wordlist)
//! parsing is: a truncated or ragged vector file must fail loudly at load time,
//! never silently produce a table where half the board has no vector and the
//! anchor quietly degrades to lexicographic guessing.

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum VectorTableError {
    #[error("vector file line {line}: expected `word v1 v2 …`, got {text:?}")]
    Malformed { line: usize, text: String },
    #[error(
        "vector file line {line}: {word:?} has {found} component(s); \
         the table is {expected}-dimensional"
    )]
    DimMismatch {
        line: usize,
        word: String,
        found: usize,
        expected: usize,
    },
    #[error("vector file line {line}: {word:?} component {index} is not a number: {value:?}")]
    BadComponent {
        line: usize,
        word: String,
        index: usize,
        value: String,
    },
    #[error("vector file line {line}: {word:?} is a duplicate (lookups are case-insensitive)")]
    Duplicate { line: usize, word: String },
    #[error("vector file contains no vectors")]
    Empty,
    #[error("reading vector file {path}: {reason}")]
    Io { path: String, reason: String },
}

/// An immutable word-vector table. Iteration order is lexicographic
/// (`BTreeMap`), which is what makes the anchor's candidate sweep — and so its
/// clues — reproducible run to run.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorTable {
    dim: usize,
    vectors: BTreeMap<String, Vec<f32>>,
}

impl VectorTable {
    /// Parse the text format: one `word v1 v2 … vN` line, whitespace
    /// separated. Blank lines and `#` comments are ignored, words are
    /// lowercased, and every vector must carry the same number of components.
    ///
    /// A leading word2vec `<count> <dim>` header line (two integers, `dim` ≥ 2)
    /// is recognized and skipped, so both common text dialects load.
    pub fn parse(text: &str) -> Result<Self, VectorTableError> {
        let mut dim: Option<usize> = None;
        let mut vectors: BTreeMap<String, Vec<f32>> = BTreeMap::new();

        for (i, raw) in text.lines().enumerate() {
            let line = i + 1;
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let mut tokens = trimmed.split_whitespace();
            let word = tokens
                .next()
                .expect("a non-empty line has a first token")
                .to_ascii_lowercase();
            let components: Vec<&str> = tokens.collect();

            if vectors.is_empty() && dim.is_none() && is_word2vec_header(&word, &components) {
                continue;
            }
            if components.is_empty() {
                return Err(VectorTableError::Malformed {
                    line,
                    text: trimmed.to_string(),
                });
            }
            let expected = *dim.get_or_insert(components.len());
            if components.len() != expected {
                return Err(VectorTableError::DimMismatch {
                    line,
                    word,
                    found: components.len(),
                    expected,
                });
            }

            let mut vector = Vec::with_capacity(expected);
            for (index, token) in components.iter().enumerate() {
                match token.parse::<f32>() {
                    Ok(v) if v.is_finite() => vector.push(v),
                    _ => {
                        return Err(VectorTableError::BadComponent {
                            line,
                            word,
                            index,
                            value: (*token).to_string(),
                        })
                    }
                }
            }
            if vectors.insert(word.clone(), vector).is_some() {
                return Err(VectorTableError::Duplicate { line, word });
            }
        }

        match dim {
            Some(dim) if !vectors.is_empty() => Ok(Self { dim, vectors }),
            _ => Err(VectorTableError::Empty),
        }
    }

    /// Load a table from disk. The match layer calls this once per run and
    /// shares the result across seats behind an `Arc`.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, VectorTableError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| VectorTableError::Io {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        Self::parse(&text)
    }

    /// The vector for `word`, looked up case-insensitively.
    pub fn get(&self, word: &str) -> Option<&[f32]> {
        if let Some(v) = self.vectors.get(word) {
            return Some(v.as_slice());
        }
        self.vectors
            .get(&word.to_ascii_lowercase())
            .map(Vec::as_slice)
    }

    pub fn contains(&self, word: &str) -> bool {
        self.get(word).is_some()
    }

    /// Cosine similarity between two table words, or `None` if either is
    /// missing — an absent word is never silently scored as "unrelated".
    pub fn similarity(&self, a: &str, b: &str) -> Option<f32> {
        Some(cosine(self.get(a)?, self.get(b)?))
    }

    /// Every word in the table, lexicographically — the anchor's candidate
    /// sweep order.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.vectors.keys().map(String::as_str)
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
}

impl std::str::FromStr for VectorTable {
    type Err = VectorTableError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

/// Cosine similarity, by hand over slices (no linear-algebra dependency).
/// Returns `0.0` — "unrelated", never a `NaN` that would poison a max — for
/// mismatched lengths or a zero-norm vector.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= 0.0 {
        return 0.0;
    }
    (dot / denom).clamp(-1.0, 1.0)
}

/// A word2vec text header is `<vocab_count> <dim>`: two integers and nothing
/// else. The `dim >= 2` guard keeps a genuine 1-dimensional vector whose word
/// happens to be a number from being eaten.
fn is_word2vec_header(word: &str, components: &[&str]) -> bool {
    components.len() == 1
        && word.parse::<usize>().is_ok()
        && components[0].parse::<usize>().is_ok_and(|dim| dim >= 2)
}
