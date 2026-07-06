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
