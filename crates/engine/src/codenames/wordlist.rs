//! The word pool a board is drawn from: an original curation of common English
//! nouns vendored in the crate (PRD-codenames-evals US-CN05 — no proprietary
//! card list). Parsing is strict so a malformed pool fails loudly at game
//! setup rather than producing weird boards.

use super::types::CARD_COUNT;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The vendored pool, replaced wholesale by curation passes; only its shape is
/// relied on here (one lowercase word per line, at least 25 of them).
const EMBEDDED: &str = include_str!("../../assets/codenames_words.txt");

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum WordlistError {
    #[error("wordlist line {line}: {word:?} must be lowercase letters (hyphens allowed inside)")]
    BadWord { line: usize, word: String },
    #[error("wordlist line {line}: {word:?} is a duplicate")]
    Duplicate { line: usize, word: String },
    #[error("wordlist has {found} words; a board needs at least {CARD_COUNT}")]
    TooShort { found: usize },
}

/// A validated word pool. Order is preserved from the source file; the board
/// draw shuffles a copy, so the file's ordering never biases a game.
///
/// Every construction path runs the same validation: [`Wordlist::parse`] for
/// text, [`Wordlist::from_words`] for an in-memory list, and the hand-written
/// [`Deserialize`] below, which routes through `from_words` rather than filling
/// the field directly. A derived `Deserialize` would let a malformed or
/// undersized pool back in through a stored `GameState` and produce a board
/// too small to deal — the forced mandatory guess then panics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Wordlist {
    words: Vec<String>,
}

impl<'de> Deserialize<'de> for Wordlist {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// The field layout only — validation lives in `from_words`.
        #[derive(Deserialize)]
        struct Raw {
            words: Vec<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Wordlist::from_words(raw.words).map_err(serde::de::Error::custom)
    }
}

impl Wordlist {
    /// Parse one word per line. Blank lines and `#` comment lines are ignored;
    /// every remaining line must be a unique lowercase token.
    pub fn parse(text: &str) -> Result<Self, WordlistError> {
        let mut words: Vec<String> = Vec::new();
        let mut lines: Vec<usize> = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let word = raw.trim();
            if word.is_empty() || word.starts_with('#') {
                continue;
            }
            words.push(word.to_string());
            lines.push(i + 1);
        }
        Self::validate(&words, |i| lines[i])?;
        Ok(Self { words })
    }

    /// The same invariants as [`Self::parse`], for a list that did not come
    /// from a file (deserialization, callers building a pool in memory). Error
    /// positions are 1-based indices into `words`.
    pub fn from_words(words: Vec<String>) -> Result<Self, WordlistError> {
        Self::validate(&words, |i| i + 1)?;
        Ok(Self { words })
    }

    /// Every word unique, well-formed, and enough of them to deal a board.
    /// `locate` maps a word's index to the position an error should report.
    fn validate(words: &[String], locate: impl Fn(usize) -> usize) -> Result<(), WordlistError> {
        for (i, word) in words.iter().enumerate() {
            if !is_valid_word(word) {
                return Err(WordlistError::BadWord {
                    line: locate(i),
                    word: word.clone(),
                });
            }
            if words[..i].contains(word) {
                return Err(WordlistError::Duplicate {
                    line: locate(i),
                    word: word.clone(),
                });
            }
        }
        if words.len() < CARD_COUNT {
            return Err(WordlistError::TooShort { found: words.len() });
        }
        Ok(())
    }

    /// The vendored pool. Panics on a malformed asset — a broken checked-in
    /// wordlist is a build-time bug, not a runtime condition.
    pub fn default_embedded() -> Self {
        Self::parse(EMBEDDED).expect("vendored codenames wordlist is valid")
    }

    pub fn words(&self) -> &[String] {
        &self.words
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// SHA-256 (hex) of the canonical content — the parsed words joined by
    /// newlines — so comments and whitespace churn never change the hash a
    /// `GameRecord` carries.
    pub fn content_hash(&self) -> String {
        let mut h = Sha256::new();
        for w in &self.words {
            h.update(w.as_bytes());
            h.update(b"\n");
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// A pool word: lowercase ASCII letters, with hyphens allowed only inside.
fn is_valid_word(word: &str) -> bool {
    let bytes = word.as_bytes();
    let ends_alpha = matches!(bytes.first(), Some(c) if c.is_ascii_lowercase())
        && matches!(bytes.last(), Some(c) if c.is_ascii_lowercase());
    ends_alpha && bytes.iter().all(|c| c.is_ascii_lowercase() || *c == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_wordlist_parses_and_is_big_enough() {
        let list = Wordlist::default_embedded();
        assert!(list.len() >= CARD_COUNT);
        assert_eq!(list.content_hash().len(), 64);
    }

    #[test]
    fn parsing_is_strict_but_tolerates_comments() {
        let mut src = String::from("# curated pool\n\n");
        for i in 0..CARD_COUNT {
            src.push_str(&format!("word{}\n", "a".repeat(i + 1)));
        }
        let list = Wordlist::parse(&src).expect("valid");
        assert_eq!(list.len(), CARD_COUNT);

        let bad = src.replace("worda\n", "Worda\n");
        assert!(matches!(
            Wordlist::parse(&bad),
            Err(WordlistError::BadWord { .. })
        ));
        assert!(matches!(
            Wordlist::parse("alpha\nbravo\n"),
            Err(WordlistError::TooShort { found: 2 })
        ));
        assert!(matches!(
            Wordlist::parse(&format!("{src}worda\n")),
            Err(WordlistError::Duplicate { .. })
        ));
    }

    /// Deserialization is a construction path like any other: it must enforce
    /// the same invariants, or a stored state could deal a board from an
    /// undersized or malformed pool.
    #[test]
    fn deserializing_enforces_the_same_invariants_as_parsing() {
        let valid = Wordlist::default_embedded();
        let json = serde_json::to_string(&valid).expect("serializes");
        let back: Wordlist = serde_json::from_str(&json).expect("valid pool round-trips");
        assert_eq!(back.content_hash(), valid.content_hash());

        let words: Vec<String> = valid.words().to_vec();
        for (bad, expected) in [
            (words[..CARD_COUNT - 1].to_vec(), "a board needs at least"),
            (
                {
                    let mut w = words[..CARD_COUNT].to_vec();
                    w[0] = "Uppercase".into();
                    w
                },
                "must be lowercase",
            ),
            (
                {
                    let mut w = words[..CARD_COUNT].to_vec();
                    w[1] = w[0].clone();
                    w
                },
                "is a duplicate",
            ),
        ] {
            let json = serde_json::json!({ "words": bad }).to_string();
            let err = serde_json::from_str::<Wordlist>(&json)
                .expect_err("malformed pool must not deserialize")
                .to_string();
            assert!(err.contains(expected), "{err}");
        }
    }

    #[test]
    fn content_hash_ignores_comments_and_blank_lines() {
        let base = Wordlist::default_embedded();
        let decorated = format!("# header\n\n{EMBEDDED}\n\n");
        assert_eq!(
            base.content_hash(),
            Wordlist::parse(&decorated).expect("valid").content_hash()
        );
    }
}
