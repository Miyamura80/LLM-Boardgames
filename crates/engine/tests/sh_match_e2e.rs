//! End-to-end harness tests: baseline bot games through the full game loop
//! (the CI smoke test), mirrored-seed schedules, and — when DATABASE_URL is
//! set — the Postgres store + match runner round trip.

use engine::secret_hitler::metrics::score_game;
use engine::secret_hitler::rating::{leaderboard, RatingTable};
use engine::secret_hitler::runner::record::GameRecord;
use engine::secret_hitler::runner::{
    advance_match, controlled_schedule, finalize_match, run_game, AgentFactory, AgentSpec,
    GameConfig, MatchSpec,
};
use engine::secret_hitler::store::Store;

fn bot_factory() -> AgentFactory {
    AgentFactory {
        keys: Default::default(),
        retry: Default::default(),
        temperature: 0.5,
        max_tokens: 512,
        utterance_char_cap: 240,
    }
}

fn bot_pool() -> Vec<AgentSpec> {
    vec![
        AgentSpec::bot("random-legal"),
        AgentSpec::bot("heuristic"),
        AgentSpec::bot("bayes-history"),
        AgentSpec::bot("random-legal"),
        AgentSpec::bot("heuristic"),
        AgentSpec::bot("bayes-history"),
    ]
}

async fn play_bot_game(seed: u64) -> GameRecord {
    let factory = bot_factory();
    let mut agents: Vec<_> = (0..7)
        .map(|i| {
            let kind = ["random-legal", "heuristic", "bayes-history"][i % 3];
            factory
                .build(&AgentSpec::bot(kind), seed ^ i as u64)
                .unwrap()
        })
        .collect();
    let cfg = GameConfig {
        game_id: format!("smoke-{seed}"),
        seed,
        discussion_rounds: 3, // bots don't speak; must be skipped cheaply
        ..Default::default()
    };
    run_game(&cfg, &mut agents, &[false; 7]).await
}

/// The PRD's CI floor: an all-baseline 7-player game runs headlessly and
/// deterministically from a seed and reaches a terminal state fast.
#[tokio::test]
async fn baseline_bot_game_completes_deterministically() {
    let a = play_bot_game(11).await;
    let b = play_bot_game(11).await;
    assert_eq!(a.winner, b.winner);
    assert_eq!(
        serde_json::to_string(&a.events).unwrap(),
        serde_json::to_string(&b.events).unwrap(),
        "same seed + same bots must reproduce the transcript bit-for-bit"
    );
    assert!(a.rounds > 0);
    assert_eq!(a.seats.len(), 7);
    // Bots are always legal: reliability counters stay zero.
    for s in &a.seats {
        assert_eq!(s.reliability.malformed_outputs, 0);
        assert_eq!(s.reliability.forced_defaults, 0);
    }
    // Metrics compute without panicking and respect their denominators.
    for m in score_game(&a) {
        assert!(m.goal_aligned_num <= m.goal_aligned_den);
        assert!(m.throughput_num <= m.throughput_den);
        assert!(m.exec_hits <= m.exec_shots);
    }
}

/// Mirrored seeds: the schedule is a pure function of (match_seed, cell) —
/// two different candidates see identical seeds, decks, and anchor layouts.
#[test]
fn controlled_schedule_is_candidate_independent_and_balanced() {
    let pool = bot_pool();
    let plans_a = controlled_schedule(99, &AgentSpec::llm("gemini/model-a"), &pool, 2).unwrap();
    let plans_b = controlled_schedule(99, &AgentSpec::llm("gemini/model-b"), &pool, 2).unwrap();
    assert_eq!(plans_a.len(), 21 * 2);
    for (a, b) in plans_a.iter().zip(&plans_b) {
        assert_eq!(a.seed, b.seed, "mirrored seeds must ignore the candidate");
        assert_eq!(a.roles, b.roles);
        assert_eq!(a.label, b.label);
    }
    // The candidate occupies every seat × role cell exactly K times.
    let mut cells = std::collections::BTreeMap::new();
    for plan in &plans_a {
        let seat = plan
            .seats
            .iter()
            .position(|s| !s.is_anchor)
            .expect("candidate seat");
        let role = plan.roles.as_ref().unwrap()[seat];
        *cells.entry((seat, format!("{role:?}"))).or_insert(0u32) += 1;
    }
    assert_eq!(cells.len(), 21, "3 roles × 7 seats");
    assert!(
        cells.values().all(|&c| c == 2),
        "every cell exactly K times"
    );
}

/// Rating sanity: a stacked table (strong bot faction) moves ratings apart and
/// duplicate entities average instead of exploding.
#[tokio::test]
async fn ratings_update_from_bot_games() {
    let mut table = RatingTable::default();
    for seed in 0..20 {
        let record = play_bot_game(seed).await;
        table.update(&record);
    }
    let rows = leaderboard(&table, 2.0);
    assert!(!rows.is_empty());
    for row in &rows {
        assert!(row.total_games > 0);
        // Weng-Lin must stay finite and sane.
        assert!(row.overall_mu.is_finite());
        assert!(row.overall_conservative <= row.overall_mu);
    }
}

/// Full store round trip: schedule → play → persist → resume no-op → finalize.
/// Skips (passes vacuously) when no DATABASE_URL is configured.
#[tokio::test]
async fn match_runner_persists_and_resumes_with_postgres() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL not set");
        return;
    };
    // Environment-dependent: skip (don't fail) when the database is
    // unreachable from this machine.
    let store = match tokio::time::timeout(std::time::Duration::from_secs(10), Store::connect(&url))
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
    let run_id = format!("test-run-{}", std::process::id());

    let spec = MatchSpec::Controlled {
        candidate: AgentSpec::bot("heuristic"),
        pool_name: "test-pool".into(),
        pool: bot_pool(),
        k: 1,
        match_seed: 7,
    };
    store
        .create_run(&run_id, spec.mode(), &serde_json::to_value(&spec).unwrap())
        .await
        .unwrap();

    let cfg = GameConfig {
        discussion_rounds: 0,
        belief_checkpoints: true,
        ..Default::default()
    };
    let factory = bot_factory();
    let progress = advance_match(&store, &run_id, &spec, &factory, &cfg, 1000)
        .await
        .unwrap();
    assert_eq!(progress.played, 21, "k=1 → 21 games");
    assert_eq!(progress.remaining, 0);

    // Resume is a no-op: everything already stored.
    let progress2 = advance_match(&store, &run_id, &spec, &factory, &cfg, 1000)
        .await
        .unwrap();
    assert_eq!(progress2.played, 0);

    let (rows, summary) = finalize_match(&store, &run_id, 2.0).await.unwrap();
    assert!(!rows.is_empty());
    assert_eq!(summary["games"], 21);

    // Replay path: stored record deserializes back into a GameRecord.
    let games = store.list_games(&run_id).await.unwrap();
    assert_eq!(games.len(), 21);
    let record = store.get_game(&games[0].game_id).await.unwrap().unwrap();
    assert_eq!(record.player_count, 7);
    assert!(!record.beliefs.is_empty(), "bot belief snapshots persisted");
}

/// The game-end belief snapshot must not see the terminal role reveal —
/// otherwise the final suspicion metric would score reading the answer key.
#[tokio::test]
async fn final_belief_elicitation_never_sees_the_role_reveal() {
    use engine::secret_hitler::actions::DecisionPoint;
    use engine::secret_hitler::agents::{
        AgentError, AgentReply, BeliefReport, RoleProbs, SeatAgent,
    };
    use engine::secret_hitler::events::GameEvent;
    use engine::secret_hitler::observation::Observation;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    struct ProbeBot {
        inner: engine::secret_hitler::agents::HeuristicBot,
        leaks: Arc<AtomicU32>,
        elicitations: Arc<AtomicU32>,
    }

    #[async_trait::async_trait]
    impl SeatAgent for ProbeBot {
        fn kind(&self) -> &'static str {
            "bot:probe"
        }
        fn model_id(&self) -> String {
            "bot:probe".into()
        }
        fn scaffold_version(&self) -> String {
            "test".into()
        }
        async fn decide(
            &mut self,
            obs: &Observation,
            decision: &DecisionPoint,
            feedback: Option<&str>,
        ) -> Result<AgentReply, AgentError> {
            self.inner.decide(obs, decision, feedback).await
        }
        async fn beliefs(&mut self, obs: &Observation) -> Result<Option<BeliefReport>, AgentError> {
            self.elicitations.fetch_add(1, Ordering::Relaxed);
            if obs
                .history
                .iter()
                .any(|r| matches!(r.event, GameEvent::GameEnded { .. }))
            {
                self.leaks.fetch_add(1, Ordering::Relaxed);
            }
            Ok(Some(BeliefReport {
                assessments: std::iter::once((
                    (obs.seat + 1) % 7,
                    RoleProbs {
                        liberal: 1.0,
                        fascist: 0.0,
                        hitler: 0.0,
                    },
                ))
                .collect(),
            }))
        }
    }

    let leaks = Arc::new(AtomicU32::new(0));
    let elicitations = Arc::new(AtomicU32::new(0));
    let mut agents: Vec<Box<dyn SeatAgent>> = (0..7)
        .map(|_| {
            Box::new(ProbeBot {
                inner: engine::secret_hitler::agents::HeuristicBot::new(),
                leaks: leaks.clone(),
                elicitations: elicitations.clone(),
            }) as Box<dyn SeatAgent>
        })
        .collect();
    let cfg = GameConfig {
        seed: 5,
        discussion_rounds: 0,
        ..Default::default()
    };
    let record = run_game(&cfg, &mut agents, &[false; 7]).await;

    assert!(elicitations.load(Ordering::Relaxed) > 0);
    assert_eq!(
        leaks.load(Ordering::Relaxed),
        0,
        "belief elicitation observed the terminal role reveal"
    );
    // The final snapshot itself exists.
    assert!(record.beliefs.iter().any(|b| b.checkpoint == u8::MAX));
}
