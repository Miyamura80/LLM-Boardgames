//! End-to-end bot games through the real runner: deterministic, headless, and
//! fast — the CI smoke test for the whole Catan stack below the LLM layer.

use engine::catan::runner::{run_game, AgentFactory, AgentSpec, CatanAgentKind, GameConfig};
use engine::catan::types::PLAYER_COUNT;
use engine::llm::{ProviderKeys, RetryPolicy};

fn factory() -> AgentFactory {
    AgentFactory {
        keys: ProviderKeys::default(),
        retry: RetryPolicy::default(),
        temperature: 0.5,
        max_tokens: 256,
        message_char_cap: 240,
    }
}

async fn play(seed: u64, kinds: [CatanAgentKind; 4]) -> engine::catan::runner::GameRecord {
    let f = factory();
    let mut agents = Vec::new();
    for (seat, kind) in kinds.into_iter().enumerate() {
        let spec = AgentSpec::bot(kind);
        agents.push(
            f.build(&spec, seed ^ (seat as u64) << 8)
                .expect("bot builds"),
        );
    }
    let cfg = GameConfig {
        game_id: format!("e2e-{seed}"),
        seed,
        ..GameConfig::default()
    };
    run_game(&cfg, &mut agents, &[false, true, true, true]).await
}

#[tokio::test]
async fn greedy_bot_game_terminates_with_consistent_record() {
    let record = play(
        11,
        [
            CatanAgentKind::Greedy,
            CatanAgentKind::Greedy,
            CatanAgentKind::Greedy,
            CatanAgentKind::Greedy,
        ],
    )
    .await;
    assert_eq!(record.player_count, PLAYER_COUNT);
    assert_eq!(record.seats.len(), 4);
    assert_eq!(record.final_vps.len(), 4);
    // The winner carries placement 1 and the win flag.
    let w = &record.seats[record.winner as usize];
    assert_eq!(w.placement, 1);
    assert!(w.won);
    // Bots are perfectly reliable: no rethinks, no forced defaults besides
    // possible turn caps (which surface as events, not seat counters).
    for seat in &record.seats {
        assert_eq!(seat.reliability.malformed_outputs, 0);
        assert_eq!(
            seat.reliability.illegal_moves, 0,
            "greedy bot made an illegal move"
        );
        assert_eq!(seat.usage.prompt_tokens, 0);
    }
    assert!(!record.seats[0].is_anchor);
    assert!(record.seats[1].is_anchor);
}

#[tokio::test]
async fn mixed_bot_games_are_deterministic_per_seed() {
    let kinds = [
        CatanAgentKind::RandomLegal,
        CatanAgentKind::Greedy,
        CatanAgentKind::RandomLegal,
        CatanAgentKind::Greedy,
    ];
    let a = play(99, kinds).await;
    let b = play(99, kinds).await;
    assert_eq!(
        serde_json::to_string(&a.events).expect("serialize"),
        serde_json::to_string(&b.events).expect("serialize"),
        "same seed + same bots must replay bit-for-bit"
    );
    let c = play(100, kinds).await;
    assert_ne!(
        serde_json::to_string(&a.events).expect("serialize"),
        serde_json::to_string(&c.events).expect("serialize"),
        "different seeds should diverge"
    );
}
