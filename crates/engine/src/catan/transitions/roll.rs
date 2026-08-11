//! Dice, production (with the bank-shortage rule), the 7: discard-half,
//! robber movement, and stealing.

use super::super::actions::IllegalMove;
use super::super::board::{graph, HexId, HEX_COUNT};
use super::super::events::CatanEvent;
use super::super::state::{GameState, Phase};
use super::super::types::{Resource, ResourceSet, Seat, DISCARD_LIMIT, RESOURCES};
use crate::game_core::Visibility;
use rand::Rng;

pub(super) fn roll_dice(state: &mut GameState) -> Result<(), IllegalMove> {
    let (d1, d2) = state.scripted_dice.pop_front().unwrap_or_else(|| {
        (
            state.rng_dice.gen_range(1..=6u8),
            state.rng_dice.gen_range(1..=6u8),
        )
    });
    state.rolled = true;
    state.push_event(
        Visibility::Public,
        CatanEvent::DiceRolled {
            seat: state.active,
            d1,
            d2,
        },
    );

    if d1 + d2 == 7 {
        let over: Vec<(Seat, u8)> = (0..state.players.len() as Seat)
            .filter_map(|s| {
                let hand = state.players[s as usize].resources.total();
                (hand > DISCARD_LIMIT).then_some((s, hand / 2))
            })
            .collect();
        if over.is_empty() {
            state.phase = Phase::MoveRobber;
        } else {
            state.push_event(
                Visibility::Public,
                CatanEvent::MustDiscard {
                    seats: over.clone(),
                },
            );
            state.phase = Phase::Discard { pending: over };
        }
    } else {
        produce(state, d1 + d2);
        state.phase = Phase::Turn;
    }
    Ok(())
}

/// Pay out one roll's production, resource by resource. If the bank cannot
/// cover a resource's total demand and more than one seat is owed it, nobody
/// receives that resource; a single claimant takes what remains.
fn produce(state: &mut GameState, number: u8) {
    let g = graph();
    let mut demands: Vec<(Resource, Vec<(Seat, u8)>)> = Vec::new();
    for h in 0..HEX_COUNT {
        if state.layout.numbers[h] != Some(number) || state.robber == h as HexId {
            continue;
        }
        let Some(resource) = state.layout.terrains[h].resource() else {
            continue;
        };
        for &v in &g.hex_vertices[h] {
            if let Some((owner, is_city)) = state.buildings[v as usize] {
                let amount = if is_city { 2 } else { 1 };
                let entry = match demands.iter_mut().find(|(r, _)| *r == resource) {
                    Some(e) => e,
                    None => {
                        demands.push((resource, Vec::new()));
                        demands.last_mut().expect("just pushed")
                    }
                };
                match entry.1.iter_mut().find(|(s, _)| *s == owner) {
                    Some((_, n)) => *n += amount,
                    None => entry.1.push((owner, amount)),
                }
            }
        }
    }

    let mut gains: Vec<(Seat, ResourceSet)> = Vec::new();
    for (resource, claims) in demands {
        let total: u8 = claims.iter().map(|(_, n)| n).sum();
        let available = state.bank.get(resource);
        let paid: Vec<(Seat, u8)> = if total <= available {
            claims
        } else if claims.len() == 1 {
            vec![(claims[0].0, available)]
        } else {
            state.push_event(
                Visibility::Public,
                CatanEvent::ResourceShortage { resource },
            );
            continue;
        };
        for (seat, n) in paid {
            if n == 0 {
                continue;
            }
            assert!(state.bank.remove(resource, n), "bank covered by checks");
            state.players[seat as usize].resources.add(resource, n);
            match gains.iter_mut().find(|(s, _)| *s == seat) {
                Some((_, set)) => set.add(resource, n),
                None => gains.push((seat, ResourceSet::of(resource, n))),
            }
        }
    }
    gains.sort_by_key(|(s, _)| *s);
    if !gains.is_empty() {
        state.push_event(Visibility::Public, CatanEvent::ResourcesProduced { gains });
    }
}

pub(super) fn discard(
    state: &mut GameState,
    seat: Seat,
    pending: &[(Seat, u8)],
    resources: ResourceSet,
) -> Result<(), IllegalMove> {
    let &(_, required) = pending
        .iter()
        .find(|(s, _)| *s == seat)
        .expect("apply verified the seat is pending");
    if resources.total() != required {
        return Err(IllegalMove(format!(
            "you must discard exactly {required} cards, you named {}",
            resources.total()
        )));
    }
    if !state.players[seat as usize].resources.contains(&resources) {
        return Err(IllegalMove(format!(
            "you do not hold {}; your hand is {}",
            resources.describe(),
            state.players[seat as usize].resources.describe()
        )));
    }
    state.players[seat as usize]
        .resources
        .remove_set(&resources);
    state.bank.add_set(&resources);
    state.push_event(
        Visibility::Public,
        CatanEvent::Discarded {
            seat,
            count: required,
        },
    );
    state.push_event(
        Visibility::Private(seat),
        CatanEvent::DiscardedContents { seat, resources },
    );

    let remaining: Vec<(Seat, u8)> = pending
        .iter()
        .copied()
        .filter(|(s, _)| *s != seat)
        .collect();
    state.phase = if remaining.is_empty() {
        Phase::MoveRobber
    } else {
        Phase::Discard { pending: remaining }
    };
    Ok(())
}

pub(super) fn move_robber(
    state: &mut GameState,
    seat: Seat,
    hex: HexId,
) -> Result<(), IllegalMove> {
    if hex as usize >= HEX_COUNT {
        return Err(IllegalMove(format!("hex {hex} does not exist")));
    }
    if hex == state.robber {
        return Err(IllegalMove(
            "the robber must move to a different hex".into(),
        ));
    }
    state.robber = hex;
    state.push_event(Visibility::Public, CatanEvent::RobberMoved { seat, hex });

    let victims = state.robber_victims(seat, hex);
    match victims.len() {
        0 => super::resume_after_robber(state),
        1 => steal(state, seat, victims[0]),
        _ => state.phase = Phase::ChooseVictim { victims },
    }
    Ok(())
}

pub(super) fn choose_victim(
    state: &mut GameState,
    seat: Seat,
    victims: &[Seat],
    victim: Seat,
) -> Result<(), IllegalMove> {
    if !victims.contains(&victim) {
        return Err(IllegalMove(format!(
            "seat {victim} is not adjacent to the robber with cards in hand (victims: {victims:?})"
        )));
    }
    steal(state, seat, victim);
    Ok(())
}

/// Take one uniformly-random card from the victim's hand. Uses the dedicated
/// steal stream so a steal never perturbs the mirrored dice sequence.
fn steal(state: &mut GameState, thief: Seat, victim: Seat) {
    let hand = state.players[victim as usize].resources;
    let pick = state.rng_steal.gen_range(0..hand.total());
    let mut acc = 0u8;
    let stolen = RESOURCES
        .into_iter()
        .find(|&r| {
            acc += hand.get(r);
            pick < acc
        })
        .expect("victims always hold at least one card");

    state.players[victim as usize].resources.remove(stolen, 1);
    state.players[thief as usize].resources.add(stolen, 1);
    state.push_event(
        Visibility::Public,
        CatanEvent::CardStolen {
            from: victim,
            to: thief,
        },
    );
    for observer in [thief, victim] {
        state.push_event(
            Visibility::Private(observer),
            CatanEvent::CardStolenContents {
                from: victim,
                to: thief,
                resource: stolen,
            },
        );
    }
    super::resume_after_robber(state);
}
