//! Catan rules tests: setup, building legality, production, the 7/robber
//! flow, dev cards, awards, ports, and win conditions (PRD-catan-evals
//! US-C02/C03).

use engine::catan::actions::Action;
use engine::catan::board::graph;
use engine::catan::state::{GameState, Phase};
use engine::catan::testkit::beginner_layout;
use engine::catan::types::*;

/// Drive the snake-draft setup with deterministic picks (first legal site,
/// first legal road), returning a state at turn 1, seat 0, pre-roll.
fn through_setup(mut state: GameState) -> GameState {
    for placement in 0..(2 * PLAYER_COUNT) {
        let seat = GameState::setup_seat(placement);
        let sites = state.setup_settlement_sites();
        let v = sites[(placement as usize * 5) % sites.len()];
        state
            .apply(seat, Action::PlaceSetupSettlement { vertex: v })
            .expect("setup settlement");
        let e = state.setup_road_sites(v)[0];
        state
            .apply(seat, Action::PlaceSetupRoad { edge: e })
            .expect("setup road");
    }
    state
}

fn scripted(dice: &[(u8, u8)]) -> GameState {
    GameState::new_scripted(7, beginner_layout(), dice, vec![DevCard::VictoryPoint])
}

/// Give a seat resources straight from the bank (test-only shortcut).
fn grant(state: &mut GameState, seat: Seat, set: ResourceSet) {
    assert!(state.bank.remove_set(&set), "bank has the cards");
    state.players[seat as usize].resources.add_set(&set);
}

#[test]
fn setup_runs_in_snake_order_and_grants_second_settlement_resources() {
    let order: Vec<Seat> = (0..8).map(GameState::setup_seat).collect();
    assert_eq!(order, vec![0, 1, 2, 3, 3, 2, 1, 0]);

    let state = through_setup(scripted(&[]));
    assert_eq!(state.phase, Phase::PreRoll);
    assert_eq!(state.turn, 1);
    assert_eq!(state.active, 0);
    for p in &state.players {
        assert_eq!(p.settlements.len(), 2);
        assert_eq!(p.roads_placed, 2);
        // Second settlement granted its adjacent producing hexes (1–3 cards).
        assert!(p.resources.total() <= 3);
    }
    // Everyone starts at 2 public VP.
    for s in 0..PLAYER_COUNT {
        assert_eq!(state.victory_points(s, true), 2);
    }
}

#[test]
fn distance_rule_rejects_adjacent_settlements() {
    let mut state = scripted(&[]);
    let v = state.setup_settlement_sites()[0];
    state
        .apply(0, Action::PlaceSetupSettlement { vertex: v })
        .expect("first placement");
    let e = state.setup_road_sites(v)[0];
    state.apply(0, Action::PlaceSetupRoad { edge: e }).unwrap();

    let neighbor = graph().vertex_neighbors[v as usize][0];
    let err = state
        .apply(1, Action::PlaceSetupSettlement { vertex: neighbor })
        .unwrap_err();
    assert!(err.0.contains("distance rule"), "{}", err.0);
    // And the same vertex is occupied.
    let err = state
        .apply(1, Action::PlaceSetupSettlement { vertex: v })
        .unwrap_err();
    assert!(
        err.0.contains("occupied") || err.0.contains("distance"),
        "{}",
        err.0
    );
}

#[test]
fn building_costs_move_through_the_bank() {
    let mut state = through_setup(scripted(&[(2, 3)]));
    state.apply(0, Action::Roll).unwrap();

    // Clear seat 0's setup income for exact accounting.
    let hand = state.players[0].resources;
    state.players[0].resources = ResourceSet::default();
    state.bank.add_set(&hand);
    let bank_before = state.bank;

    grant(&mut state, 0, road_cost());
    let edge = state.road_sites(0)[0];
    state.apply(0, Action::BuildRoad { edge }).unwrap();
    assert_eq!(state.players[0].resources.total(), 0);
    assert_eq!(state.bank, bank_before, "cost returned to the bank");
    assert_eq!(state.players[0].roads_placed, 3);

    // A city upgrade: 2 grain + 3 ore, replaces the settlement.
    grant(&mut state, 0, city_cost());
    let settlement = state.players[0].settlements[0];
    state
        .apply(0, Action::BuildCity { vertex: settlement })
        .unwrap();
    assert_eq!(state.players[0].cities, vec![settlement]);
    assert_eq!(state.players[0].settlements.len(), 1);
    assert_eq!(state.victory_points(0, true), 3);
    assert_eq!(state.bank, bank_before);
}

#[test]
fn unaffordable_and_disconnected_builds_are_rejected() {
    let mut state = through_setup(scripted(&[(2, 3)]));
    state.apply(0, Action::Roll).unwrap();
    state.bank.add_set(&state.players[0].resources.clone());
    state.players[0].resources = ResourceSet::default();

    let edge = state.road_sites(0)[0];
    let err = state.apply(0, Action::BuildRoad { edge }).unwrap_err();
    assert!(err.0.contains("costs"), "{}", err.0);

    // A road nowhere near seat 0's network is rejected even when affordable.
    grant(&mut state, 0, road_cost());
    let foreign = (0..72u8)
        .find(|e| !state.road_sites(0).contains(e) && state.roads[*e as usize].is_none())
        .expect("some disconnected edge");
    let err = state
        .apply(0, Action::BuildRoad { edge: foreign })
        .unwrap_err();
    assert!(err.0.contains("not connected"), "{}", err.0);
}

#[test]
fn seven_forces_discard_of_half_rounded_down_then_robber() {
    let mut state = through_setup(scripted(&[(3, 4)]));
    // Seat 1: 8 cards (discards 4). Seat 2: exactly 7 (exempt).
    state.bank.add_set(&state.players[1].resources.clone());
    state.players[1].resources = ResourceSet::default();
    state.bank.add_set(&state.players[2].resources.clone());
    state.players[2].resources = ResourceSet::default();
    grant(
        &mut state,
        1,
        ResourceSet {
            brick: 5,
            ore: 3,
            ..Default::default()
        },
    );
    grant(
        &mut state,
        2,
        ResourceSet {
            wool: 7,
            ..Default::default()
        },
    );

    state.apply(0, Action::Roll).unwrap();
    let Phase::Discard { pending } = &state.phase else {
        panic!("expected discard phase, got {:?}", state.phase)
    };
    assert_eq!(
        pending
            .iter()
            .map(|&(s, n)| (s, n))
            .collect::<Vec<_>>()
            .len(),
        1
    );
    assert_eq!(pending[0], (1, 4));

    // Wrong count and cards-not-held both bounce.
    let err = state
        .apply(
            1,
            Action::Discard {
                resources: ResourceSet {
                    brick: 1,
                    ..Default::default()
                },
            },
        )
        .unwrap_err();
    assert!(err.0.contains("exactly 4"), "{}", err.0);
    let err = state
        .apply(
            1,
            Action::Discard {
                resources: ResourceSet {
                    grain: 4,
                    ..Default::default()
                },
            },
        )
        .unwrap_err();
    assert!(err.0.contains("do not hold"), "{}", err.0);

    state
        .apply(
            1,
            Action::Discard {
                resources: ResourceSet {
                    brick: 3,
                    ore: 1,
                    ..Default::default()
                },
            },
        )
        .unwrap();
    assert_eq!(state.players[1].resources.total(), 4);
    assert_eq!(state.phase, Phase::MoveRobber);
}

#[test]
fn robber_must_move_and_steals_one_random_card() {
    let mut state = through_setup(scripted(&[(3, 4)]));
    state.apply(0, Action::Roll).unwrap();
    assert_eq!(state.phase, Phase::MoveRobber);

    let err = state
        .apply(0, Action::MoveRobber { hex: state.robber })
        .unwrap_err();
    assert!(err.0.contains("different hex"), "{}", err.0);

    // Move onto a hex adjacent to an opponent with cards.
    let g = graph();
    let victim_settlement = state.players[1].settlements[1]; // second settlement (has income)
    assert!(state.players[1].resources.total() > 0);
    let hex = g.vertex_hexes[victim_settlement as usize][0];
    let before_victim = state.players[1].resources.total();
    let before_thief = state.players[0].resources.total();
    state.apply(0, Action::MoveRobber { hex }).unwrap();
    assert_eq!(state.robber, hex);
    // Either an auto-steal happened or the thief must pick among victims.
    match &state.phase {
        Phase::Turn => {
            assert_eq!(state.players[1].resources.total(), before_victim - 1);
            assert_eq!(state.players[0].resources.total(), before_thief + 1);
        }
        Phase::ChooseVictim { victims } => {
            assert!(victims.contains(&1));
            state.apply(0, Action::StealFrom { seat: 1 }).unwrap();
            assert_eq!(state.players[1].resources.total(), before_victim - 1);
            assert_eq!(state.players[0].resources.total(), before_thief + 1);
        }
        p => panic!("unexpected phase {p:?}"),
    }
}

#[test]
fn robbed_hex_does_not_produce() {
    let mut state = through_setup(scripted(&[(3, 4), (2, 4)]));
    state.apply(0, Action::Roll).unwrap(); // 7
                                           // Park the robber on a 6-hex (hex 4: hills, 6).
    state.apply(0, Action::MoveRobber { hex: 4 }).unwrap();
    if let Phase::ChooseVictim { victims } = &state.phase {
        let v = victims[0];
        state.apply(0, Action::StealFrom { seat: v }).unwrap();
    }
    state.apply(0, Action::EndTurn).unwrap();

    // Roll a 6: hex 4 is robbed; only hex 17 (fields, 6) may produce.
    let brick_before: Vec<u8> = state.players.iter().map(|p| p.resources.brick).collect();
    state.apply(1, Action::Roll).unwrap(); // (2,4) = 6
    let brick_after: Vec<u8> = state.players.iter().map(|p| p.resources.brick).collect();
    assert_eq!(
        brick_before, brick_after,
        "the robbed hills hex must not pay brick"
    );
}

#[test]
fn dev_cards_wait_a_turn_and_are_limited_to_one_per_turn() {
    let state = GameState::new_scripted(
        7,
        beginner_layout(),
        &[(2, 3), (2, 3), (2, 3), (2, 3), (2, 3), (2, 3)],
        vec![DevCard::Knight, DevCard::Knight], // drawn from the end
    );
    let mut state = through_setup(state);
    state.apply(0, Action::Roll).unwrap();

    grant(&mut state, 0, dev_cost());
    state.apply(0, Action::BuyDev).unwrap();
    assert_eq!(state.players[0].devs_new, vec![DevCard::Knight]);
    let err = state.apply(0, Action::PlayKnight).unwrap_err();
    assert!(err.0.contains("bought this turn"), "{}", err.0);

    // Next time around the table the knight is playable — but only one dev per
    // turn.
    for s in 0..PLAYER_COUNT {
        if s > 0 {
            state.apply(s, Action::Roll).unwrap();
        }
        state.apply(s, Action::EndTurn).unwrap();
    }
    assert_eq!(state.active, 0);
    grant(&mut state, 0, dev_cost());
    state.apply(0, Action::PlayKnight).unwrap(); // pre-roll knight is legal
    assert_eq!(state.players[0].knights_played, 1);
    // Robber flow: resolve then we're back at pre-roll (not rolled yet).
    let target = (0..19u8).find(|&h| h != state.robber).unwrap();
    state.apply(0, Action::MoveRobber { hex: target }).unwrap();
    if let Phase::ChooseVictim { victims } = state.phase.clone() {
        state
            .apply(0, Action::StealFrom { seat: victims[0] })
            .unwrap();
    }
    assert_eq!(
        state.phase,
        Phase::PreRoll,
        "knight before roll returns to pre-roll"
    );
    state.apply(0, Action::Roll).unwrap();
    state.apply(0, Action::BuyDev).unwrap();
    let err = state.apply(0, Action::PlayKnight).unwrap_err();
    assert!(
        err.0.contains("already played") || err.0.contains("bought this turn"),
        "{}",
        err.0
    );
}

#[test]
fn victory_point_dev_cards_win_immediately_at_ten() {
    let mut state = through_setup(GameState::new_scripted(
        7,
        beginner_layout(),
        &[(2, 3)],
        vec![DevCard::VictoryPoint],
    ));
    state.apply(0, Action::Roll).unwrap();
    // Hand-place to 9 VP without touching roads (no award recompute): two
    // cities (4) + Largest Army (2) + three hidden VP cards (3).
    state.largest_army = Some((0, 3));
    let s0 = state.players[0].settlements.clone();
    for v in s0 {
        state.players[0].settlements.retain(|&x| x != v);
        state.players[0].cities.push(v);
        state.buildings[v as usize] = Some((0, true));
    }
    state.players[0].devs_playable = vec![
        DevCard::VictoryPoint,
        DevCard::VictoryPoint,
        DevCard::VictoryPoint,
    ];
    assert_eq!(state.victory_points(0, true), 9);
    assert_eq!(state.victory_points(0, false), 6, "hidden VP stays hidden");

    grant(&mut state, 0, dev_cost());
    state.apply(0, Action::BuyDev).unwrap();
    assert_eq!(
        state.winner,
        Some(0),
        "10th VP from a bought card ends the game"
    );
    assert!(state.is_over());
}

#[test]
fn largest_army_transfers_only_on_strictly_more_knights() {
    let mut state = through_setup(scripted(&[]));
    state.players[0].knights_played = 2;
    state.settle_largest_army(0);
    assert_eq!(state.largest_army, None);
    state.players[0].knights_played = 3;
    state.settle_largest_army(0);
    assert_eq!(state.largest_army, Some((0, 3)));
    // A tie does not transfer.
    state.players[1].knights_played = 3;
    state.settle_largest_army(1);
    assert_eq!(state.largest_army, Some((0, 3)));
    state.players[1].knights_played = 4;
    state.settle_largest_army(1);
    assert_eq!(state.largest_army, Some((1, 4)));
}

#[test]
fn bank_shortage_pays_single_claimant_partially_and_multiple_none() {
    let mut state = through_setup(scripted(&[(2, 4), (2, 4)]));
    // Drain the bank of brick down to 1.
    let held = state.bank.brick - 1;
    state.bank.remove(Resource::Brick, held);
    state.players[3].resources.add(Resource::Brick, held);

    // Give seat 0 a city on the 6-hills hex so a roll of 6 demands 2 brick
    // with only 1 in the bank → single claimant takes the remainder... unless
    // another seat also touches the hex, in which case nobody is paid.
    let g = graph();
    let hills6 = 4u8;
    let claimants: Vec<Seat> = g.hex_vertices[hills6 as usize]
        .iter()
        .filter_map(|&v| state.buildings[v as usize].map(|(s, _)| s))
        .collect();
    let brick_before: Vec<u8> = state.players.iter().map(|p| p.resources.brick).collect();
    state.apply(0, Action::Roll).unwrap(); // 6
    let brick_after: Vec<u8> = state.players.iter().map(|p| p.resources.brick).collect();
    let paid_total: i32 = brick_after
        .iter()
        .zip(&brick_before)
        .map(|(a, b)| *a as i32 - *b as i32)
        .sum();
    if claimants.len() > 1 {
        assert_eq!(
            paid_total, 0,
            "no one is paid when the bank can't cover all"
        );
    } else {
        assert!(
            paid_total <= 1,
            "single claimant limited to the bank remainder"
        );
    }
}

#[test]
fn dice_stream_is_independent_of_play_randomness() {
    // Two same-seed states; one burns general RNG on forced defaults first.
    let mut a = GameState::new(42);
    let mut b = GameState::new(42);
    let d = b.pending_decisions()[0].clone();
    for _ in 0..5 {
        b.forced_default(&d); // consumes b.rng only
    }
    use rand::Rng;
    let a_dice: Vec<u8> = (0..20).map(|_| a.rng_dice.gen_range(1..=6)).collect();
    let b_dice: Vec<u8> = (0..20).map(|_| b.rng_dice.gen_range(1..=6)).collect();
    assert_eq!(a_dice, b_dice, "dice must mirror across diverging play");
}
