//! The engine-authoritative game state: the grid, the key card, the clue log,
//! and whose turn it is. One seeded RNG feeds the board draw, the key layout,
//! and every forced default, so a seed plus an action sequence reproduces a
//! transcript exactly.

use super::board::Board;
use super::events::{CodenamesEvent, EventRecord};
use super::types::*;
use super::wordlist::Wordlist;
use crate::game_core::Visibility;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where the turn is. Decision points derive from this (see `actions.rs`).
///
/// The game needs no turn-count backstop: every turn reveals at least one card
/// (the mandatory first guess), so play terminates within 25 turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Phase {
    /// The active team's spymaster owes a clue.
    Clue,
    /// The active team's operative is guessing under the live clue.
    Guess {
        clue: Clue,
        guesses_used: u8,
    },
    GameOver,
}

impl Phase {
    /// Short kebab name for logs, errors, and observations.
    pub fn name(&self) -> &'static str {
        match self {
            Phase::Clue => "clue",
            Phase::Guess { .. } => "guess",
            Phase::GameOver => "game-over",
        }
    }
}

/// One reveal under a clue, in guess order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GuessOutcome {
    pub word: String,
    pub identity: CardIdentity,
}

/// A clue and everything that happened under it — the spine of the clue
/// history every observation and metric reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClueRecord {
    pub turn: u32,
    pub team: Team,
    pub clue: Clue,
    pub outcomes: Vec<GuessOutcome>,
    /// The operative ended the turn voluntarily rather than by a bad reveal.
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub seed: u64,
    /// The single game RNG: board draw, key layout, forced-default picks.
    pub(super) rng: ChaCha8Rng,
    pub config: GameConfig,
    pub board: Board,
    /// Kept for forced-default clue draws (a legal clue must come from
    /// somewhere deterministic).
    pub wordlist: Wordlist,
    pub wordlist_hash: String,
    /// Turn counter (0 during setup, then 1, 2, … — one per team turn).
    pub turn: u32,
    pub active: Team,
    pub phase: Phase,
    pub clues: Vec<ClueRecord>,
    pub winner: Option<Team>,
    pub end_reason: Option<EndReason>,
    pub events: Vec<EventRecord>,
}

impl GameState {
    /// A game on the vendored wordlist with default rules knobs.
    pub fn new(seed: u64) -> Self {
        Self::with_wordlist(seed, Wordlist::default_embedded(), GameConfig::default())
    }

    /// A game on an explicit pool (config override or scripted tests).
    pub fn with_wordlist(seed: u64, wordlist: Wordlist, config: GameConfig) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let board = Board::generate(&mut rng, &wordlist);
        let wordlist_hash = wordlist.content_hash();

        let mut state = Self {
            seed,
            rng,
            config,
            board,
            wordlist,
            wordlist_hash,
            turn: 0,
            active: Team::A,
            phase: Phase::Clue,
            clues: Vec::new(),
            winner: None,
            end_reason: None,
            events: Vec::new(),
        };

        state.push_event(
            Visibility::Public,
            CodenamesEvent::GameStarted {
                starting_team: Team::A,
                wordlist_hash: state.wordlist_hash.clone(),
                rules_version: RULES_VERSION.to_string(),
            },
        );
        let words = state.board.words();
        state.push_event(Visibility::Public, CodenamesEvent::BoardLaid { words });
        super::transitions::begin_turn(&mut state, Team::A);
        state
    }

    pub fn push_event(&mut self, visibility: Visibility, event: CodenamesEvent) {
        self.events.push(EventRecord {
            idx: self.events.len() as u32,
            round: self.turn,
            visibility,
            event,
        });
    }

    pub fn push_forced_default(&mut self, seat: Seat, decision: &str) {
        self.push_event(
            Visibility::Public,
            CodenamesEvent::ForcedDefault {
                seat,
                decision: decision.to_string(),
            },
        );
    }

    pub fn is_over(&self) -> bool {
        self.winner.is_some()
    }

    /// Agents of `team` still face-down.
    pub fn remaining_agents(&self, team: Team) -> u8 {
        self.board.remaining_agents(team)
    }

    /// The live clue, while one is being guessed under.
    pub fn current_clue(&self) -> Option<&Clue> {
        match &self.phase {
            Phase::Guess { clue, .. } => Some(clue),
            _ => None,
        }
    }

    /// Settle the game. The winner is passed in explicitly because a team can
    /// win on the *opponent's* reveal (their last agent, or the assassin).
    pub(super) fn finish(&mut self, winner: Team, reason: EndReason) {
        self.winner = Some(winner);
        self.end_reason = Some(reason);
        self.phase = Phase::GameOver;
        let turns = self.turn;
        self.push_event(
            Visibility::Public,
            CodenamesEvent::GameEnded {
                winner,
                reason,
                turns,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::actions::Action;
    use crate::codenames::testkit;

    /// A fixed seed plus an identical action sequence must reproduce the
    /// transcript byte for byte, forced defaults included.
    #[test]
    fn seeded_games_replay_identically() {
        let transcript = |seed: u64| {
            let mut state = testkit::scripted_game(seed);
            while !state.is_over() {
                let decision = state.pending_decisions().remove(0);
                let action: Action = state.forced_default(&decision);
                state.push_forced_default(decision.seat(), decision.kind());
                state
                    .apply(decision.seat(), action)
                    .expect("forced defaults are always legal");
            }
            serde_json::to_string(&state.events).expect("events serialize")
        };
        assert_eq!(transcript(7), transcript(7));
        assert_ne!(transcript(7), transcript(8));
    }

    #[test]
    fn setup_opens_with_team_a_owing_a_clue() {
        let state = testkit::scripted_game(3);
        assert_eq!(state.turn, 1);
        assert_eq!(state.active, Team::A);
        assert_eq!(state.phase, Phase::Clue);
        assert_eq!(state.remaining_agents(Team::A), STARTING_AGENTS);
        assert_eq!(state.remaining_agents(Team::B), SECOND_AGENTS);
        assert_eq!(state.wordlist_hash.len(), 64);
    }
}
