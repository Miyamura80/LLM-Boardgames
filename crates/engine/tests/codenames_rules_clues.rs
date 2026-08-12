//! Codenames rules tests: the clue cluster of PRD-codenames-evals US-CN02 —
//! token shape (case, hyphens, digits, spaces), the length cap, collisions
//! with unrevealed board words (equal / contained / containing) versus the
//! freedom revealed words grant, and the number bounds.
//!
//! Clue legality is the load-bearing design point of the whole eval (PRD §6):
//! the rethink loop, the forced defaults, and the reliability counters all
//! hang off `apply()` being a deterministic authority. So this suite pins the
//! rejection *messages* exactly as well as the verdicts — they are re-prompt
//! input, and drifting wording silently changes what models are told.

use engine::codenames::actions::{Action, DecisionPoint};
use engine::codenames::events::CodenamesEvent;
use engine::codenames::state::{GameState, Phase};
use engine::codenames::testkit;
use engine::codenames::types::*;
use engine::codenames::wordlist::Wordlist;

fn clue(word: &str, number: u8) -> Action {
    Action::GiveClue {
        word: word.to_string(),
        number,
    }
}

/// The engine's message for a rejected clue (without the `Display` prefix).
fn reject(state: &mut GameState, word: &str, number: u8) -> String {
    state
        .apply(SEAT_A_SPYMASTER, clue(word, number))
        .expect_err("clue should have been rejected")
        .0
}

/// The board word the engine will name as the collision: the first face-down
/// card, in grid order, that the clue equals, contains, or sits inside.
fn first_collision(state: &GameState, clue: &str) -> String {
    state
        .board
        .cards
        .iter()
        .find(|c| !c.revealed && (c.word.contains(clue) || clue.contains(c.word.as_str())))
        .map(|c| c.word.clone())
        .expect("expected a colliding board word")
}

fn overlap_message(state: &GameState, normalized: &str) -> String {
    let board_word = first_collision(state, normalized);
    format!(
        "clue \"{normalized}\" overlaps the unrevealed board word \"{board_word}\": a clue may \
         not equal, contain, or be contained in an unrevealed board word"
    )
}

#[test]
fn clue_words_are_trimmed_and_lowercased_before_they_are_recorded() {
    let mut state = testkit::scripted_game(4);
    let word = testkit::legal_clue(&state);

    state
        .apply(
            SEAT_A_SPYMASTER,
            clue(&format!("  {}\n", word.to_uppercase()), 2),
        )
        .expect("case and surrounding whitespace are forgiven");

    assert_eq!(state.clues[0].clue.word, word, "stored normalized");
    assert_eq!(state.clues[0].clue.number, 2);
    // The transcript keeps the submitted spelling beside the normalized word,
    // so an offline audit can still see exactly what was written.
    let submitted = format!("  {}\n", word.to_uppercase());
    assert!(state.events.iter().any(|e| matches!(
        &e.event,
        CodenamesEvent::ClueGiven { clue, team: Team::A, raw_word, .. }
            if clue.word == word && raw_word.as_deref() == Some(submitted.as_str())
    )));
    assert!(matches!(
        state.phase,
        Phase::Guess {
            guesses_used: 0,
            ..
        }
    ));
}

/// An already-normalized submission carries no redundant copy of itself.
#[test]
fn a_normalized_submission_records_no_raw_spelling() {
    let mut state = testkit::scripted_game(4);
    let word = testkit::legal_clue(&state);
    state
        .apply(SEAT_A_SPYMASTER, clue(&word, 1))
        .expect("a normalized token is legal");
    assert!(state
        .events
        .iter()
        .any(|e| matches!(&e.event, CodenamesEvent::ClueGiven { raw_word: None, .. })));
}

/// Play a whole game on nothing but forced defaults, asserting every one of
/// them is legal — the property the clue-word cap validation exists to protect.
/// Returns the clue words the forced defaults produced.
fn play_out_on_forced_defaults(seed: u64, wordlist: Wordlist, rules: GameConfig) -> Vec<String> {
    let mut state = GameState::with_wordlist(seed, wordlist, rules);
    let mut clues = Vec::new();
    let mut steps = 0;
    while !state.is_over() {
        let decision = state.pending_decisions().remove(0);
        let action = state.forced_default(&decision);
        if let Action::GiveClue { word, .. } = &action {
            clues.push(word.clone());
        }
        state
            .apply(decision.seat(), action)
            .expect("a forced default must be legal by construction");
        steps += 1;
        assert!(steps < 500, "game failed to terminate");
    }
    clues
}

/// A clue-word cap below the shortest word of the pool the game is dealt from
/// makes every candidate — and every forced default — illegal, which the shared
/// resolver turns into a panic. It is rejected where a game is built, so the
/// degenerate value can no longer reach the forced-default path at all.
///
/// The floor is the *effective* pool's, not a constant: `clue_word_max_len` and
/// the wordlist are only degenerate as a pair.
#[test]
fn a_degenerate_clue_word_cap_is_rejected_before_a_game_exists() {
    let vendored = Wordlist::default_embedded();
    for cap in [0usize, 1, 2] {
        let err = GameConfig::new(cap, vendored.min_word_len()).expect_err("degenerate cap");
        assert!(err.contains("clue_word_max_len"), "{err}");
        assert!(err.contains("at least 3"), "{err}");
    }

    // At the vendored pool's floor a game still plays out entirely on forced
    // defaults — the fallback always has something legal to draw.
    let rules = GameConfig::new(3, vendored.min_word_len()).expect("the pool's floor is allowed");
    for seed in 0..4u64 {
        play_out_on_forced_defaults(seed, vendored.clone(), rules.clone());
    }
}

/// The other half of the pair: a custom pool of two-letter words at a cap of
/// two is a legitimate game — a constant floor rejected it — and it stays
/// playable on forced defaults alone.
///
/// The pool is exactly one grid's worth, so every word is on the board and
/// every board word blocks itself: the pool draw runs dry and the synthetic
/// ladder has to carry every clue. Between them the 25 words also use all 26
/// letters, so no single letter is legal either and the ladder has to climb.
#[test]
fn a_short_word_pool_is_playable_at_a_matching_cap() {
    let l: Vec<char> = ('a'..='z').collect();
    let mut words: Vec<String> = (0..13)
        .map(|i| format!("{}{}", l[2 * i], l[2 * i + 1]))
        .collect();
    words.extend((0..CARD_COUNT - 13).map(|i| format!("{}{}", l[i], l[i + 2])));
    let pool = Wordlist::from_words(words).expect("25 distinct two-letter words");
    assert_eq!(pool.len(), CARD_COUNT, "exactly one grid's worth");
    assert_eq!(pool.min_word_len(), 2);

    let rules = GameConfig::new(2, pool.min_word_len()).expect("two-letter words fit a cap of two");
    for seed in 0..8u64 {
        let clues = play_out_on_forced_defaults(seed, pool.clone(), rules.clone());
        assert!(!clues.is_empty(), "a game gives at least one clue");
        // Before the first reveal the whole pool is on the board and blocks
        // itself, so that clue can only have come from the synthetic ladder.
        // (Later ones may draw a pool word again: a revealed word is legal.)
        assert!(
            !pool.words().contains(&clues[0]),
            "the opening clue must come from the synthetic ladder: {clues:?}"
        );
    }

    // A cap under the structural floor is still refused, whatever the pool.
    assert!(GameConfig::new(0, pool.min_word_len()).is_err());
    assert_eq!(MIN_CLUE_WORD_MAX_LEN, 1, "one character is the hard floor");
}

#[test]
fn hyphens_are_legal_inside_the_word_and_nowhere_else() {
    let mut state = testkit::scripted_game(4);
    state
        .apply(SEAT_A_SPYMASTER, clue("Deep-Sea", 1))
        .expect("an interior hyphen is part of a single word");
    assert_eq!(state.clues[0].clue.word, "deep-sea");

    let mut state = testkit::scripted_game(4);
    for bad in ["-lead", "lead-", "-", "co--op-"] {
        assert_eq!(
            reject(&mut state, bad, 1),
            format!(
                "clue \"{bad}\" must be letters only, with hyphens allowed inside the word \
                 (no digits, punctuation, or spaces)"
            ),
            "hyphen placement {bad:?}"
        );
    }
    // A doubled interior hyphen is ugly but not a rules violation.
    state
        .apply(SEAT_A_SPYMASTER, clue("co--op", 1))
        .expect("interior hyphens are not counted");
}

#[test]
fn a_clue_must_be_one_alphabetic_token() {
    let mut state = testkit::scripted_game(4);

    for bad in ["two words", "deep sea now"] {
        assert_eq!(
            reject(&mut state, bad, 1),
            format!("clue \"{bad}\" must be a single word: spaces are not allowed")
        );
    }
    for blank in ["", "   ", "\t\n"] {
        assert_eq!(
            reject(&mut state, blank, 1),
            "a clue must be a single word, but none was given"
        );
    }
    for bad in ["agent7", "7", "over!", "caf\u{e9}", "under_score", "a.b"] {
        assert_eq!(
            reject(&mut state, bad, 1),
            format!(
                "clue \"{bad}\" must be letters only, with hyphens allowed inside the word \
                 (no digits, punctuation, or spaces)"
            ),
            "token {bad:?}"
        );
    }
}

#[test]
fn the_clue_length_cap_is_enforced_and_configurable() {
    let mut state = testkit::scripted_game(4);
    assert_eq!(GameConfig::default().clue_word_max_len, 20);
    let twenty = "a".repeat(20);
    let twenty_one = "a".repeat(21);

    state
        .apply(SEAT_A_SPYMASTER, clue(&twenty, 1))
        .expect("exactly at the cap is legal");
    let mut state = testkit::scripted_game(4);
    assert_eq!(
        reject(&mut state, &twenty_one, 1),
        format!("clue \"{twenty_one}\" is 21 characters long; the limit is 20")
    );

    // A tightened cap is honored, and measured on the trimmed word.
    let mut tight = GameState::with_wordlist(
        4,
        testkit::test_wordlist(),
        GameConfig {
            clue_word_max_len: 6,
        },
    );
    let err = tight
        .apply(SEAT_A_SPYMASTER, clue("  seventeen  ", 1))
        .expect_err("nine characters against a cap of six")
        .0;
    assert_eq!(
        err,
        "clue \"seventeen\" is 9 characters long; the limit is 6"
    );
    tight
        .apply(SEAT_A_SPYMASTER, clue("sixchr", 1))
        .expect("six characters fits a cap of six");
}

/// The substring rule, in all three directions, plus the case-insensitivity
/// that keeps it from being trivially dodged.
#[test]
fn clues_may_not_touch_an_unrevealed_board_word() {
    let mut state = testkit::scripted_game(11);
    let board_word = state.board.cards[0].word.clone();
    let prefix: String = board_word.chars().take(3).collect();
    let superstring = format!("{board_word}s");

    for bad in [
        board_word.clone(),
        board_word.to_uppercase(),
        format!("  {board_word}  "),
        prefix,
        superstring,
    ] {
        let normalized = bad.trim().to_ascii_lowercase();
        assert_eq!(
            reject(&mut state, &bad, 1),
            overlap_message(&state, &normalized),
            "collision {bad:?}"
        );
    }

    // Every other board word is equally off limits, not just the first.
    for card in state.board.words() {
        let normalized = card.clone();
        assert_eq!(
            reject(&mut state, &card, 1),
            overlap_message(&state, &normalized)
        );
    }
}

/// A revealed word stops constraining clues — the official rule, and the one
/// place where clue legality changes as a game progresses.
#[test]
fn revealed_board_words_become_legal_clues() {
    let mut state = testkit::scripted_game(11);
    let word = testkit::word_with(&state, CardIdentity::Agent { team: Team::A });
    assert_eq!(
        reject(&mut state, &word, 1),
        overlap_message(&state, &word),
        "illegal while face-down"
    );

    testkit::reveal(&mut state, &word);
    state
        .apply(SEAT_A_SPYMASTER, clue(&word, 1))
        .expect("a revealed word is a legal clue");
    assert_eq!(state.clues[0].clue.word, word);
}

#[test]
fn clue_numbers_run_from_one_to_the_teams_remaining_agents() {
    let mut state = testkit::scripted_game(4);
    let word = testkit::legal_clue(&state);

    assert_eq!(
        reject(&mut state, &word, 0),
        "clue number must be at least 1 (0 and \"unlimited\" clues are out of scope)"
    );
    assert_eq!(
        reject(&mut state, &word, 10),
        "clue number 10 exceeds the 9 agent(s) your team still has unrevealed"
    );
    assert_eq!(
        reject(&mut state, &word, u8::MAX),
        "clue number 255 exceeds the 9 agent(s) your team still has unrevealed"
    );
    match state.pending_decisions().remove(0) {
        DecisionPoint::GiveClue { max_number, .. } => assert_eq!(max_number, STARTING_AGENTS),
        other => panic!("expected a clue decision, got {other:?}"),
    }
    state
        .apply(SEAT_A_SPYMASTER, clue(&word, 9))
        .expect("nine agents remain, so nine is legal");

    // The ceiling tracks the key: reveal agents and it drops with them.
    let mut state = testkit::scripted_game(4);
    for word in testkit::words_with(&state, CardIdentity::Agent { team: Team::A })
        .into_iter()
        .take(7)
    {
        testkit::reveal(&mut state, &word);
    }
    let word = testkit::legal_clue(&state);
    match state.pending_decisions().remove(0) {
        DecisionPoint::GiveClue { max_number, .. } => assert_eq!(max_number, 2),
        other => panic!("expected a clue decision, got {other:?}"),
    }
    assert_eq!(
        reject(&mut state, &word, 3),
        "clue number 3 exceeds the 2 agent(s) your team still has unrevealed"
    );
    state
        .apply(SEAT_A_SPYMASTER, clue(&word, 2))
        .expect("two agents remain, so two is legal");
}

/// Token validation runs before the number check, so an agent that got both
/// wrong is told about the word first — a stable, deterministic feedback order.
#[test]
fn a_rejected_clue_leaves_the_decision_open_and_the_transcript_untouched() {
    let mut state = testkit::scripted_game(4);
    let events_before = state.events.len();
    let decision_before = state.pending_decisions();

    assert!(reject(&mut state, "two words", 0).contains("must be a single word"));
    assert_eq!(state.events.len(), events_before, "no event was recorded");
    assert!(state.clues.is_empty(), "no clue was recorded");
    assert_eq!(state.phase, Phase::Clue);
    assert_eq!(state.pending_decisions(), decision_before);
    assert_eq!(state.turn, 1);

    // Wrong action for the phase is reported by name, not silently ignored.
    testkit::give_clue(&mut state, 1);
    let err = state
        .apply(SEAT_A_OPERATIVE, clue("extra", 1))
        .expect_err("the operative owes a guess, not a clue")
        .0;
    assert_eq!(err, "give_clue is not a valid action in phase guess");
}
