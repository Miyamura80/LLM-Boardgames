//! Agent-facing decision points and actions (atomic: one clue, or one single
//! guess — never a batched guess list), plus the deterministic forced legal
//! defaults applied when an agent exhausts its retry budget.

use super::state::{GameState, Phase};
use super::transitions::clue::clue_word_is_legal;
use super::types::{Clue, Seat, Team};
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::game_core::IllegalMove;

/// A decision the engine is waiting on. Codenames is fully sequential, so
/// `pending_decisions` never returns more than one of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum DecisionPoint {
    /// The active team's spymaster owes a clue.
    GiveClue {
        seat: Seat,
        team: Team,
        /// Largest legal clue number right now (= own unrevealed agents).
        max_number: u8,
    },
    /// The active team's operative owes one guess — or a pass, once the
    /// mandatory first guess of the clue has been made.
    GuessOrPass {
        seat: Seat,
        team: Team,
        clue: Clue,
        guesses_used: u8,
        /// Guesses still allowed under this clue (number + 1 total).
        guesses_remaining: u8,
    },
}

impl DecisionPoint {
    /// The seat that must act.
    pub fn seat(&self) -> Seat {
        match self {
            DecisionPoint::GiveClue { seat, .. } | DecisionPoint::GuessOrPass { seat, .. } => *seat,
        }
    }

    /// Short kebab name for logs and reliability counters.
    pub fn kind(&self) -> &'static str {
        match self {
            DecisionPoint::GiveClue { .. } => "give-clue",
            DecisionPoint::GuessOrPass { .. } => "guess-or-pass",
        }
    }
}

impl crate::game_core::DecisionOps for DecisionPoint {
    fn seat(&self) -> Seat {
        DecisionPoint::seat(self)
    }
    fn kind(&self) -> &'static str {
        DecisionPoint::kind(self)
    }
}

/// An agent's move at a decision point. Clue words are normalized (trimmed,
/// lowercased) by the engine; guesses name a board word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    GiveClue { word: String, number: u8 },
    Guess { word: String },
    Pass,
}

impl GameState {
    /// Every decision the engine is currently waiting on — always exactly one
    /// until the game is over.
    pub fn pending_decisions(&self) -> Vec<DecisionPoint> {
        match &self.phase {
            Phase::Clue => vec![DecisionPoint::GiveClue {
                seat: self.active.spymaster(),
                team: self.active,
                max_number: self.remaining_agents(self.active),
            }],
            Phase::Guess { clue, guesses_used } => vec![DecisionPoint::GuessOrPass {
                seat: self.active.operative(),
                team: self.active,
                clue: clue.clone(),
                guesses_used: *guesses_used,
                guesses_remaining: clue.guess_cap().saturating_sub(*guesses_used),
            }],
            Phase::GameOver => Vec::new(),
        }
    }

    /// The deterministic forced legal default: clue → a seeded-random
    /// wordlist word that is legal on the current board, number 1; the
    /// mandatory first guess → a seeded-random unrevealed card; any later
    /// guess → pass.
    ///
    /// A forced first guess can hit the assassin. That is accepted and
    /// precedented (PRD §7): honoring the must-guess rule keeps the engine
    /// faithful, and forced actions are logged and metric-exempt.
    pub fn forced_default(&mut self, decision: &DecisionPoint) -> Action {
        match decision {
            DecisionPoint::GiveClue { .. } => Action::GiveClue {
                word: self.forced_clue_word(),
                number: 1,
            },
            DecisionPoint::GuessOrPass {
                guesses_used: 0, ..
            } => {
                let words = self.board.unrevealed_words();
                let pick = self.rng.gen_range(0..words.len());
                Action::Guess {
                    word: words[pick].clone(),
                }
            }
            DecisionPoint::GuessOrPass { .. } => Action::Pass,
        }
    }

    /// A seeded-random pool word that is legal on the current board, with a
    /// deterministic synthetic fallback for the (practically unreachable) case
    /// where the draw leaves the whole pool illegal.
    fn forced_clue_word(&mut self) -> String {
        let max_len = self.config.clue_word_max_len;
        let candidates: Vec<&String> = self
            .wordlist
            .words()
            .iter()
            .filter(|w| clue_word_is_legal(&self.board, w, max_len))
            .collect();
        if !candidates.is_empty() {
            let pick = self.rng.gen_range(0..candidates.len());
            return candidates[pick].clone();
        }
        let longest = max_len.max(2);
        (2..=longest)
            .map(|n| "z".repeat(n))
            .find(|w| clue_word_is_legal(&self.board, w, max_len))
            .unwrap_or_else(|| "z".repeat(longest))
    }
}
