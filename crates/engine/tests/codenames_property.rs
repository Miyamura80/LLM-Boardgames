//! Property tests: randomized seeded Codenames games driven end to end by
//! four [`RandomLegalBot`]s through the shared `game_core` rethink loop
//! (PRD-codenames-evals US-CN02/US-CN06).
//!
//! The retry budget is deliberately 1: a single illegal or malformed action
//! would immediately show up as a forced default, so "every action the random
//! bot produces is legal by construction" is asserted rather than assumed.

use engine::codenames::actions::DecisionPoint;
use engine::codenames::agents::{RandomLegalBot, SeatAgent};
use engine::codenames::events::CodenamesEvent;
use engine::codenames::state::GameState;
use engine::codenames::types::*;
use engine::codenames::wordlist::Wordlist;
use engine::game_core::{resolve_decision, Reliability};
use std::time::Instant;

/// Codenames reveals at least one card per turn (the mandatory first guess),
/// so 25 cards bound the game at 25 turns.
const MAX_TURNS: u32 = 25;

fn table(seed: u64) -> Vec<Box<dyn SeatAgent>> {
    (0..SEAT_COUNT as u64)
        .map(|s| Box::new(RandomLegalBot::new(seed ^ (s << 8))) as Box<dyn SeatAgent>)
        .collect()
}

/// Play one full game, asserting the per-step invariants as it goes.
async fn play(seed: u64) -> (GameState, Reliability) {
    let mut state = GameState::new(seed);
    let mut agents = table(seed);
    let mut reliability = Reliability::default();
    let mut steps = 0u32;

    while !state.is_over() {
        steps += 1;
        assert!(steps <= 4 * MAX_TURNS + 4, "seed {seed}: no termination");

        let decisions = state.pending_decisions();
        assert_eq!(decisions.len(), 1, "codenames is fully sequential");
        let decision = decisions[0].clone();
        assert_eq!(
            decision.seat(),
            expected_seat(&state, &decision),
            "seed {seed}: a decision reached a seat that is not on the clock"
        );

        let seat = decision.seat();
        let outcome = resolve_decision(
            1,
            &mut state,
            &mut agents[seat as usize],
            &decision,
            &mut reliability,
        )
        .await;
        assert!(
            !outcome.forced,
            "seed {seed}: seat {seat} produced an action the engine rejected"
        );
        check_card_accounting(&state, seed);
        assert!(state.turn <= MAX_TURNS, "seed {seed}: turn {}", state.turn);
    }

    assert!(state.winner.is_some() && state.end_reason.is_some());
    assert!(state.pending_decisions().is_empty());
    (state, reliability)
}

/// The seat the rules put on the clock, derived independently of the decision.
fn expected_seat(state: &GameState, decision: &DecisionPoint) -> Seat {
    match decision {
        DecisionPoint::GiveClue { .. } => state.active.spymaster(),
        DecisionPoint::GuessOrPass { .. } => state.active.operative(),
    }
}

/// The key card is fixed at setup: 9 / 8 / 7 / 1, whatever has been flipped.
/// Revealed cards are exactly the cards the transcript says were revealed.
fn check_card_accounting(state: &GameState, seed: u64) {
    let count = |want: CardIdentity| {
        state
            .board
            .cards
            .iter()
            .filter(|c| c.identity == want)
            .count()
    };
    assert_eq!(state.board.cards.len(), CARD_COUNT, "seed {seed}");
    assert_eq!(
        count(CardIdentity::Agent { team: Team::A }),
        9,
        "seed {seed}"
    );
    assert_eq!(
        count(CardIdentity::Agent { team: Team::B }),
        8,
        "seed {seed}"
    );
    assert_eq!(count(CardIdentity::Bystander), 7, "seed {seed}");
    assert_eq!(count(CardIdentity::Assassin), 1, "seed {seed}");

    for team in TEAMS {
        let revealed = state
            .board
            .cards
            .iter()
            .filter(|c| c.revealed && c.identity == CardIdentity::Agent { team })
            .count() as u8;
        assert_eq!(
            revealed + state.remaining_agents(team),
            team.agent_count(),
            "seed {seed}: {team:?} agent accounting"
        );
    }

    let revealed = state.board.cards.iter().filter(|c| c.revealed).count();
    let flips = state
        .events
        .iter()
        .filter(|e| matches!(e.event, CodenamesEvent::GuessRevealed { .. }))
        .count();
    assert_eq!(revealed, flips, "seed {seed}: reveals match the transcript");
    assert!(revealed <= CARD_COUNT);
}

/// Replay the transcript and re-derive clue legality from scratch: no clue was
/// ever accepted while it equaled, contained, or sat inside a board word that
/// was still face-down at the time.
fn check_clue_legality(state: &GameState, seed: u64) {
    let pool = Wordlist::default_embedded();
    let mut grid: Vec<(String, bool)> = Vec::new();
    let mut clues = 0;

    for record in &state.events {
        match &record.event {
            CodenamesEvent::BoardLaid { words } => {
                grid = words.iter().map(|w| (w.clone(), false)).collect();
            }
            CodenamesEvent::ClueGiven { clue, .. } => {
                clues += 1;
                assert!(!grid.is_empty(), "seed {seed}: clue before the board");
                for (word, revealed) in &grid {
                    assert!(
                        *revealed || !(word.contains(&clue.word) || clue.word.contains(word)),
                        "seed {seed}: clue {:?} touches face-down word {word:?}",
                        clue.word
                    );
                }
                assert!(clue.number >= 1 && clue.number <= STARTING_AGENTS);
                assert!(
                    pool.words().contains(&clue.word),
                    "seed {seed}: {:?} is not a pool word",
                    clue.word
                );
            }
            CodenamesEvent::GuessRevealed { word, .. } => {
                let card = grid
                    .iter_mut()
                    .find(|(w, _)| w == word)
                    .unwrap_or_else(|| panic!("seed {seed}: {word:?} is not on the board"));
                assert!(!card.1, "seed {seed}: {word:?} was revealed twice");
                card.1 = true;
            }
            _ => {}
        }
    }
    assert!(clues > 0, "seed {seed}: a game with no clues");
}

#[tokio::test]
async fn random_bot_games_terminate_with_intact_invariants() {
    for seed in 0..40u64 {
        let (state, reliability) = play(seed).await;
        check_clue_legality(&state, seed);

        // Legal by construction: nothing the bots did needed a retry or a
        // forced default, and they spend no tokens.
        assert_eq!(reliability.illegal_moves, 0, "seed {seed}");
        assert_eq!(reliability.malformed_outputs, 0, "seed {seed}");
        assert_eq!(reliability.forced_defaults, 0, "seed {seed}");
        assert_eq!(reliability.transport_failures, 0, "seed {seed}");
        assert!(!state
            .events
            .iter()
            .any(|e| matches!(e.event, CodenamesEvent::ForcedDefault { .. })));

        // The winner is one of the two real endings, and the transcript's own
        // turn count agrees with the state.
        let winner = state.winner.expect("game over");
        match state.end_reason.expect("game over") {
            EndReason::AgentsFound => assert_eq!(state.remaining_agents(winner), 0),
            EndReason::Assassin => assert!(state
                .board
                .cards
                .iter()
                .any(|c| c.revealed && c.identity == CardIdentity::Assassin)),
        }
        assert!(matches!(
            state.events.last().map(|e| &e.event),
            Some(CodenamesEvent::GameEnded { winner: w, turns, .. })
                if *w == winner && *turns == state.turn
        ));
    }
}

/// Every step of a live game offers its decision to exactly one seat; the
/// other three are refused, not merely ignored.
#[tokio::test]
async fn no_decision_is_ever_offered_to_a_non_pending_seat() {
    let mut state = GameState::new(5);
    let mut agents = table(5);
    let mut reliability = Reliability::default();

    while !state.is_over() {
        let decision = state.pending_decisions().remove(0);
        let on_clock = decision.seat();
        let role = seat_role(on_clock);
        match &decision {
            DecisionPoint::GiveClue { .. } => assert_eq!(role, Role::Spymaster),
            DecisionPoint::GuessOrPass { .. } => assert_eq!(role, Role::Operative),
        }
        assert_eq!(seat_team(on_clock), state.active);

        for seat in 0..SEAT_COUNT {
            if seat == on_clock {
                continue;
            }
            assert!(
                !state.pending_decisions().iter().any(|d| d.seat() == seat),
                "seat {seat} was offered a decision off turn"
            );
            let action = state.forced_default(&decision);
            let err = state
                .apply(seat, action)
                .expect_err("an off-turn seat must be refused")
                .0;
            assert!(err.contains("no pending decision"), "{err}");
        }

        resolve_decision(
            1,
            &mut state,
            &mut agents[on_clock as usize],
            &decision,
            &mut reliability,
        )
        .await;
    }
    assert_eq!(reliability.forced_defaults, 0);
}

/// Determinism over a wide seed sweep: same seed, same bots, byte-identical
/// transcript — the property every replay, report, and rating run rests on.
#[tokio::test]
async fn seeded_games_replay_bit_for_bit_across_many_seeds() {
    let mut transcripts = Vec::new();
    for seed in 0..200u64 {
        let (a, _) = play(seed).await;
        let (b, _) = play(seed).await;
        let ja = serde_json::to_string(&a.events).expect("events serialize");
        let jb = serde_json::to_string(&b.events).expect("events serialize");
        assert_eq!(ja, jb, "seed {seed} did not replay identically");
        transcripts.push(ja);
    }
    // Seeds are not all secretly the same game.
    let mut distinct = transcripts.clone();
    distinct.sort();
    distinct.dedup();
    assert!(
        distinct.len() > transcripts.len() * 9 / 10,
        "seeds collapse onto {} distinct transcripts",
        distinct.len()
    );
}

/// A four-random-bot game is the zero-token CI smoke test, so it has to stay
/// cheap. The bound is loose enough for an unoptimized debug build and still
/// catches an order-of-magnitude regression (US-CN06: "sub-second").
#[tokio::test]
async fn a_four_bot_game_is_a_sub_second_smoke_test() {
    let start = Instant::now();
    let (state, _) = play(7).await;
    let elapsed = start.elapsed();
    assert!(state.is_over());
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "a 4-random-bot game took {elapsed:?}"
    );

    // Seat identity is what the rating layer keys on, and a bot is free.
    let bot = RandomLegalBot::new(7);
    assert_eq!(bot.kind(), "bot:codenames-random");
    assert_eq!(bot.model_id(), "bot:codenames-random");
    assert_eq!(bot.scaffold_version(), "bot-random-v1");
    assert_eq!(bot.usage().prompt_tokens, 0);
    assert_eq!(bot.usage().completion_tokens, 0);
}
