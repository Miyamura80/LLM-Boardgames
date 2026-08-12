//! Clue giving and the deterministic legality check that the whole eval
//! machinery hangs off (PRD-codenames-evals §6): a single alphabetic token,
//! case-insensitive, that does not equal, contain, or sit inside any
//! **unrevealed** board word — revealed words become legal clues, per the
//! official rule.
//!
//! The check is deliberately looser than rules-as-written (no stemmer, no
//! rhyme adjudication — PRD §5). That looseness is symmetric across
//! candidates, and clues are stored verbatim so stricter offline audits stay
//! possible. Tightening it bumps `RULES_VERSION`.

use super::super::actions::IllegalMove;
use super::super::board::Board;
use super::super::events::CodenamesEvent;
use super::super::state::{ClueRecord, GameState, Phase};
use super::super::types::{seat_team, Clue, Seat};
use crate::game_core::Visibility;

/// Validate a clue token and return its normalized (trimmed, lowercased) form.
/// Surrounding whitespace is forgiven; interior whitespace is not.
pub(crate) fn normalize_clue_word(word: &str, max_len: usize) -> Result<String, IllegalMove> {
    let trimmed = word.trim();
    if trimmed.is_empty() {
        return Err(IllegalMove(
            "a clue must be a single word, but none was given".into(),
        ));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(IllegalMove(format!(
            "clue \"{trimmed}\" must be a single word: spaces are not allowed"
        )));
    }
    let interior_ok = trimmed.chars().all(|c| c.is_ascii_alphabetic() || c == '-');
    let ends_ok = trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && trimmed
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphabetic());
    if !interior_ok || !ends_ok {
        return Err(IllegalMove(format!(
            "clue \"{trimmed}\" must be letters only, with hyphens allowed inside the word \
             (no digits, punctuation, or spaces)"
        )));
    }
    let len = trimmed.chars().count();
    if len > max_len {
        return Err(IllegalMove(format!(
            "clue \"{trimmed}\" is {len} characters long; the limit is {max_len}"
        )));
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// The unrevealed board word a normalized clue collides with, if any.
pub(crate) fn overlapping_board_word(board: &Board, clue: &str) -> Option<String> {
    board
        .cards
        .iter()
        .find(|c| !c.revealed && (c.word.contains(clue) || clue.contains(c.word.as_str())))
        .map(|c| c.word.clone())
}

/// Whether a raw word would pass every clue check on this board — used by the
/// forced-default draw so it can only ever produce a legal clue.
pub(crate) fn clue_word_is_legal(board: &Board, word: &str, max_len: usize) -> bool {
    normalize_clue_word(word, max_len).is_ok_and(|w| overlapping_board_word(board, &w).is_none())
}

pub(super) fn give_clue(
    state: &mut GameState,
    seat: Seat,
    word: &str,
    number: u8,
) -> Result<(), IllegalMove> {
    let normalized = normalize_clue_word(word, state.config.clue_word_max_len)?;
    if let Some(board_word) = overlapping_board_word(&state.board, &normalized) {
        return Err(IllegalMove(format!(
            "clue \"{normalized}\" overlaps the unrevealed board word \"{board_word}\": a clue may \
             not equal, contain, or be contained in an unrevealed board word"
        )));
    }

    let team = seat_team(seat);
    let remaining = state.remaining_agents(team);
    if number == 0 {
        return Err(IllegalMove(
            "clue number must be at least 1 (0 and \"unlimited\" clues are out of scope)".into(),
        ));
    }
    if number > remaining {
        return Err(IllegalMove(format!(
            "clue number {number} exceeds the {remaining} agent(s) your team still has unrevealed"
        )));
    }

    // Normalization is lossy (case, surrounding whitespace); the transcript
    // keeps the submitted spelling beside the normalized word so a stricter
    // offline audit can still read what the spymaster actually wrote.
    let raw_word = (word != normalized).then(|| word.to_string());
    let clue = Clue {
        word: normalized,
        number,
    };
    state.push_event(
        Visibility::Public,
        CodenamesEvent::ClueGiven {
            seat,
            team,
            clue: clue.clone(),
            raw_word,
        },
    );
    state.clues.push(ClueRecord {
        turn: state.turn,
        team,
        clue: clue.clone(),
        outcomes: Vec::new(),
        passed: false,
    });
    state.phase = Phase::Guess {
        clue,
        guesses_used: 0,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::actions::Action;
    use crate::codenames::testkit;
    use crate::codenames::types::{CardIdentity, GameConfig, Team, SEAT_A_SPYMASTER};

    /// A clue may not equal, sit inside, or contain an unrevealed board word.
    #[test]
    fn clues_overlapping_unrevealed_board_words_are_rejected() {
        let mut state = testkit::scripted_game(11);
        let board_word = state.board.cards[0].word.clone();
        let prefix: String = board_word.chars().take(3).collect();
        let extended = format!("{board_word}s");

        for bad in [
            board_word.clone(),
            board_word.to_uppercase(),
            prefix,
            extended,
        ] {
            let err = state
                .apply(
                    SEAT_A_SPYMASTER,
                    Action::GiveClue {
                        word: bad.clone(),
                        number: 1,
                    },
                )
                .expect_err("board-word overlap is illegal");
            assert!(
                err.0.contains("overlaps the unrevealed board word"),
                "unexpected message for {bad:?}: {}",
                err.0
            );
        }
    }

    /// Once a word is revealed it stops constraining clues (official rule).
    #[test]
    fn revealed_board_words_become_legal_clues() {
        let mut state = testkit::scripted_game(11);
        let word = testkit::word_with(&state, CardIdentity::Agent { team: Team::A });
        assert!(!clue_word_is_legal(&state.board, &word, 20));

        testkit::reveal(&mut state, &word);
        assert!(
            clue_word_is_legal(&state.board, &word, 20),
            "a revealed word is a legal clue"
        );
    }

    #[test]
    fn clue_tokens_must_be_single_alphabetic_words_within_the_length_cap() {
        let mut state = testkit::scripted_game(4);
        let cases = [
            ("two words", "single word"),
            ("agent7", "letters only"),
            ("-lead", "letters only"),
            ("", "single word"),
        ];
        for (word, expected) in cases {
            let err = state
                .apply(
                    SEAT_A_SPYMASTER,
                    Action::GiveClue {
                        word: word.to_string(),
                        number: 1,
                    },
                )
                .expect_err("malformed token");
            assert!(err.0.contains(expected), "{word:?} gave {}", err.0);
        }
        // Hyphens inside the word are fine, and the cap is configurable.
        assert!(normalize_clue_word("deep-sea", 20).is_ok());
        assert!(normalize_clue_word("  Deep-Sea\n", 20).is_ok_and(|w| w == "deep-sea"));
        assert!(normalize_clue_word("deep-sea", 4).is_err());
        assert_eq!(GameConfig::default().clue_word_max_len, 20);
    }

    #[test]
    fn clue_numbers_are_bounded_by_remaining_agents() {
        let mut state = testkit::scripted_game(4);
        let clue = testkit::legal_clue(&state);
        for (number, expected) in [(0u8, "at least 1"), (10, "exceeds the 9 agent(s)")] {
            let err = state
                .apply(
                    SEAT_A_SPYMASTER,
                    Action::GiveClue {
                        word: clue.clone(),
                        number,
                    },
                )
                .expect_err("out-of-range number");
            assert!(err.0.contains(expected), "{number} gave {}", err.0);
        }
        state
            .apply(
                SEAT_A_SPYMASTER,
                Action::GiveClue {
                    word: clue,
                    number: 9,
                },
            )
            .expect("nine agents remain, so nine is legal");
    }
}
