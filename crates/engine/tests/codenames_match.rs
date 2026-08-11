//! The Codenames match layer, schedule and store halves (PRD-codenames-evals
//! US-CN08 / US-CN11): mirrored controlled schedules with exact role and side
//! coverage, arena rotation, and — when DATABASE_URL is set — the Postgres
//! store + match-runner round trip. The scoring halves (metrics US-CN10,
//! ratings US-CN09) live in `codenames_scoring.rs`.

use engine::codenames::metrics::key_card_for;
use engine::codenames::rating::leaderboard;
use engine::codenames::runner::{
    advance_match, arena_schedule, controlled_schedule, finalize_match, planned_distribution,
    AgentFactory, AgentSpec, GameConfig, MatchSpec,
};
use engine::codenames::state::GameState;
use engine::codenames::store::CodenamesStore;
use engine::codenames::types::*;
use engine::codenames::wordlist::Wordlist;
use std::collections::BTreeMap;

fn bot_factory() -> AgentFactory {
    AgentFactory {
        keys: Default::default(),
        retry: Default::default(),
        temperature: 0.5,
        max_tokens: 256,
        vectors: None,
    }
}

fn bot_pool() -> Vec<AgentSpec> {
    (0..3)
        .map(|_| AgentSpec::bot(engine::codenames::runner::CodenamesAgentKind::Random))
        .collect()
}

// ===========================================================================
// US-CN08 — mirrored, role- and side-balanced schedules
// ===========================================================================

/// The load-bearing variance control: two different candidates scheduled with
/// the same `match_seed` face the *identical* 25 words and the *identical*
/// 9/8/7/1 key card in every cell — verified by actually generating the boards,
/// not just by comparing seeds.
#[test]
fn controlled_cells_deal_identical_boards_and_keys_to_every_candidate() {
    let pool = bot_pool();
    let a = controlled_schedule(99, &AgentSpec::llm("model-a"), &pool, 3, 2).unwrap();
    let b = controlled_schedule(99, &AgentSpec::llm("model-b"), &pool, 3, 2).unwrap();
    assert_eq!(a.len(), 3 * 2 * 2 * 2, "boards × sides × roles × K");
    assert_eq!(a.len(), b.len());

    let board_of = |seed: u64| {
        GameState::with_wordlist(
            seed,
            Wordlist::default_embedded(),
            engine::codenames::types::GameConfig::default(),
        )
        .board
    };
    let mut seen_boards = 0;
    for (pa, pb) in a.iter().zip(&b) {
        assert_eq!(pa.label, pb.label, "cells must line up");
        assert_eq!(pa.seed, pb.seed, "seeds must be candidate-independent");
        let (ba, bb) = (board_of(pa.seed), board_of(pb.seed));
        assert_eq!(ba.words(), bb.words(), "{}: grids diverge", pa.label);
        assert_eq!(
            ba.key_card(),
            bb.key_card(),
            "{}: key cards diverge",
            pa.label
        );
        seen_boards += 1;
    }
    assert_eq!(seen_boards, a.len());

    // A different match_seed is a different family of boards.
    let c = controlled_schedule(100, &AgentSpec::llm("model-a"), &pool, 3, 2).unwrap();
    assert!(a.iter().zip(&c).all(|(x, y)| x.seed != y.seed));
}

/// Exact coverage: the candidate plays both roles on both sides, `boards × K`
/// times each, and never sits at two seats of one game.
#[test]
fn the_candidate_covers_every_role_and_side_exactly() {
    let plans = controlled_schedule(7, &AgentSpec::llm("cand"), &bot_pool(), 4, 3).unwrap();
    let mut cells: BTreeMap<(String, String), u32> = BTreeMap::new();
    for plan in &plans {
        let candidate_seats: Vec<usize> = plan
            .seats
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.is_anchor)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(candidate_seats.len(), 1, "exactly one candidate seat");
        let seat = candidate_seats[0] as Seat;
        let side = match seat_team(seat) {
            Team::A => "starting",
            Team::B => "second",
        };
        *cells
            .entry((side.into(), seat_role(seat).as_str().into()))
            .or_default() += 1;
    }
    assert_eq!(cells.len(), 4);
    assert!(
        cells.values().all(|&n| n == 4 * 3),
        "every (side, role) cell exactly boards × K times: {cells:?}"
    );
}

/// Arena rotation visits every seat evenly, and the planned distribution the
/// run finalizes against says so.
#[test]
fn arena_rotation_is_even_and_planned_coverage_is_logged() {
    let models: Vec<AgentSpec> = ["m0", "m1", "m2", "m3"].map(AgentSpec::llm).into();
    let plans = arena_schedule(5, &models, 12);
    assert_eq!(plans.len(), 12);
    let planned = planned_distribution(&plans);
    for m in ["m0", "m1", "m2", "m3"] {
        let by = &planned[m];
        for seat in 0..SEAT_COUNT {
            assert_eq!(by[&format!("seat{seat}")], 3, "{m} seat{seat}");
        }
        assert_eq!(by["role:spymaster"], 6);
        assert_eq!(by["role:operative"], 6);
        assert_eq!(by["side:starting"], 6);
        assert_eq!(by["side:second"], 6);
    }
}
// ===========================================================================
// US-CN11 — store round trip (skipped without a database)
// ===========================================================================

/// Full store round trip: schedule → play → persist → chunked resume → resume
/// no-op → finalize. Skips (passes vacuously) when no DATABASE_URL is
/// configured or the database is unreachable, exactly like `sh_match_e2e`.
#[tokio::test]
async fn match_runner_persists_and_resumes_with_postgres() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL not set");
        return;
    };
    let store = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        CodenamesStore::connect(&url),
    )
    .await
    {
        Ok(Ok(store)) => store,
        Ok(Err(e)) => {
            eprintln!("skipping: DATABASE_URL unreachable: {e}");
            return;
        }
        Err(_) => {
            eprintln!("skipping: DATABASE_URL connect timed out");
            return;
        }
    };
    let run_id = format!("codenames-test-run-{}", std::process::id());

    let spec = MatchSpec::Controlled {
        candidate: AgentSpec::bot(engine::codenames::runner::CodenamesAgentKind::Random),
        pool_name: "test-pool".into(),
        pool: bot_pool(),
        boards: 1,
        k: 1,
        match_seed: 7,
    };
    store
        .create_run(&run_id, spec.mode(), &serde_json::to_value(&spec).unwrap())
        .await
        .unwrap();

    let cfg = GameConfig::default();
    let factory = bot_factory();

    // Chunked continue: two games now, the rest on the next call.
    let first = advance_match(&store, &run_id, &spec, &factory, &cfg, 2)
        .await
        .unwrap();
    assert_eq!(first.total_games, 4, "1 board × 2 sides × 2 roles × K=1");
    assert_eq!(first.played, 2);
    assert_eq!(first.remaining, 2);

    let second = advance_match(&store, &run_id, &spec, &factory, &cfg, 1000)
        .await
        .unwrap();
    assert_eq!(second.played, 2, "the interrupted run finishes");
    assert_eq!(second.remaining, 0);

    // Resume is a no-op: every scheduled game is already stored.
    let third = advance_match(&store, &run_id, &spec, &factory, &cfg, 1000)
        .await
        .unwrap();
    assert_eq!(third.played, 0);
    assert_eq!(third.remaining, 0);

    let (rows, summary) = finalize_match(&store, &run_id, 2.0).await.unwrap();
    assert!(!rows.is_empty());
    assert_eq!(summary["games"], 4);

    // Realized coverage is computed from persisted seat rows: 4 games × 4 seats
    // per dimension (seat, role, side).
    let realized = summary["realized_distribution"]
        .as_object()
        .expect("realized_distribution present");
    let (mut seats, mut roles, mut sides) = (0u64, 0u64, 0u64);
    for model in realized.values() {
        for (key, count) in model.as_object().unwrap() {
            let n = count.as_u64().unwrap();
            if key.starts_with("seat") {
                seats += n;
            } else if key.starts_with("role:") {
                roles += n;
            } else if key.starts_with("side:") {
                sides += n;
            }
        }
    }
    assert_eq!((seats, roles, sides), (16, 16, 16));
    assert!(summary["planned_distribution"].is_object());

    // Ratings reload keyed by (model_id, role), with the side diagnostic
    // re-aggregated from the seat rows.
    let table = store.load_ratings(&run_id).await.unwrap();
    assert!(table
        .entities
        .keys()
        .any(|(_, role)| *role == Role::Spymaster));
    assert!(!table.sides.is_empty());
    assert_eq!(leaderboard(&table, 2.0).len(), rows.len());

    // Replay path: the stored record deserializes and its key reconstructs.
    let games = store.list_games(&run_id).await.unwrap();
    assert_eq!(games.len(), 4);
    assert!(games
        .iter()
        .all(|g| g.schedule_label.starts_with("controlled/")));
    let record = store.get_game(&games[0].game_id).await.unwrap().unwrap();
    assert_eq!(record.seats.len(), SEAT_COUNT as usize);
    assert!(key_card_for(&record, &Wordlist::default_embedded()).is_some());

    let runs = store.list_runs().await.unwrap();
    let run = runs.iter().find(|r| r.run_id == run_id).unwrap();
    assert_eq!(run.game, "codenames", "the frontend picker's discriminator");
    assert_eq!(run.status, "complete");
    assert_eq!(run.games_played, 4);

    let metrics = store.model_metric_summary(&run_id).await.unwrap();
    assert!(!metrics.is_empty());
    assert!(metrics.iter().all(|m| m.guess_hits <= m.guesses));
}
