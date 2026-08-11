//! Uniform-random legal play: the zero-token CI floor. Clues are drawn from
//! the vendored pool and carry no meaning — this bot is a legality and
//! determinism baseline, never a rating anchor (PRD-codenames-evals US-CN06:
//! "random clue-giving is not a baseline, it is noise").
//!
//! Every action is legal *by construction*: clue candidates are filtered
//! through the same rule `apply()` enforces, and guesses name a face-down card
//! from the observation. Property tests assert the engine never rejects one.

use super::{AgentError, AgentReply, SeatAgent};
use crate::codenames::actions::{Action, DecisionPoint};
use crate::codenames::observation::Observation;
use crate::codenames::types::GameConfig;
use crate::codenames::wordlist::Wordlist;
use async_trait::async_trait;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Seeded from a `u64` like every other bot in the harness, so a whole game of
/// them replays bit-for-bit.
pub struct RandomLegalBot {
    rng: ChaCha8Rng,
    /// Clue candidates. Parsed once: the pool never changes mid-game.
    wordlist: Wordlist,
    /// The engine's cap is config-driven and absent from observations, so the
    /// bot assumes the default. A game configured *below* the default would
    /// only narrow this bot's candidate set, never make its clue illegal —
    /// candidates are re-filtered against the cap it knows.
    clue_word_max_len: usize,
}

impl RandomLegalBot {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            wordlist: Wordlist::default_embedded(),
            clue_word_max_len: GameConfig::default().clue_word_max_len,
        }
    }

    /// A seeded-random pool word that is legal on the current board, with the
    /// engine's synthetic fallback for the (practically unreachable) case
    /// where every pool word collides with a face-down card.
    fn clue_word(&mut self, obs: &Observation) -> String {
        let max_len = self.clue_word_max_len;
        let candidates: Vec<&String> = self
            .wordlist
            .words()
            .iter()
            .filter(|w| clue_word_is_legal(obs, w, max_len))
            .collect();
        if !candidates.is_empty() {
            let pick = self.rng.gen_range(0..candidates.len());
            return candidates[pick].clone();
        }
        let longest = max_len.max(2);
        (2..=longest)
            .map(|n| "z".repeat(n))
            .find(|w| clue_word_is_legal(obs, w, max_len))
            .unwrap_or_else(|| "z".repeat(longest))
    }

    /// A seeded-random face-down word. A live game always has one: revealing
    /// a team's last agent ends it well before the grid empties.
    fn face_down_word(&mut self, obs: &Observation) -> String {
        let words: Vec<&String> = obs
            .grid
            .iter()
            .filter(|c| !c.revealed)
            .map(|c| &c.word)
            .collect();
        assert!(
            !words.is_empty(),
            "a live game always leaves a face-down card"
        );
        let pick = self.rng.gen_range(0..words.len());
        words[pick].clone()
    }
}

/// The clue rule as an agent can check it from its own observation: a single
/// alphabetic token (hyphens inside) within the length cap that neither
/// contains nor sits inside any **unrevealed** board word. Mirrors
/// `transitions::clue`, which stays the sole authority — this is the filter
/// that keeps the bot from ever handing it an illegal clue.
fn clue_word_is_legal(obs: &Observation, word: &str, max_len: usize) -> bool {
    let shape_ok = word.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
        && word.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && word.chars().last().is_some_and(|c| c.is_ascii_alphabetic())
        && word.chars().count() <= max_len;
    if !shape_ok {
        return false;
    }
    let clue = word.to_ascii_lowercase();
    !obs.grid
        .iter()
        .any(|c| !c.revealed && (c.word.contains(&clue) || clue.contains(c.word.as_str())))
}

#[async_trait]
impl SeatAgent for RandomLegalBot {
    fn kind(&self) -> &'static str {
        "bot:codenames-random"
    }
    fn model_id(&self) -> String {
        "bot:codenames-random".into()
    }
    fn scaffold_version(&self) -> String {
        "bot-random-v1".into()
    }

    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        _feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        let action = match decision {
            // Number 1 is always in range: a team with no unrevealed agents
            // left has already won, so the game would be over.
            DecisionPoint::GiveClue { .. } => Action::GiveClue {
                word: self.clue_word(obs),
                number: 1,
            },
            // The first guess of a clue is mandatory; anything after it is a
            // free roll this bot declines.
            DecisionPoint::GuessOrPass {
                guesses_used: 0, ..
            } => Action::Guess {
                word: self.face_down_word(obs),
            },
            DecisionPoint::GuessOrPass { .. } => Action::Pass,
        };
        Ok(AgentReply {
            action,
            thought: None,
        })
    }
}
