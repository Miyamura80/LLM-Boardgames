//! Guess resolution: reveal one card and decide whether the operative keeps
//! going, the turn passes, or the game is over. Own agent → continue up to
//! number + 1 guesses; bystander or enemy agent → turn ends (an enemy agent
//! still counts for the enemy); assassin → the guessing team loses at once.

use super::super::actions::IllegalMove;
use super::super::events::CodenamesEvent;
use super::super::state::{GameState, GuessOutcome, Phase};
use super::super::types::{seat_team, CardIdentity, Clue, EndReason, Seat};
use crate::game_core::Visibility;

pub(super) fn guess(
    state: &mut GameState,
    seat: Seat,
    word: &str,
    clue: &Clue,
    guesses_used: u8,
) -> Result<(), IllegalMove> {
    let needle = word.trim().to_ascii_lowercase();
    let index = state
        .board
        .find_word(&needle)
        .ok_or_else(|| IllegalMove(format!("\"{needle}\" is not a word on this board")))?;
    if state.board.cards[index].revealed {
        return Err(IllegalMove(format!(
            "\"{needle}\" is already revealed; guess one of the face-down words"
        )));
    }

    let team = seat_team(seat);
    state.board.cards[index].revealed = true;
    let identity = state.board.cards[index].identity;
    let used = guesses_used.saturating_add(1);
    // Only an own agent keeps the turn alive, and only within the clue's
    // number + 1 guess cap.
    let continues = identity.agent_team() == Some(team) && used < clue.guess_cap();

    if let Some(record) = state.clues.last_mut() {
        record.outcomes.push(GuessOutcome {
            word: needle.clone(),
            identity,
        });
    }
    state.push_event(
        Visibility::Public,
        CodenamesEvent::GuessRevealed {
            seat,
            team,
            word: needle,
            identity,
            ends_turn: !continues,
        },
    );

    // The assassin ends everything; otherwise a reveal can complete either
    // team's set — including the opponent's, via this team's mis-guess.
    if identity == CardIdentity::Assassin {
        state.finish(team.other(), EndReason::Assassin);
        return Ok(());
    }
    if let Some(owner) = identity.agent_team() {
        if state.remaining_agents(owner) == 0 {
            state.finish(owner, EndReason::AgentsFound);
            return Ok(());
        }
    }

    if continues {
        state.phase = Phase::Guess {
            clue: clue.clone(),
            guesses_used: used,
        };
    } else {
        super::end_turn(state);
    }
    Ok(())
}

/// Ending the turn voluntarily. Illegal before the mandatory first guess.
pub(super) fn pass(state: &mut GameState, seat: Seat, guesses_used: u8) -> Result<(), IllegalMove> {
    if guesses_used == 0 {
        return Err(IllegalMove(
            "you must make at least one guess for this clue before passing".into(),
        ));
    }
    if let Some(record) = state.clues.last_mut() {
        record.passed = true;
    }
    state.push_event(
        Visibility::Public,
        CodenamesEvent::TurnPassed {
            seat,
            team: seat_team(seat),
        },
    );
    super::end_turn(state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::actions::Action;
    use crate::codenames::testkit;
    use crate::codenames::types::*;

    #[test]
    fn the_first_guess_of_a_clue_is_mandatory() {
        let mut state = testkit::scripted_game(21);
        testkit::give_clue(&mut state, 2);
        let err = state
            .apply(SEAT_A_OPERATIVE, Action::Pass)
            .expect_err("pass before the first guess");
        assert!(err.0.contains("at least one guess"), "{}", err.0);

        // After one guess a pass is legal and hands the turn over.
        let own = testkit::word_with(&state, CardIdentity::Agent { team: Team::A });
        state
            .apply(SEAT_A_OPERATIVE, Action::Guess { word: own })
            .expect("own agent");
        state.apply(SEAT_A_OPERATIVE, Action::Pass).expect("pass");
        assert_eq!(state.active, Team::B);
        assert_eq!(state.phase, Phase::Clue);
        assert!(state.clues[0].passed);
    }

    /// A clue of N entitles the operative to N + 1 guesses, no more.
    #[test]
    fn own_agents_continue_the_turn_up_to_the_guess_cap() {
        let mut state = testkit::scripted_game(21);
        testkit::give_clue(&mut state, 2);
        for _ in 0..3 {
            assert_eq!(state.active, Team::A);
            let own = testkit::word_with(&state, CardIdentity::Agent { team: Team::A });
            state
                .apply(SEAT_A_OPERATIVE, Action::Guess { word: own })
                .expect("own agent keeps the turn alive");
        }
        // Three guesses (2 + 1) is the cap: the turn has passed to B.
        assert_eq!(state.active, Team::B);
        assert_eq!(state.clues[0].outcomes.len(), 3);
    }

    #[test]
    fn bystanders_and_enemy_agents_end_the_turn() {
        for identity in [
            CardIdentity::Bystander,
            CardIdentity::Agent { team: Team::B },
        ] {
            let mut state = testkit::scripted_game(21);
            testkit::give_clue(&mut state, 3);
            let word = testkit::word_with(&state, identity);
            state
                .apply(SEAT_A_OPERATIVE, Action::Guess { word })
                .expect("legal guess, wrong card");
            assert_eq!(state.active, Team::B, "{identity:?} ends the turn");
            assert!(!state.is_over());
        }
    }

    #[test]
    fn the_assassin_ends_the_game_as_a_loss_for_the_guessing_team() {
        let mut state = testkit::scripted_game(21);
        testkit::give_clue(&mut state, 1);
        let word = testkit::word_with(&state, CardIdentity::Assassin);
        state
            .apply(SEAT_A_OPERATIVE, Action::Guess { word })
            .expect("guessing the assassin is a legal move with a fatal outcome");
        assert!(state.is_over());
        assert_eq!(state.winner, Some(Team::B));
        assert_eq!(state.end_reason, Some(EndReason::Assassin));
        assert!(state
            .apply(SEAT_B_SPYMASTER, Action::Pass)
            .is_err_and(|e| e.0.contains("game is over")));
    }

    /// Revealing the opponent's last agent wins the game *for them*.
    #[test]
    fn a_mis_guess_can_hand_the_opponent_the_win() {
        let mut state = testkit::scripted_game(21);
        // Strip team B down to a single agent, then let A reveal it.
        let mut b_words = testkit::words_with(&state, CardIdentity::Agent { team: Team::B });
        let last = b_words.pop().expect("eight B agents");
        for word in b_words {
            testkit::reveal(&mut state, &word);
        }
        assert_eq!(state.remaining_agents(Team::B), 1);

        testkit::give_clue(&mut state, 1);
        state
            .apply(SEAT_A_OPERATIVE, Action::Guess { word: last })
            .expect("legal guess");
        assert_eq!(state.winner, Some(Team::B));
        assert_eq!(state.end_reason, Some(EndReason::AgentsFound));
    }

    #[test]
    fn guesses_must_name_an_unrevealed_board_word() {
        let mut state = testkit::scripted_game(21);
        testkit::give_clue(&mut state, 2);
        let err = state
            .apply(
                SEAT_A_OPERATIVE,
                Action::Guess {
                    word: "notonthisboard".into(),
                },
            )
            .expect_err("unknown word");
        assert!(err.0.contains("is not a word on this board"), "{}", err.0);

        let own = testkit::word_with(&state, CardIdentity::Agent { team: Team::A });
        state
            .apply(SEAT_A_OPERATIVE, Action::Guess { word: own.clone() })
            .expect("own agent");
        let err = state
            .apply(SEAT_A_OPERATIVE, Action::Guess { word: own })
            .expect_err("already revealed");
        assert!(err.0.contains("already revealed"), "{}", err.0);
    }

    #[test]
    fn only_the_seat_on_turn_may_act() {
        let mut state = testkit::scripted_game(21);
        let err = state
            .apply(SEAT_A_OPERATIVE, Action::Pass)
            .expect_err("the spymaster owes a clue");
        assert!(err.0.contains("no pending decision"), "{}", err.0);
        testkit::give_clue(&mut state, 1);
        let err = state
            .apply(
                SEAT_B_OPERATIVE,
                Action::Guess {
                    word: testkit::word_with(&state, CardIdentity::Bystander),
                },
            )
            .expect_err("not team B's turn");
        assert!(err.0.contains("no pending decision"), "{}", err.0);
    }
}
