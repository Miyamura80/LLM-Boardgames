//! Randomized full-game property tests: invariants hold every step, games
//! terminate, and identical seeds reproduce identical transcripts.

use crate::game::*;
use std::collections::BTreeMap;

// ===========================================================================
// Property / invariant tests over randomized full games
// ===========================================================================

fn random_legal_action(d: &Decision, rng: &mut Rng) -> Action {
    match &d.kind {
        DecisionKind::Nominate { eligible, .. } => {
            Action::Nominate(eligible[rng.below(eligible.len())])
        }
        DecisionKind::Vote { voters, .. } => Action::CastVotes(
            voters
                .iter()
                .map(|&s| (s, rng.below(2) == 0))
                .collect::<BTreeMap<_, _>>(),
        ),
        DecisionKind::PresidentDiscard { drawn, .. } => Action::Discard(rng.below(drawn.len())),
        DecisionKind::ChancellorEnact { hand, .. } => Action::Enact(rng.below(hand.len())),
        DecisionKind::VetoConsent { .. } => Action::VetoConsent(rng.below(2) == 0),
        DecisionKind::Investigate { eligible, .. } => {
            Action::Investigate(eligible[rng.below(eligible.len())])
        }
        DecisionKind::SpecialElection { eligible, .. } => {
            Action::SpecialElection(eligible[rng.below(eligible.len())])
        }
        DecisionKind::Execution { eligible, .. } => {
            Action::Execute(eligible[rng.below(eligible.len())])
        }
    }
}

fn in_hand(d: &Decision) -> usize {
    match d.kind {
        DecisionKind::PresidentDiscard { .. } => 3,
        DecisionKind::ChancellorEnact { .. } | DecisionKind::VetoConsent { .. } => 2,
        _ => 0,
    }
}

fn assert_invariants(g: &GameState) {
    // Tile conservation: draw + discard + enacted + in-hand == 17.
    let enacted = g.board().liberal as usize + g.board().fascist as usize;
    let hand = g.current_decision().as_ref().map(in_hand).unwrap_or(0);
    assert_eq!(
        g.deck_in_circulation() + enacted + hand,
        crate::game::policy::TOTAL_TILES,
        "tile conservation violated"
    );

    // Exactly one policy per enactment event.
    let enact_events = g
        .log()
        .public_events()
        .filter(|e| {
            matches!(
                e,
                Event::PolicyEnacted { .. } | Event::ChaosPolicyEnacted { .. }
            )
        })
        .count();
    assert_eq!(enact_events, enacted, "enactment events must match board");

    // No pending action for a dead seat; president alive while playing.
    if let Some(d) = g.current_decision() {
        if let Some(actor) = d.actor() {
            assert!(g.is_alive(actor), "decision offered to a dead seat");
        }
        if let DecisionKind::Vote { voters, .. } = &d.kind {
            assert!(voters.iter().all(|&s| g.is_alive(s)));
        }
        assert!(g.is_alive(g.president()), "president must be alive");
    }
}

fn play_random_game(seed: u64, driver_seed: u64) -> GameState {
    let mut g = GameState::new(GameConfig::new(seed));
    let mut rng = Rng::new(driver_seed);
    let mut steps = 0;
    while !g.is_over() {
        assert_invariants(&g);
        let d = g.current_decision().expect("decision while not over");
        let a = random_legal_action(&d, &mut rng);
        g.apply(a).expect("random legal action must be legal");
        steps += 1;
        assert!(steps < 5000, "game failed to terminate");
    }
    assert_invariants(&g);
    g
}

#[test]
fn randomized_games_hold_all_invariants_and_terminate() {
    for seed in 0..200u64 {
        let g = play_random_game(seed, seed.wrapping_mul(2654435761));
        assert!(g.is_over());
        assert!(g.winner().is_some());
    }
}

#[test]
fn same_seed_and_decisions_reproduce_identical_transcripts() {
    for seed in 0..25u64 {
        let driver = seed ^ 0xABCD;
        let a = play_random_game(seed, driver);
        let b = play_random_game(seed, driver);
        let ja = serde_json::to_string(a.log()).unwrap();
        let jb = serde_json::to_string(b.log()).unwrap();
        assert_eq!(ja, jb, "seed {seed} not reproducible");
    }
}
