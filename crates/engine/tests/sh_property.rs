//! Property tests: randomized legal games must uphold engine invariants.

use engine::secret_hitler::actions::{Action, DecisionPoint};
use engine::secret_hitler::events::GameEvent;
use engine::secret_hitler::state::GameState;
use engine::secret_hitler::types::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Pick a uniformly random **legal** action for a decision.
fn random_legal(rng: &mut StdRng, d: &DecisionPoint) -> Action {
    match d {
        DecisionPoint::Nominate { eligible, .. } => Action::Nominate {
            target: eligible[rng.gen_range(0..eligible.len())],
        },
        DecisionPoint::Vote { .. } => Action::Vote {
            ja: rng.gen_bool(0.6),
        },
        DecisionPoint::Discard { tiles, .. } => Action::Discard {
            policy: tiles[rng.gen_range(0..tiles.len())],
        },
        DecisionPoint::Enact {
            tiles, can_veto, ..
        } => {
            if *can_veto && rng.gen_bool(0.3) {
                Action::ProposeVeto
            } else {
                Action::Enact {
                    policy: tiles[rng.gen_range(0..tiles.len())],
                }
            }
        }
        DecisionPoint::VetoConsent { .. } => Action::VetoConsent {
            approve: rng.gen_bool(0.5),
        },
        DecisionPoint::UsePower { targets, .. } => Action::UsePower {
            target: targets[rng.gen_range(0..targets.len())],
        },
    }
}

#[test]
fn randomized_games_uphold_invariants() {
    for seed in 0..300u64 {
        let mut state = GameState::new(seed);
        let mut rng = StdRng::seed_from_u64(seed ^ 0xC0FFEE);
        let mut steps = 0;

        while !state.is_over() {
            let decisions = state.pending_decisions();
            assert!(
                !decisions.is_empty(),
                "seed {seed}: live game with no pending decision"
            );

            for d in &decisions {
                // No action is ever offered to a dead player.
                assert!(
                    state.is_alive(d.seat()),
                    "seed {seed}: decision offered to dead seat {}",
                    d.seat()
                );
            }

            // Apply one action (votes queue several decisions; take the first).
            let d = &decisions[0];
            let action = random_legal(&mut rng, d);
            state
                .apply(d.seat(), action)
                .unwrap_or_else(|e| panic!("seed {seed}: legal action rejected: {e}"));

            // Policy tiles are conserved: deck + discard + enacted + any tiles
            // currently held in a legislative hand == 17.
            let in_hand = match &state.phase {
                engine::secret_hitler::state::Phase::LegislativePresident { tiles } => tiles.len(),
                engine::secret_hitler::state::Phase::LegislativeChancellor { tiles, .. } => {
                    tiles.len()
                }
                engine::secret_hitler::state::Phase::VetoConsent { tiles } => tiles.len(),
                _ => 0,
            };
            let on_board = (state.liberal_policies + state.fascist_policies) as usize;
            assert_eq!(
                state.deck.total() + on_board + in_hand,
                17,
                "seed {seed}: tile conservation violated"
            );

            steps += 1;
            assert!(steps < 20_000, "seed {seed}: game did not terminate");
        }

        // Terminal state is a real win condition and consistent with the board.
        let win = state.winner.expect("game over implies winner");
        match win {
            WinCondition::LiberalPolicies => assert_eq!(state.liberal_policies, 5),
            WinCondition::FascistPolicies => assert_eq!(state.fascist_policies, 6),
            WinCondition::HitlerExecuted => {
                assert!(!state.is_alive(state.hitler_seat()))
            }
            WinCondition::HitlerChancellor => {
                assert!(state.fascist_policies >= 3);
                assert_eq!(state.gov_chancellor, Some(state.hitler_seat()));
            }
        }
        assert!(state.pending_decisions().is_empty());

        // Exactly one policy is enacted per successful government: every
        // PolicyEnacted is preceded by a GovernmentFormed or TopDeckEnacted
        // with no other PolicyEnacted in between.
        let mut open_government = false;
        for rec in &state.events {
            match &rec.event {
                GameEvent::GovernmentFormed { .. } | GameEvent::TopDeckEnacted { .. } => {
                    open_government = true;
                }
                GameEvent::PolicyEnacted { .. } => {
                    assert!(
                        open_government,
                        "seed {seed}: policy enacted without a government/top-deck"
                    );
                    open_government = false;
                }
                _ => {}
            }
        }

        // The event count of enacted policies matches the board.
        let enacted = state
            .events
            .iter()
            .filter(|r| matches!(r.event, GameEvent::PolicyEnacted { .. }))
            .count();
        assert_eq!(
            enacted,
            (state.liberal_policies + state.fascist_policies) as usize,
            "seed {seed}"
        );
    }
}

#[test]
fn observations_never_leak_hidden_information_in_random_games() {
    for seed in 0..50u64 {
        let mut state = GameState::new(seed);
        let mut rng = StdRng::seed_from_u64(seed);
        while !state.is_over() {
            let decisions = state.pending_decisions();
            let d = &decisions[0];

            for seat in state.alive_seats() {
                let obs = state.observe(seat);
                for rec in &obs.history {
                    // Every record in a seat's history must be visible to it.
                    assert!(rec.visibility.visible_to(seat));
                    if let GameEvent::RolesDealt { seat: s, .. } = &rec.event {
                        assert_eq!(*s, seat, "seed {seed}: foreign role deal leaked");
                    }
                }
                // Hitler must never learn teammates from the observation.
                if obs.role == Role::Hitler {
                    assert!(obs.known_teammates.is_empty());
                }
            }

            let action = random_legal(&mut rng, d);
            state.apply(d.seat(), action).unwrap();
        }
    }
}
