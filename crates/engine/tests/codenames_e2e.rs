//! End-to-end bot games through the real runner: deterministic, headless, and
//! sub-second — the CI smoke test for the whole Codenames stack below the LLM
//! layer (PRD-codenames-evals §8).

use engine::codenames::runner::{
    run_game, AgentFactory, AgentSpec, CodenamesAgentKind, GameConfig,
};
use engine::codenames::types::{EndReason, Role, Team, SEAT_COUNT};
use engine::llm::{ProviderKeys, RetryPolicy};

fn factory() -> AgentFactory {
    AgentFactory {
        keys: ProviderKeys::default(),
        retry: RetryPolicy::default(),
        temperature: 0.5,
        max_tokens: 256,
        vectors: None,
    }
}

async fn play(seed: u64) -> engine::codenames::runner::GameRecord {
    let f = factory();
    let mut agents = Vec::new();
    for seat in 0..SEAT_COUNT as u64 {
        let spec = AgentSpec::bot(CodenamesAgentKind::Random);
        agents.push(f.build(&spec, seed ^ seat << 8).expect("bot builds"));
    }
    let cfg = GameConfig {
        game_id: format!("e2e-{seed}"),
        seed,
        ..GameConfig::default()
    };
    run_game(&cfg, &mut agents, &[false, true, true, true]).await
}

#[tokio::test]
async fn a_four_bot_game_terminates_with_a_consistent_record() {
    let record = play(11).await;

    assert_eq!(record.seats.len(), SEAT_COUNT as usize);
    assert_eq!(record.rules_version, engine::codenames::RULES_VERSION);
    assert_eq!(record.wordlist_hash.len(), 64);
    assert_eq!(record.schedule_label, "adhoc");
    assert!(record.turns >= 1);

    // Seats carry their fixed roles, and exactly the winning team won.
    let roles: Vec<Role> = record.seats.iter().map(|s| s.role).collect();
    assert_eq!(
        roles,
        vec![
            Role::Spymaster,
            Role::Operative,
            Role::Spymaster,
            Role::Operative
        ]
    );
    for seat in &record.seats {
        assert_eq!(seat.won, seat.team == record.winner);
        assert_eq!(seat.agent_kind, "bot:codenames-random");
        assert_eq!(seat.model_id, "bot:codenames-random");
        // Bots are perfectly reliable: legal by construction, zero tokens.
        assert_eq!(seat.reliability.malformed_outputs, 0);
        assert_eq!(seat.reliability.illegal_moves, 0);
        assert_eq!(seat.reliability.forced_defaults, 0);
        assert!(seat.thoughts.is_empty());
        assert_eq!(seat.usage.prompt_tokens, 0);
    }
    assert_eq!(record.team_seats(Team::A).len(), 2);
    assert!(record.seats[1].is_anchor && !record.seats[0].is_anchor);

    // The transcript ends where the record says it does.
    let ended = record
        .events
        .iter()
        .filter(|e| {
            matches!(
                e.event,
                engine::codenames::events::CodenamesEvent::GameEnded { .. }
            )
        })
        .count();
    assert_eq!(ended, 1, "exactly one game-ending event");
    assert!(matches!(
        record.end_reason,
        EndReason::AgentsFound | EndReason::Assassin
    ));
}

#[tokio::test]
async fn seeded_bot_games_replay_identically_and_differ_across_seeds() {
    let a = play(5).await;
    let b = play(5).await;
    let other = play(6).await;

    let transcript = |r: &engine::codenames::runner::GameRecord| {
        serde_json::to_string(&r.events).expect("events serialize")
    };
    assert_eq!(transcript(&a), transcript(&b));
    assert_eq!(a.winner, b.winner);
    assert_ne!(transcript(&a), transcript(&other));
}

/// A batch of seeds: every game terminates, and every reveal is accounted for.
#[tokio::test]
async fn a_batch_of_seeded_games_all_terminate() {
    for seed in 0..8u64 {
        let record = play(seed).await;
        let reveals = record
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.event,
                    engine::codenames::events::CodenamesEvent::GuessRevealed { .. }
                )
            })
            .count();
        assert!(
            (1..=25).contains(&reveals),
            "seed {seed} revealed {reveals} cards"
        );
        assert!(record.turns as usize <= reveals + 1);
    }
}
