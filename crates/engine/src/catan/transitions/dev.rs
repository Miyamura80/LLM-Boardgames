//! Development-card play: Knight, Road Building, Year of Plenty, Monopoly.
//! One dev card per turn, never on the turn it was bought (VP cards are never
//! "played" — they simply count).

use super::super::actions::IllegalMove;
use super::super::events::CatanEvent;
use super::super::state::{GameState, Phase};
use super::super::types::{DevCard, Resource, Seat, PLAYER_COUNT};
use crate::game_core::Visibility;

fn take_card(state: &mut GameState, seat: Seat, card: DevCard) -> Result<(), IllegalMove> {
    if state.dev_played_this_turn {
        return Err(IllegalMove(
            "you have already played a development card this turn".into(),
        ));
    }
    let devs = &mut state.players[seat as usize].devs_playable;
    let Some(pos) = devs.iter().position(|&c| c == card) else {
        let new = state.players[seat as usize]
            .devs_new
            .iter()
            .filter(|&&c| c == card)
            .count();
        return Err(IllegalMove(if new > 0 {
            format!(
                "your {} was bought this turn and cannot be played until your next turn",
                card.as_str()
            )
        } else {
            format!("you do not hold a playable {}", card.as_str())
        }));
    };
    state.players[seat as usize].devs_playable.remove(pos);
    state.dev_played_this_turn = true;
    state.push_event(Visibility::Public, CatanEvent::DevCardPlayed { seat, card });
    Ok(())
}

pub(super) fn play_knight(state: &mut GameState, seat: Seat) -> Result<(), IllegalMove> {
    take_card(state, seat, DevCard::Knight)?;
    state.players[seat as usize].knights_played += 1;
    super::build::award_largest_army(state, seat);
    state.check_win();
    if !state.is_over() {
        state.phase = Phase::MoveRobber;
    }
    Ok(())
}

pub(super) fn play_road_building(state: &mut GameState, seat: Seat) -> Result<(), IllegalMove> {
    let grants = state.players[seat as usize].roads_left().min(2);
    if grants == 0 {
        return Err(IllegalMove("you have no road pieces left".into()));
    }
    if state.road_sites(seat).is_empty() {
        return Err(IllegalMove("you have no legal road placement".into()));
    }
    take_card(state, seat, DevCard::RoadBuilding)?;
    state.free_roads = grants;
    state.phase = Phase::FreeRoad;
    Ok(())
}

pub(super) fn play_year_of_plenty(
    state: &mut GameState,
    seat: Seat,
    first: Resource,
    second: Resource,
) -> Result<(), IllegalMove> {
    let mut need = super::super::types::ResourceSet::default();
    need.add(first, 1);
    need.add(second, 1);
    if !state.bank.contains(&need) {
        return Err(IllegalMove(format!(
            "the bank cannot supply {}",
            need.describe()
        )));
    }
    take_card(state, seat, DevCard::YearOfPlenty)?;
    state.bank.remove_set(&need);
    state.players[seat as usize].resources.add_set(&need);
    state.push_event(
        Visibility::Public,
        CatanEvent::YearOfPlentyTaken {
            seat,
            first,
            second,
        },
    );
    Ok(())
}

pub(super) fn play_monopoly(
    state: &mut GameState,
    seat: Seat,
    resource: Resource,
) -> Result<(), IllegalMove> {
    take_card(state, seat, DevCard::Monopoly)?;
    let mut taken: Vec<(Seat, u8)> = Vec::new();
    for s in 0..PLAYER_COUNT {
        if s == seat {
            continue;
        }
        let n = state.players[s as usize].resources.get(resource);
        if n > 0 {
            state.players[s as usize].resources.remove(resource, n);
            state.players[seat as usize].resources.add(resource, n);
            taken.push((s, n));
        }
    }
    state.push_event(
        Visibility::Public,
        CatanEvent::MonopolyResolved {
            seat,
            resource,
            taken,
        },
    );
    Ok(())
}
