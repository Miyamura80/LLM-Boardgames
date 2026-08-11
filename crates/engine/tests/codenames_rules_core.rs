//! Codenames rules tests: the guess-side clusters of PRD-codenames-evals
//! US-CN02 — guess-cap accounting, the mandatory first guess, turn end on a
//! bystander or enemy agent, the assassin loss, winning on the opponent's
//! mis-guess, winning on the last agent mid-streak, and seeded board/key
//! determinism. Clue validation lives in `codenames_rules_clues.rs`.
//!
//! These exercise the public API only. A few assertions pin `IllegalMove`
//! message text exactly: those strings are fed back to a model verbatim on the
//! rethink re-prompt, so their wording is part of the contract, not an
//! implementation detail.

use engine::codenames::actions::{Action, DecisionPoint};
use engine::codenames::events::CodenamesEvent;
use engine::codenames::state::{GameState, Phase};
use engine::codenames::testkit;
use engine::codenames::types::*;
use engine::codenames::wordlist::Wordlist;

/// The engine's message for a rejected action (without the `Display` prefix).
fn reject(state: &mut GameState, seat: Seat, action: Action) -> String {
    state
        .apply(seat, action)
        .expect_err("action should have been rejected")
        .0
}

fn guess_own_agent(state: &mut GameState, team: Team) {
    let word = testkit::word_with(state, CardIdentity::Agent { team });
    state
        .apply(team.operative(), Action::Guess { word })
        .expect("own agent is a legal guess");
}

/// The single decision the engine is waiting on.
fn pending(state: &GameState) -> DecisionPoint {
    let mut decisions = state.pending_decisions();
    assert_eq!(decisions.len(), 1, "codenames is fully sequential");
    decisions.remove(0)
}

#[test]
fn the_first_guess_of_a_clue_is_mandatory_and_a_later_pass_ends_the_turn() {
    let mut state = testkit::scripted_game(21);
    testkit::give_clue(&mut state, 2);

    assert_eq!(
        reject(&mut state, SEAT_A_OPERATIVE, Action::Pass),
        "you must make at least one guess for this clue before passing",
        "the mandatory-first-guess message is fed back to the model verbatim"
    );

    guess_own_agent(&mut state, Team::A);
    state
        .apply(SEAT_A_OPERATIVE, Action::Pass)
        .expect("a pass after the first guess is legal");

    assert_eq!(state.active, Team::B);
    assert_eq!(state.phase, Phase::Clue);
    assert_eq!(state.turn, 2, "passing opens the next team's turn");
    assert!(state.clues[0].passed);
    assert_eq!(state.clues[0].outcomes.len(), 1);
    assert_eq!(pending(&state).seat(), SEAT_B_SPYMASTER);
}

/// A clue of N buys exactly N + 1 guesses — the official bonus guess, no more.
#[test]
fn a_clue_of_n_entitles_the_operative_to_exactly_n_plus_one_guesses() {
    for number in 1..=3u8 {
        let mut state = testkit::scripted_game(21);
        testkit::give_clue(&mut state, number);

        let mut made = 0u8;
        while state.active == Team::A {
            match pending(&state) {
                DecisionPoint::GuessOrPass {
                    guesses_used,
                    guesses_remaining,
                    clue,
                    ..
                } => {
                    assert_eq!(guesses_used, made);
                    assert_eq!(guesses_remaining, number + 1 - made);
                    assert_eq!(clue.guess_cap(), number + 1);
                }
                other => panic!("expected a guess decision, got {other:?}"),
            }
            guess_own_agent(&mut state, Team::A);
            made += 1;
            assert!(made <= number + 1, "guessed past the cap for clue {number}");
        }

        assert_eq!(made, number + 1, "clue {number} should buy {number} + 1");
        assert_eq!(state.clues[0].outcomes.len(), (number + 1) as usize);
        assert!(!state.clues[0].passed, "the cap ended the turn, not a pass");
        assert_eq!(state.remaining_agents(Team::A), STARTING_AGENTS - made);
    }
}

#[test]
fn a_bystander_ends_the_turn_without_moving_either_score() {
    let mut state = testkit::scripted_game(21);
    testkit::give_clue(&mut state, 3);
    let word = testkit::word_with(&state, CardIdentity::Bystander);

    state
        .apply(SEAT_A_OPERATIVE, Action::Guess { word })
        .expect("a bystander is a legal, unlucky guess");

    assert_eq!(state.active, Team::B);
    assert!(!state.is_over());
    assert_eq!(state.remaining_agents(Team::A), STARTING_AGENTS);
    assert_eq!(state.remaining_agents(Team::B), SECOND_AGENTS);
    assert!(state.events.iter().any(|e| matches!(
        &e.event,
        CodenamesEvent::GuessRevealed {
            identity: CardIdentity::Bystander,
            ends_turn: true,
            ..
        }
    )));
}

#[test]
fn an_enemy_agent_ends_the_turn_and_scores_for_the_enemy() {
    let mut state = testkit::scripted_game(21);
    testkit::give_clue(&mut state, 3);
    let word = testkit::word_with(&state, CardIdentity::Agent { team: Team::B });

    state
        .apply(SEAT_A_OPERATIVE, Action::Guess { word })
        .expect("revealing an enemy agent is legal");

    assert_eq!(state.active, Team::B);
    assert!(!state.is_over());
    assert_eq!(state.remaining_agents(Team::A), STARTING_AGENTS);
    assert_eq!(
        state.remaining_agents(Team::B),
        SECOND_AGENTS - 1,
        "the reveal counts for team B even though team A made it"
    );
}

#[test]
fn the_assassin_is_an_immediate_loss_for_the_guessing_team() {
    let mut state = testkit::scripted_game(21);
    testkit::give_clue(&mut state, 3);
    let word = testkit::word_with(&state, CardIdentity::Assassin);

    state
        .apply(SEAT_A_OPERATIVE, Action::Guess { word })
        .expect("guessing the assassin is legal, with a fatal outcome");

    assert!(state.is_over());
    assert_eq!(state.winner, Some(Team::B));
    assert_eq!(state.end_reason, Some(EndReason::Assassin));
    assert_eq!(state.phase, Phase::GameOver);
    assert!(
        state.pending_decisions().is_empty(),
        "a finished game asks nobody for anything"
    );
    // Every seat is locked out, and the message says why.
    for seat in [
        SEAT_A_SPYMASTER,
        SEAT_A_OPERATIVE,
        SEAT_B_SPYMASTER,
        SEAT_B_OPERATIVE,
    ] {
        assert_eq!(reject(&mut state, seat, Action::Pass), "the game is over");
    }
}

/// Revealing the opponent's last agent wins the game *for them* — the reveal
/// scores for the card's owner regardless of who flipped it.
#[test]
fn a_mis_guess_can_hand_the_opponent_the_win() {
    let mut state = testkit::scripted_game(21);
    let mut b_words = testkit::words_with(&state, CardIdentity::Agent { team: Team::B });
    let last = b_words.pop().expect("eight B agents");
    for word in b_words {
        testkit::reveal(&mut state, &word);
    }
    assert_eq!(state.remaining_agents(Team::B), 1);

    testkit::give_clue(&mut state, 2);
    state
        .apply(SEAT_A_OPERATIVE, Action::Guess { word: last })
        .expect("legal guess, losing outcome");

    assert_eq!(state.winner, Some(Team::B));
    assert_eq!(state.end_reason, Some(EndReason::AgentsFound));
    assert!(state.remaining_agents(Team::A) > 0, "team A never finished");
    assert!(matches!(
        state.events.last().map(|e| &e.event),
        Some(CodenamesEvent::GameEnded {
            winner: Team::B,
            reason: EndReason::AgentsFound,
            ..
        })
    ));
}

/// The win lands the instant the last agent flips, mid-streak — the operative
/// does not get to spend the guess it still had in hand.
#[test]
fn a_team_wins_the_moment_its_last_agent_is_revealed_mid_streak() {
    let mut state = testkit::scripted_game(21);
    let mut a_words = testkit::words_with(&state, CardIdentity::Agent { team: Team::A });
    let keep: Vec<String> = a_words.drain(..2).collect();
    for word in a_words {
        testkit::reveal(&mut state, &word);
    }
    assert_eq!(state.remaining_agents(Team::A), 2);

    testkit::give_clue(&mut state, 2); // cap of three guesses
    for (i, word) in keep.into_iter().enumerate() {
        state
            .apply(SEAT_A_OPERATIVE, Action::Guess { word })
            .expect("own agent");
        if i == 0 {
            assert!(!state.is_over(), "one agent still face-down");
            assert!(matches!(
                state.phase,
                Phase::Guess {
                    guesses_used: 1,
                    ..
                }
            ));
        }
    }

    assert_eq!(state.winner, Some(Team::A));
    assert_eq!(state.end_reason, Some(EndReason::AgentsFound));
    assert_eq!(
        state.clues[0].outcomes.len(),
        2,
        "the third guess was never offered"
    );
    assert!(
        !matches!(
            state.events.last().map(|e| &e.event),
            Some(CodenamesEvent::TurnStarted { .. })
        ),
        "the game ended instead of opening another turn"
    );
}

#[test]
fn guesses_must_name_a_face_down_board_word() {
    let mut state = testkit::scripted_game(21);
    testkit::give_clue(&mut state, 2);

    assert_eq!(
        reject(
            &mut state,
            SEAT_A_OPERATIVE,
            Action::Guess {
                word: "notonthisboard".into(),
            },
        ),
        "\"notonthisboard\" is not a word on this board"
    );

    let own = testkit::word_with(&state, CardIdentity::Agent { team: Team::A });
    // Guesses are normalized like clues: case and surrounding space forgiven.
    state
        .apply(
            SEAT_A_OPERATIVE,
            Action::Guess {
                word: format!("  {}  ", own.to_uppercase()),
            },
        )
        .expect("a guess is trimmed and lowercased before lookup");
    assert_eq!(
        reject(
            &mut state,
            SEAT_A_OPERATIVE,
            Action::Guess { word: own.clone() }
        ),
        format!("\"{own}\" is already revealed; guess one of the face-down words")
    );
}

/// Exactly one seat is ever on the clock, and the other three are told so in
/// the same shape of message.
#[test]
fn only_the_seat_on_turn_receives_a_decision() {
    let mut state = testkit::scripted_game(21);
    let all = [
        SEAT_A_SPYMASTER,
        SEAT_A_OPERATIVE,
        SEAT_B_SPYMASTER,
        SEAT_B_OPERATIVE,
    ];

    for phase_name in ["clue", "guess"] {
        let on_clock = pending(&state).seat();
        assert_eq!(state.phase.name(), phase_name);
        for seat in all {
            if seat == on_clock {
                continue;
            }
            let action = Action::Guess {
                word: testkit::word_with(&state, CardIdentity::Bystander),
            };
            assert_eq!(
                reject(&mut state, seat, action),
                format!("seat {seat} has no pending decision in phase {phase_name}")
            );
        }
        if phase_name == "clue" {
            testkit::give_clue(&mut state, 1);
        }
    }
}

#[test]
fn seeded_boards_and_keys_are_reproducible_and_correctly_composed() {
    let key_of = |state: &GameState| {
        state
            .observe(SEAT_A_SPYMASTER)
            .key
            .expect("spymasters receive the key card")
            .identities
    };

    for seed in 0..8u64 {
        let a = GameState::new(seed);
        let b = GameState::new(seed);
        assert_eq!(a.board, b.board, "seed {seed}: same grid and same key");
        assert_eq!(key_of(&a), key_of(&b));

        let words = a.board.words();
        assert_eq!(words.len(), CARD_COUNT);
        let mut sorted = words.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), CARD_COUNT, "drawn without replacement");

        // 9 / 8 / 7 / 1, with the extra agent on the starting team.
        let key = key_of(&a);
        let count = |want: CardIdentity| key.iter().filter(|i| **i == want).count();
        assert_eq!(count(CardIdentity::Agent { team: Team::A }), 9);
        assert_eq!(count(CardIdentity::Agent { team: Team::B }), 8);
        assert_eq!(count(CardIdentity::Bystander), 7);
        assert_eq!(count(CardIdentity::Assassin), 1);
        assert_eq!(key.len(), CARD_COUNT);

        // The pool a board was drawn from is pinned in the record.
        assert_eq!(a.wordlist_hash, Wordlist::default_embedded().content_hash());
    }

    assert_ne!(
        GameState::new(1).board,
        GameState::new(2).board,
        "different seeds must give different games"
    );
    // The scripted pool is a different draw entirely, and says so.
    assert_ne!(
        testkit::scripted_game(1).wordlist_hash,
        GameState::new(1).wordlist_hash
    );
}
