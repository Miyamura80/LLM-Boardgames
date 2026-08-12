//! The 5×5 word grid and its key card: a seeded draw of 25 words from the
//! wordlist plus a seeded 9/8/7/1 identity layout. Deterministic given the
//! game RNG, so controlled schedules mirror grids across candidates.

use super::types::*;
use super::wordlist::Wordlist;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One board card. `identity` is engine state, never an event — observations
/// expose it only once `revealed` (or to a spymaster, via [`KeyCard`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Card {
    pub word: String,
    pub identity: CardIdentity,
    pub revealed: bool,
}

/// The full hidden layout, row-major. Only spymaster observations carry one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KeyCard {
    pub identities: Vec<CardIdentity>,
}

/// The laid board: 25 cards in row-major order (`index = row * 5 + col`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Board {
    pub cards: Vec<Card>,
}

/// The key-card pool: 9 starting-team agents, 8 for the second team,
/// 7 bystanders, 1 assassin.
fn identity_pool() -> Vec<CardIdentity> {
    let mut pool = Vec::with_capacity(CARD_COUNT);
    pool.extend(std::iter::repeat_n(
        CardIdentity::Agent { team: Team::A },
        STARTING_AGENTS as usize,
    ));
    pool.extend(std::iter::repeat_n(
        CardIdentity::Agent { team: Team::B },
        SECOND_AGENTS as usize,
    ));
    pool.extend(std::iter::repeat_n(
        CardIdentity::Bystander,
        BYSTANDERS as usize,
    ));
    pool.extend(std::iter::repeat_n(
        CardIdentity::Assassin,
        ASSASSINS as usize,
    ));
    pool
}

impl Board {
    /// Draw 25 words without replacement and deal the key over them. Both
    /// shuffles come from the game RNG in a fixed order, so a seed pins the
    /// grid and the key together.
    pub fn generate(rng: &mut ChaCha8Rng, wordlist: &Wordlist) -> Self {
        let mut words: Vec<String> = wordlist.words().to_vec();
        words.shuffle(rng);
        words.truncate(CARD_COUNT);

        let mut identities = identity_pool();
        identities.shuffle(rng);

        let cards = words
            .into_iter()
            .zip(identities)
            .map(|(word, identity)| Card {
                word,
                identity,
                revealed: false,
            })
            .collect();
        Self { cards }
    }

    /// Index of the card carrying `word` (already normalized to lowercase),
    /// revealed or not.
    pub fn find_word(&self, word: &str) -> Option<usize> {
        self.cards.iter().position(|c| c.word == word)
    }

    /// Every board word in grid order (public information from turn one).
    pub fn words(&self) -> Vec<String> {
        self.cards.iter().map(|c| c.word.clone()).collect()
    }

    /// The hidden layout, for spymaster observations only.
    pub fn key_card(&self) -> KeyCard {
        KeyCard {
            identities: self.cards.iter().map(|c| c.identity).collect(),
        }
    }

    pub fn unrevealed_words(&self) -> Vec<String> {
        self.cards
            .iter()
            .filter(|c| !c.revealed)
            .map(|c| c.word.clone())
            .collect()
    }

    /// Agents of `team` still face-down — the clue-number ceiling and the
    /// win condition both read this.
    pub fn remaining_agents(&self, team: Team) -> u8 {
        self.cards
            .iter()
            .filter(|c| !c.revealed && c.identity == CardIdentity::Agent { team })
            .count() as u8
    }

    pub fn revealed_agents(&self, team: Team) -> u8 {
        team.agent_count() - self.remaining_agents(team)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::testkit;
    use rand::SeedableRng;

    #[test]
    fn boards_are_seed_stable_and_correctly_composed() {
        let wordlist = testkit::test_wordlist();
        for seed in 0..16u64 {
            let a = Board::generate(&mut ChaCha8Rng::seed_from_u64(seed), &wordlist);
            let b = Board::generate(&mut ChaCha8Rng::seed_from_u64(seed), &wordlist);
            assert_eq!(a, b, "same seed, same grid and key");

            assert_eq!(a.cards.len(), CARD_COUNT);
            let mut words = a.words();
            words.sort();
            words.dedup();
            assert_eq!(words.len(), CARD_COUNT, "words drawn without replacement");

            assert_eq!(a.remaining_agents(Team::A), STARTING_AGENTS);
            assert_eq!(a.remaining_agents(Team::B), SECOND_AGENTS);
            let count = |want: CardIdentity| a.cards.iter().filter(|c| c.identity == want).count();
            assert_eq!(count(CardIdentity::Bystander), BYSTANDERS as usize);
            assert_eq!(count(CardIdentity::Assassin), ASSASSINS as usize);
        }
    }

    #[test]
    fn different_seeds_give_different_boards() {
        let wordlist = testkit::test_wordlist();
        let a = Board::generate(&mut ChaCha8Rng::seed_from_u64(1), &wordlist);
        let b = Board::generate(&mut ChaCha8Rng::seed_from_u64(2), &wordlist);
        assert_ne!(a, b);
    }
}
