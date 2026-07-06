//! Runner + baseline tests (US-005): a full all-baseline 7-player game runs
//! headlessly, deterministically, and terminates quickly.

use crate::eval::{run_game, Agent, BaselineAgent, RunnerConfig};
use crate::game::{GameConfig, GameState};

fn baseline_table() -> Vec<Box<dyn Agent>> {
    (0..7)
        .map(|_| Box::new(BaselineAgent::new()) as Box<dyn Agent>)
        .collect()
}

fn baseline_cfg() -> RunnerConfig {
    RunnerConfig {
        max_attempts: 3,
        discussion_rounds: 0, // baselines are silent
        elicit_beliefs: false,
    }
}

#[tokio::test]
async fn all_baseline_game_completes() {
    let game = GameState::new(GameConfig::new(1));
    let rec = run_game(game, &baseline_table(), &baseline_cfg()).await;
    // Terminal state reached with a winner and a reason.
    assert!(matches!(
        rec.win_reason,
        crate::game::WinReason::LiberalPolicies
            | crate::game::WinReason::FascistPolicies
            | crate::game::WinReason::HitlerExecuted
            | crate::game::WinReason::HitlerChancellor
    ));
    // Baselines never emit malformed/illegal/forced-default events.
    for r in rec.reliability.values() {
        assert_eq!(r.malformed_outputs, 0);
        assert_eq!(r.illegal_moves, 0);
        assert_eq!(r.forced_defaults, 0);
    }
    assert_eq!(rec.usage.calls, 0, "baselines make no LLM calls");
}

#[tokio::test]
async fn all_baseline_game_is_deterministic() {
    let a = run_game(
        GameState::new(GameConfig::new(7)),
        &baseline_table(),
        &baseline_cfg(),
    )
    .await;
    let b = run_game(
        GameState::new(GameConfig::new(7)),
        &baseline_table(),
        &baseline_cfg(),
    )
    .await;
    let ja = serde_json::to_string(&a.log).unwrap();
    let jb = serde_json::to_string(&b.log).unwrap();
    assert_eq!(ja, jb, "same seed → identical baseline transcript");
    assert_eq!(a.winner, b.winner);
}

#[tokio::test]
async fn baseline_games_terminate_across_many_seeds() {
    for seed in 0..50u64 {
        let rec = run_game(
            GameState::new(GameConfig::new(seed)),
            &baseline_table(),
            &baseline_cfg(),
        )
        .await;
        // A record always carries all seven seat assignments.
        assert_eq!(rec.seats.len(), 7);
    }
}

// ---- match runner + rating + metrics --------------------------------------

use crate::eval::{run_match, MatchConfig};

fn distinct_roster() -> Vec<String> {
    (0..7).map(|i| format!("baseline-{i}")).collect()
}

#[tokio::test]
async fn match_runner_produces_rating_metrics_and_distribution() {
    let cfg = MatchConfig {
        games: 14,
        base_seed: 100,
        roster: distinct_roster(),
        runner: RunnerConfig {
            max_attempts: 3,
            discussion_rounds: 0,
            elicit_beliefs: false,
        },
    };
    let outcome = run_match(&cfg, |m| Box::new(BaselineAgent::named(m))).await;

    assert_eq!(outcome.records.len(), 14);
    // Every game seats 7 distinct models (no model on both teams).
    for rec in &outcome.records {
        let names: std::collections::BTreeSet<_> = rec.seats.iter().map(|s| &s.agent).collect();
        assert_eq!(names.len(), 7, "a model appeared twice in one game");
    }
    // Leaderboard has every roster model with μ/σ per role.
    assert_eq!(outcome.leaderboard.models.len(), 7);
    for m in &outcome.leaderboard.models {
        assert!(m.total_games > 0);
        assert!(m.roles.iter().all(|r| r.sigma > 0.0));
    }
    // Metrics computed (goal-aligned enactment present for players who chose).
    assert!(outcome
        .metrics
        .iter()
        .any(|m| m.goal_aligned_enactment.is_some()));
    // Fairness distribution logged.
    assert_eq!(outcome.distribution.by_model.len(), 7);
}

#[test]
#[should_panic(expected = "distinct models")]
fn match_rejects_roster_that_would_duplicate_a_model() {
    let cfg = MatchConfig {
        games: 1,
        base_seed: 1,
        roster: vec!["a".into(), "b".into(), "c".into()], // < 7 distinct
        runner: RunnerConfig::default(),
    };
    let _ = crate::eval::plan_match(&cfg);
}

#[tokio::test]
async fn baseline_goal_aligned_enactment_is_high() {
    // Baselines always keep/enact their own faction's tile when possible, so
    // goal-aligned enactment should be perfect for seats that made choices.
    let cfg = MatchConfig {
        games: 8,
        base_seed: 42,
        roster: distinct_roster(),
        runner: RunnerConfig {
            max_attempts: 3,
            discussion_rounds: 0,
            elicit_beliefs: false,
        },
    };
    let outcome = run_match(&cfg, |m| Box::new(BaselineAgent::named(m))).await;
    for m in &outcome.metrics {
        if let Some(rate) = m.goal_aligned_enactment {
            assert!(rate > 0.99, "baseline should be goal-aligned, got {rate}");
        }
    }
}
