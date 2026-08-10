//! Property tests: randomized seeded games driven to termination, asserting
//! the invariants every downstream metric depends on — resource conservation,
//! VP bookkeeping, hidden-information filtering, and termination.

use engine::catan::actions::{Action, DecisionPoint};
use engine::catan::state::GameState;
use engine::catan::types::*;
use engine::game_core::Visibility;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// A chaotic but legality-seeking driver: explores builds, devs, bank trades,
/// and player trades; anything rejected falls back to the forced default.
fn random_action(state: &mut GameState, d: &DecisionPoint, rng: &mut ChaCha8Rng) -> Action {
    match d {
        DecisionPoint::TurnAction { seat } => {
            let seat = *seat;
            match rng.gen_range(0..10u8) {
                0 | 1 => Action::EndTurn,
                2 => {
                    let sites = state.road_sites(seat);
                    if sites.is_empty() {
                        Action::EndTurn
                    } else {
                        Action::BuildRoad {
                            edge: sites[rng.gen_range(0..sites.len())],
                        }
                    }
                }
                3 => {
                    let sites = state.settlement_sites(seat);
                    if sites.is_empty() {
                        Action::EndTurn
                    } else {
                        Action::BuildSettlement {
                            vertex: sites[rng.gen_range(0..sites.len())],
                        }
                    }
                }
                4 => match state.players[seat as usize].settlements.first() {
                    Some(&v) => Action::BuildCity { vertex: v },
                    None => Action::EndTurn,
                },
                5 => Action::BuyDev,
                6 => {
                    let give = RESOURCES[rng.gen_range(0..5)];
                    let receive = RESOURCES[rng.gen_range(0..5)];
                    Action::BankTrade { give, receive }
                }
                7 => {
                    // Offer 1 held resource for 1 different one, to a random
                    // addressee (or everyone).
                    let hand = state.players[seat as usize].resources;
                    let held: Vec<Resource> =
                        RESOURCES.into_iter().filter(|&r| hand.get(r) > 0).collect();
                    if held.is_empty() {
                        return Action::EndTurn;
                    }
                    let give = held[rng.gen_range(0..held.len())];
                    let want = RESOURCES[rng.gen_range(0..5)];
                    if want == give {
                        return Action::EndTurn;
                    }
                    let to = match rng.gen_range(0..4u8) {
                        0 => None,
                        n => Some((seat + n) % PLAYER_COUNT),
                    };
                    Action::ProposeTrade {
                        give: ResourceSet::of(give, 1),
                        receive: ResourceSet::of(want, 1),
                        to,
                        message: Some("deal?".into()),
                    }
                }
                8 => match state.players[seat as usize].devs_playable.first() {
                    Some(DevCard::Knight) => Action::PlayKnight,
                    Some(DevCard::RoadBuilding) => Action::PlayRoadBuilding,
                    Some(DevCard::YearOfPlenty) => Action::PlayYearOfPlenty {
                        first: RESOURCES[rng.gen_range(0..5)],
                        second: RESOURCES[rng.gen_range(0..5)],
                    },
                    Some(DevCard::Monopoly) => Action::PlayMonopoly {
                        resource: RESOURCES[rng.gen_range(0..5)],
                    },
                    _ => Action::EndTurn,
                },
                _ => Action::Say {
                    message: "hmm".into(),
                },
            }
        }
        DecisionPoint::RespondTrade { offer, .. } => match rng.gen_range(0..3u8) {
            0 => Action::AcceptTrade { message: None },
            1 => Action::RejectTrade { message: None },
            _ => Action::CounterTrade {
                give: offer.receive,
                receive: offer.give,
                message: Some("counter".into()),
            },
        },
        DecisionPoint::ResolveTrade {
            accepters,
            counters,
            ..
        } => {
            let mut options: Vec<Option<Seat>> = vec![None];
            options.extend(accepters.iter().map(|&s| Some(s)));
            options.extend(counters.iter().map(|&(s, _)| Some(s)));
            Action::ResolveTrade {
                partner: options[rng.gen_range(0..options.len())],
            }
        }
        _ => state.forced_default(d),
    }
}

fn drive(seed: u64) -> GameState {
    let mut state = GameState::new(seed);
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x0DD5);
    let mut steps = 0u32;
    while !state.is_over() {
        steps += 1;
        assert!(steps < 200_000, "game failed to terminate (seed {seed})");
        let decisions = state.pending_decisions();
        assert!(!decisions.is_empty(), "no pending decision in a live game");
        let d = decisions[rng.gen_range(0..decisions.len())].clone();
        let seat = d.seat();
        let action = random_action(&mut state, &d, &mut rng);
        if state.apply(seat, action).is_err() {
            let fallback = state.forced_default(&d);
            state
                .apply(seat, fallback)
                .expect("forced default must be legal");
        }
        check_conservation(&state, seed);
    }
    state
}

/// Cards never leave the (players + bank) system.
fn check_conservation(state: &GameState, seed: u64) {
    for r in RESOURCES {
        let held: u16 = state
            .players
            .iter()
            .map(|p| p.resources.get(r) as u16)
            .sum::<u16>()
            + state.bank.get(r) as u16;
        assert_eq!(
            held,
            BANK_PER_RESOURCE as u16,
            "{} conservation broken (seed {seed})",
            r.as_str()
        );
    }
}

#[test]
fn randomized_games_terminate_with_intact_invariants() {
    for seed in 0..12u64 {
        let state = drive(seed);
        let winner = state.winner.expect("game over");

        // Winner has the highest total VP (ties only via the turn-cap path,
        // where the earlier seat wins).
        let vps: Vec<u8> = (0..PLAYER_COUNT)
            .map(|s| state.victory_points(s, true))
            .collect();
        let best = *vps.iter().max().expect("four seats");
        assert_eq!(vps[winner as usize], best, "seed {seed}");

        // Piece bookkeeping matches the board arrays.
        for s in 0..PLAYER_COUNT as usize {
            let roads_on_board = state
                .roads
                .iter()
                .filter(|&&r| r == Some(s as Seat))
                .count();
            assert_eq!(roads_on_board, state.players[s].roads_placed as usize);
            let buildings_on_board = state
                .buildings
                .iter()
                .filter(|b| matches!(b, Some((o, _)) if *o == s as Seat))
                .count();
            assert_eq!(
                buildings_on_board,
                state.players[s].settlements.len() + state.players[s].cities.len()
            );
        }
    }
}

#[test]
fn observations_never_leak_private_information() {
    let state = drive(3);
    for seat in 0..PLAYER_COUNT {
        let obs = state.observe(seat);
        for record in &obs.history {
            assert!(
                record.visibility.visible_to(seat),
                "seat {seat} observed an event it was not entitled to"
            );
            if let Visibility::Private(s) = record.visibility {
                assert_eq!(s, seat);
            }
        }
        // Opponent hands appear only as counts.
        for p in &obs.players {
            if p.seat != seat {
                assert_eq!(
                    p.hand_count,
                    state.players[p.seat as usize].resources.total()
                );
            }
        }
    }
}

#[test]
fn same_seed_same_driver_reproduces_the_transcript() {
    let a = drive(9);
    let b = drive(9);
    assert_eq!(a.events.len(), b.events.len());
    let ja = serde_json::to_string(&a.events).expect("events serialize");
    let jb = serde_json::to_string(&b.events).expect("events serialize");
    assert_eq!(ja, jb, "identical seed + decisions must replay bit-for-bit");
}
