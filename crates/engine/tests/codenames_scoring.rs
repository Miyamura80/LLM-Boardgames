//! Codenames scoring: the objective metric suite (PRD-codenames-evals US-CN10)
//! and the role-conditioned two-team rating (US-CN09), both driven over crafted
//! transcripts so every expectation has one unambiguous right answer. The
//! schedule and store halves live in `codenames_match.rs`.

use engine::codenames::board::KeyCard;
use engine::codenames::events::{CodenamesEvent, EventRecord};
use engine::codenames::metrics::{score_game, SeatMetrics};
use engine::codenames::rating::{leaderboard, RatingState, RatingTable};
use engine::codenames::runner::{GameRecord, SeatRecord};
use engine::codenames::types::*;
use engine::game_core::{Reliability, Visibility};
use engine::llm::TokenUsage;
// ===========================================================================
// US-CN10 — the metric suite over crafted transcripts
// ===========================================================================

/// 25 synthetic words with a canonical key: 0..9 team A agents, 9..17 team B,
/// 17..24 bystanders, 24 the assassin.
fn crafted_board() -> (Vec<String>, KeyCard) {
    let words: Vec<String> = (0..CARD_COUNT).map(|i| format!("w{i:02}")).collect();
    let identities = (0..CARD_COUNT)
        .map(|i| match i {
            0..=8 => CardIdentity::Agent { team: Team::A },
            9..=16 => CardIdentity::Agent { team: Team::B },
            17..=23 => CardIdentity::Bystander,
            _ => CardIdentity::Assassin,
        })
        .collect();
    (words, KeyCard { identities })
}

fn clue(word: &str, number: u8) -> Clue {
    Clue {
        word: word.into(),
        number,
    }
}

fn crafted_record(winner: Team, end_reason: EndReason, events: Vec<CodenamesEvent>) -> GameRecord {
    GameRecord {
        game_id: "crafted".into(),
        seed: 1,
        rules_version: RULES_VERSION.into(),
        wordlist_hash: "0".repeat(64),
        schedule_label: "crafted".into(),
        winner,
        end_reason,
        turns: 3,
        seats: (0..SEAT_COUNT)
            .map(|seat| SeatRecord {
                seat,
                role: seat_role(seat),
                team: seat_team(seat),
                model_id: format!("m{seat}"),
                agent_kind: "bot".into(),
                scaffold_version: "s".into(),
                temperature: None,
                is_anchor: false,
                won: seat_team(seat) == winner,
                reliability: Reliability::default(),
                thoughts: vec![],
                usage: TokenUsage::default(),
            })
            .collect(),
        events: events
            .into_iter()
            .enumerate()
            .map(|(idx, event)| EventRecord {
                idx: idx as u32,
                round: 1,
                visibility: Visibility::Public,
                event,
            })
            .collect(),
        duration_ms: 0,
    }
}

fn reveal(seat: Seat, word: &str, identity: CardIdentity, ends_turn: bool) -> CodenamesEvent {
    CodenamesEvent::GuessRevealed {
        seat,
        team: seat_team(seat),
        word: word.into(),
        identity,
        ends_turn,
    }
}

fn give(seat: Seat, word: &str, number: u8) -> CodenamesEvent {
    CodenamesEvent::ClueGiven {
        seat,
        team: seat_team(seat),
        clue: clue(word, number),
    }
}

/// One transcript exercising every metric branch: a clue paid in full plus an
/// overreaching bonus guess, a voluntary pass, and a forced clue whose fatal
/// outcome must not be charged to the spymaster.
fn worked_example() -> Vec<SeatMetrics> {
    let (words, key) = crafted_board();
    let events = vec![
        CodenamesEvent::GameStarted {
            starting_team: Team::A,
            wordlist_hash: "0".repeat(64),
            rules_version: RULES_VERSION.into(),
        },
        CodenamesEvent::BoardLaid { words },
        CodenamesEvent::TurnStarted {
            team: Team::A,
            turn: 1,
        },
        give(SEAT_A_SPYMASTER, "signal", 2),
        reveal(
            SEAT_A_OPERATIVE,
            "w00",
            CardIdentity::Agent { team: Team::A },
            false,
        ),
        reveal(
            SEAT_A_OPERATIVE,
            "w01",
            CardIdentity::Agent { team: Team::A },
            false,
        ),
        // The optional third (number + 1) guess, and it misses: overreach.
        reveal(SEAT_A_OPERATIVE, "w17", CardIdentity::Bystander, true),
        CodenamesEvent::TurnStarted {
            team: Team::B,
            turn: 2,
        },
        give(SEAT_B_SPYMASTER, "echo", 3),
        reveal(
            SEAT_B_OPERATIVE,
            "w09",
            CardIdentity::Agent { team: Team::B },
            false,
        ),
        CodenamesEvent::TurnPassed {
            seat: SEAT_B_OPERATIVE,
            team: Team::B,
        },
        CodenamesEvent::TurnStarted {
            team: Team::A,
            turn: 3,
        },
        // A forced clue: metric-exempt, and so is its fatal outcome.
        CodenamesEvent::ForcedDefault {
            seat: SEAT_A_SPYMASTER,
            decision: "give-clue".into(),
        },
        give(SEAT_A_SPYMASTER, "zzz", 1),
        reveal(SEAT_A_OPERATIVE, "w24", CardIdentity::Assassin, true),
        CodenamesEvent::GameEnded {
            winner: Team::B,
            reason: EndReason::Assassin,
            turns: 3,
        },
    ];
    let record = crafted_record(Team::B, EndReason::Assassin, events);
    score_game(&record, &key)
}

#[test]
fn spymaster_metrics_count_yield_and_ignore_forced_clues() {
    let m = &worked_example()[SEAT_A_SPYMASTER as usize];
    assert_eq!(m.role, Role::Spymaster);
    // The forced third-turn clue is excluded from every play-quality counter.
    assert_eq!(m.clues_given, 1);
    assert_eq!(m.mean_clue_number(), Some(2.0));
    assert_eq!(m.agents_found, 2);
    assert_eq!(m.agents_per_clue(), Some(2.0));
    assert_eq!(m.clue_yield(), Some(1.0), "two promised, two delivered");
    assert_eq!(m.bystander_hits_caused, 1);
    assert_eq!(m.enemy_hits_caused, 0);
    assert_eq!(
        m.assassin_hits_caused, 0,
        "the assassin fell under a FORCED clue — not the spymaster's doing"
    );
    // Outcome fields.
    assert!(!m.won && m.assassin_loss);
    assert_eq!(m.turns, 3);

    let b = &worked_example()[SEAT_B_SPYMASTER as usize];
    assert_eq!(b.clue_yield(), Some(1.0 / 3.0), "promised 3, delivered 1");
    assert!(b.won && !b.assassin_loss);
}

#[test]
fn operative_metrics_separate_first_guesses_from_the_bonus_overreach() {
    let m = &worked_example()[SEAT_A_OPERATIVE as usize];
    assert_eq!(m.guesses, 4);
    assert_eq!(m.guess_hits, 2);
    assert_eq!(m.guess_precision(), Some(0.5));
    // One first guess per clue, including the forced clue's (the *guess* was
    // the operative's own choice even though the clue was not).
    assert_eq!(m.first_guesses, 2);
    assert_eq!(m.first_guess_hits, 1);
    assert_eq!(m.first_guess_precision(), Some(0.5));
    // Only the (number + 1)th guess counts as overreach; the second guess of a
    // "2" clue is an ordinary guess.
    assert_eq!(m.bonus_guesses, 1);
    assert_eq!(m.bonus_misses, 1);
    assert_eq!(m.overreach_rate(), Some(1.0));
    assert_eq!(m.assassin_hits, 1);
    assert_eq!(m.passes, 0);
    assert_eq!(m.pass_discipline(), None, "no pass, no reading");
}

/// Pass discipline is the board hazard at the moment of the pass: four cards
/// were face-up, 21 face-down, and 7 of those were team B's remaining agents.
#[test]
fn pass_discipline_reads_the_board_hazard_from_the_key() {
    let m = &worked_example()[SEAT_B_OPERATIVE as usize];
    assert_eq!(m.passes, 1);
    let hazard = m.pass_discipline().expect("one pass scored");
    assert!(
        (hazard - (1.0 - 7.0 / 21.0)).abs() < 1e-9,
        "hazard was {hazard}"
    );
    assert_eq!(m.guesses, 1);
    assert_eq!(m.guess_hits, 1);
}

/// Forced guesses and forced passes are metric-exempt on both sides of the
/// attribution: they inform neither the operative's precision nor the
/// spymaster's yield.
#[test]
fn forced_guesses_and_passes_are_metric_exempt() {
    let (words, key) = crafted_board();
    let events = vec![
        CodenamesEvent::BoardLaid { words },
        give(SEAT_A_SPYMASTER, "signal", 2),
        // Forced first guess — it even hits the assassin, which is accepted
        // and precedented, but it is not evidence about anybody's play.
        CodenamesEvent::ForcedDefault {
            seat: SEAT_A_OPERATIVE,
            decision: "guess-or-pass".into(),
        },
        reveal(SEAT_A_OPERATIVE, "w24", CardIdentity::Assassin, true),
        CodenamesEvent::GameEnded {
            winner: Team::B,
            reason: EndReason::Assassin,
            turns: 1,
        },
    ];
    let metrics = score_game(&crafted_record(Team::B, EndReason::Assassin, events), &key);
    let op = &metrics[SEAT_A_OPERATIVE as usize];
    assert_eq!(op.guesses, 0);
    assert_eq!(op.assassin_hits, 0);
    let sm = &metrics[SEAT_A_SPYMASTER as usize];
    assert_eq!(sm.clues_given, 1, "the clue itself was freely chosen");
    assert_eq!(
        sm.assassin_hits_caused, 0,
        "a forced guess is not the spymaster's outcome"
    );
}

/// A pass with the guess cap already spent is not a decision, so it is not
/// scored as one.
#[test]
fn a_pass_with_no_guesses_left_is_not_pass_discipline() {
    let (words, key) = crafted_board();
    let events = vec![
        CodenamesEvent::BoardLaid { words },
        give(SEAT_A_SPYMASTER, "signal", 1),
        reveal(
            SEAT_A_OPERATIVE,
            "w00",
            CardIdentity::Agent { team: Team::A },
            false,
        ),
        reveal(
            SEAT_A_OPERATIVE,
            "w01",
            CardIdentity::Agent { team: Team::A },
            false,
        ),
        CodenamesEvent::TurnPassed {
            seat: SEAT_A_OPERATIVE,
            team: Team::A,
        },
        CodenamesEvent::GameEnded {
            winner: Team::A,
            reason: EndReason::AgentsFound,
            turns: 1,
        },
    ];
    let metrics = score_game(
        &crafted_record(Team::A, EndReason::AgentsFound, events),
        &key,
    );
    assert_eq!(metrics[SEAT_A_OPERATIVE as usize].passes, 0);
    // The second guess of a "1" clue is the bonus guess, and it hit.
    assert_eq!(metrics[SEAT_A_OPERATIVE as usize].bonus_guesses, 1);
    assert_eq!(metrics[SEAT_A_OPERATIVE as usize].bonus_misses, 0);
}

// ===========================================================================
// US-CN09 — role-conditioned two-team ratings
// ===========================================================================

fn outcome_record(models: [&str; 4], winner: Team) -> GameRecord {
    let mut record = crafted_record(winner, EndReason::AgentsFound, vec![]);
    for seat in &mut record.seats {
        seat.model_id = models[seat.seat as usize].into();
        seat.won = seat.team == winner;
    }
    record
}

#[test]
fn ratings_move_the_winner_up_and_key_the_two_roles_apart() {
    let mut table = RatingTable::default();
    for _ in 0..12 {
        table.update(&outcome_record(["sm-a", "op-a", "sm-b", "op-b"], Team::A));
    }
    let base = RatingState::default().mu;
    assert!(table.entities[&("sm-a".into(), Role::Spymaster)].mu > base);
    assert!(table.entities[&("op-a".into(), Role::Operative)].mu > base);
    assert!(table.entities[&("sm-b".into(), Role::Spymaster)].mu < base);
    // Role is part of the key, so a model has at most one entry per role.
    assert!(!table
        .entities
        .contains_key(&("sm-a".into(), Role::Operative)));

    // The two winning seats top the board (they tie), the two losers sit below.
    let rows = leaderboard(&table, 2.0);
    let ranked: Vec<&str> = rows.iter().map(|r| r.model_id.as_str()).collect();
    assert_eq!(
        &ranked[2..],
        &["op-b", "sm-b"],
        "losers rank last: {ranked:?}"
    );
    let sm = rows.iter().find(|r| r.model_id == "sm-a").unwrap();
    assert!(sm.spymaster.is_some() && sm.operative.is_none());
    assert!(sm.overall_conservative <= sm.overall_mu);
    // Side is a diagnostic, never a rating key.
    let sides: Vec<&str> = sm.sides.iter().map(|s| s.side.as_str()).collect();
    assert_eq!(sides, vec!["starting"]);
    assert_eq!(sm.sides[0].win_rate, 1.0);
}

#[test]
fn duplicate_entities_average_their_deltas_instead_of_double_counting() {
    let mut table = RatingTable::default();
    table.update(&outcome_record(["dup", "a", "dup", "b"], Team::A));
    let st = &table.entities[&("dup".to_string(), Role::Spymaster)];
    assert!(
        (st.mu - RatingState::default().mu).abs() < 1e-9,
        "a self-play role must net out, moved to {}",
        st.mu
    );
    assert_eq!(st.games, 2);
    assert_eq!(st.wins, 1);
    // Both sides show up in the diagnostic, one win each way.
    let row = leaderboard(&table, 2.0)
        .into_iter()
        .find(|r| r.model_id == "dup")
        .unwrap();
    assert_eq!(row.sides.len(), 2);
}
