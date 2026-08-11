//! Per-seat observations, where the game's one asymmetry lives: spymasters
//! receive the key card, operatives structurally cannot — `key` is simply
//! absent from their view. The key is derived here from state (SH's
//! `known_teammates` precedent); it is never an event, so `game_core`'s
//! `Visibility` needs no team variant.

use super::board::KeyCard;
use super::events::EventRecord;
use super::state::{GameState, GuessOutcome, Phase};
use super::types::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One grid cell as a seat sees it: the word always, the identity only once
/// the card has been revealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CardView {
    pub index: u8,
    pub row: u8,
    pub col: u8,
    pub word: String,
    pub revealed: bool,
    /// `Some` only for revealed cards — public information by then.
    pub identity: Option<CardIdentity>,
}

/// A clue and what it produced. `guesses_remaining` is `Some` only for the
/// live clue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClueView {
    pub turn: u32,
    pub team: Team,
    pub word: String,
    pub number: u8,
    pub outcomes: Vec<GuessOutcome>,
    pub passed: bool,
    pub guesses_remaining: Option<u8>,
}

/// A team's progress, visible to everyone (revealed cards are public).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamScore {
    pub team: Team,
    pub agents_total: u8,
    pub agents_revealed: u8,
    pub agents_remaining: u8,
}

/// Everything one seat is entitled to know.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Observation {
    pub seat: Seat,
    pub role: Role,
    pub team: Team,
    /// The 5×5 grid, row-major.
    pub grid: Vec<CardView>,
    /// Clue history in order, oldest first.
    pub clues: Vec<ClueView>,
    /// Both teams, in `TEAMS` order.
    pub scores: Vec<TeamScore>,
    pub turn: u32,
    pub active_team: Team,
    pub phase: &'static str,
    pub winner: Option<Team>,
    pub end_reason: Option<EndReason>,
    /// The full key card — `Some` for spymaster seats only.
    pub key: Option<KeyCard>,
    /// Everything this seat legitimately witnessed, in order (in Codenames,
    /// every event is public).
    pub history: Vec<EventRecord>,
}

impl GameState {
    /// Build the role-conditioned view for `seat`.
    pub fn observe(&self, seat: Seat) -> Observation {
        let role = seat_role(seat);
        let live_clue_index = match self.phase {
            Phase::Guess { .. } => self.clues.len().checked_sub(1),
            _ => None,
        };

        let grid = self
            .board
            .cards
            .iter()
            .enumerate()
            .map(|(i, card)| CardView {
                index: i as u8,
                row: (i / GRID_SIDE) as u8,
                col: (i % GRID_SIDE) as u8,
                word: card.word.clone(),
                revealed: card.revealed,
                identity: card.revealed.then_some(card.identity),
            })
            .collect();

        let clues = self
            .clues
            .iter()
            .enumerate()
            .map(|(i, record)| ClueView {
                turn: record.turn,
                team: record.team,
                word: record.clue.word.clone(),
                number: record.clue.number,
                outcomes: record.outcomes.clone(),
                passed: record.passed,
                guesses_remaining: (Some(i) == live_clue_index).then(|| {
                    record
                        .clue
                        .guess_cap()
                        .saturating_sub(record.outcomes.len() as u8)
                }),
            })
            .collect();

        Observation {
            seat,
            role,
            team: seat_team(seat),
            grid,
            clues,
            scores: TEAMS
                .into_iter()
                .map(|team| TeamScore {
                    team,
                    agents_total: team.agent_count(),
                    agents_revealed: self.board.revealed_agents(team),
                    agents_remaining: self.board.remaining_agents(team),
                })
                .collect(),
            turn: self.turn,
            active_team: self.active,
            phase: self.phase.name(),
            winner: self.winner,
            end_reason: self.end_reason,
            key: (role == Role::Spymaster).then(|| self.board.key_card()),
            history: self
                .events
                .iter()
                .filter(|e| e.visibility.visible_to(seat))
                .cloned()
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::testkit;

    /// The key-leak guard: nothing an operative can see names an unrevealed
    /// card's identity — not the grid, not the clue log, not the transcript.
    #[test]
    fn operative_observations_carry_no_key_information() {
        let mut state = testkit::scripted_game(31);
        testkit::give_clue(&mut state, 2);
        let own = testkit::word_with(&state, CardIdentity::Agent { team: Team::A });
        state
            .apply(
                SEAT_A_OPERATIVE,
                crate::codenames::actions::Action::Guess { word: own.clone() },
            )
            .expect("own agent");

        for seat in [SEAT_A_OPERATIVE, SEAT_B_OPERATIVE] {
            let view = state.observe(seat);
            assert!(view.key.is_none(), "operatives never receive a key card");
            assert_eq!(view.role, Role::Operative);
            for card in &view.grid {
                assert_eq!(
                    card.identity.is_some(),
                    card.revealed,
                    "identity leaked for face-down card {:?}",
                    card.word
                );
            }
            for clue in &view.clues {
                assert_eq!(clue.outcomes.len(), 1, "only the resolved guess is logged");
            }
            // Belt and braces: with the assassin still face-down, the word
            // must not appear anywhere in the serialized view.
            let json = serde_json::to_string(&view).expect("observation serializes");
            assert!(!json.contains("assassin"), "assassin identity leaked");
            assert_eq!(view.scores[Team::A.index()].agents_revealed, 1);
        }
    }

    #[test]
    fn spymasters_see_the_whole_key_and_agree_with_each_other() {
        let state = testkit::scripted_game(31);
        let a = state.observe(SEAT_A_SPYMASTER);
        let b = state.observe(SEAT_B_SPYMASTER);
        let key = a.key.clone().expect("spymasters receive the key card");
        assert_eq!(key.identities.len(), CARD_COUNT);
        assert_eq!(a.key, b.key, "both spymasters see the same key card");
        assert_eq!(a.team, Team::A);
        assert_eq!(b.team, Team::B);
        assert_eq!(a.grid, b.grid, "the grid itself is symmetric");
    }

    #[test]
    fn the_live_clue_reports_its_remaining_guesses() {
        let mut state = testkit::scripted_game(31);
        testkit::give_clue(&mut state, 2);
        let view = state.observe(SEAT_A_OPERATIVE);
        assert_eq!(view.clues[0].guesses_remaining, Some(3));

        let own = testkit::word_with(&state, CardIdentity::Agent { team: Team::A });
        state
            .apply(
                SEAT_A_OPERATIVE,
                crate::codenames::actions::Action::Guess { word: own },
            )
            .expect("own agent");
        let view = state.observe(SEAT_A_OPERATIVE);
        assert_eq!(view.clues[0].guesses_remaining, Some(2));
        assert_eq!(view.clues[0].outcomes.len(), 1);
        assert_eq!(view.phase, "guess");
    }
}
