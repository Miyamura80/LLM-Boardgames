//! Building (roads, settlements, cities), dev-card purchase, and the Longest
//! Road / Largest Army awards.

use super::super::actions::IllegalMove;
use super::super::board::{EdgeId, VertexId};
use super::super::events::CatanEvent;
use super::super::longest_road::longest_route;
use super::super::state::{GameState, Phase};
use super::super::types::*;
use crate::game_core::Visibility;

fn pay(
    state: &mut GameState,
    seat: Seat,
    cost: &ResourceSet,
    what: &str,
) -> Result<(), IllegalMove> {
    if !state.players[seat as usize].resources.contains(cost) {
        return Err(IllegalMove(format!(
            "a {what} costs {}; your hand is {}",
            cost.describe(),
            state.players[seat as usize].resources.describe()
        )));
    }
    state.players[seat as usize].resources.remove_set(cost);
    state.bank.add_set(cost);
    Ok(())
}

pub(super) fn build_road(
    state: &mut GameState,
    seat: Seat,
    edge: EdgeId,
) -> Result<(), IllegalMove> {
    check_road_site(state, seat, edge)?;
    pay(state, seat, &road_cost(), "road")?;
    place_road(state, seat, edge, false);
    Ok(())
}

/// A Road Building placement: free, and returns to the action loop when the
/// grant is used up (or no legal site remains).
pub(super) fn free_road(
    state: &mut GameState,
    seat: Seat,
    edge: EdgeId,
) -> Result<(), IllegalMove> {
    check_road_site(state, seat, edge)?;
    place_road(state, seat, edge, true);
    state.free_roads -= 1;
    if state.free_roads == 0 || state.road_sites(seat).is_empty() {
        state.free_roads = 0;
        state.phase = Phase::Turn;
    }
    Ok(())
}

fn check_road_site(state: &GameState, seat: Seat, edge: EdgeId) -> Result<(), IllegalMove> {
    if state.players[seat as usize].roads_left() == 0 {
        return Err(IllegalMove(format!(
            "you have placed all {MAX_ROADS} of your roads"
        )));
    }
    if !state.road_sites(seat).contains(&edge) {
        return Err(IllegalMove(format!(
            "edge {edge} is occupied, off-board, or not connected to your network"
        )));
    }
    Ok(())
}

fn place_road(state: &mut GameState, seat: Seat, edge: EdgeId, free: bool) {
    state.roads[edge as usize] = Some(seat);
    state.players[seat as usize].roads_placed += 1;
    state.push_event(
        Visibility::Public,
        CatanEvent::RoadBuilt { seat, edge, free },
    );
    award_longest_road(state);
    state.check_win();
}

pub(super) fn build_settlement(
    state: &mut GameState,
    seat: Seat,
    vertex: VertexId,
) -> Result<(), IllegalMove> {
    if state.players[seat as usize].settlements_left() == 0 {
        return Err(IllegalMove(format!(
            "all {MAX_SETTLEMENTS} of your settlements are on the board — upgrade one to a city first"
        )));
    }
    if !state.settlement_sites(seat).contains(&vertex) {
        return Err(IllegalMove(format!(
            "vertex {vertex} is not a legal settlement site (it must be vacant, off every neighbor vertex, and touch one of your roads)"
        )));
    }
    pay(state, seat, &settlement_cost(), "settlement")?;
    state.buildings[vertex as usize] = Some((seat, false));
    state.players[seat as usize].settlements.push(vertex);
    state.push_event(
        Visibility::Public,
        CatanEvent::SettlementBuilt { seat, vertex },
    );
    // A new settlement can sever an opponent's road.
    award_longest_road(state);
    state.check_win();
    Ok(())
}

pub(super) fn build_city(
    state: &mut GameState,
    seat: Seat,
    vertex: VertexId,
) -> Result<(), IllegalMove> {
    if state.players[seat as usize].cities_left() == 0 {
        return Err(IllegalMove(format!(
            "all {MAX_CITIES} of your cities are placed"
        )));
    }
    if state.buildings.get(vertex as usize).copied().flatten() != Some((seat, false)) {
        return Err(IllegalMove(format!(
            "vertex {vertex} does not hold one of your settlements"
        )));
    }
    pay(state, seat, &city_cost(), "city")?;
    state.buildings[vertex as usize] = Some((seat, true));
    state.players[seat as usize]
        .settlements
        .retain(|&v| v != vertex);
    state.players[seat as usize].cities.push(vertex);
    state.push_event(Visibility::Public, CatanEvent::CityBuilt { seat, vertex });
    state.check_win();
    Ok(())
}

pub(super) fn buy_dev(state: &mut GameState, seat: Seat) -> Result<(), IllegalMove> {
    if state.dev_deck.is_empty() {
        return Err(IllegalMove("the development deck is empty".into()));
    }
    pay(state, seat, &dev_cost(), "development card")?;
    let card = state.dev_deck.pop().expect("checked non-empty");
    state.players[seat as usize].devs_new.push(card);
    state.push_event(Visibility::Public, CatanEvent::DevCardBought { seat });
    state.push_event(
        Visibility::Private(seat),
        CatanEvent::DevCardDrawn { seat, card },
    );
    // A bought VP card counts immediately for the buyer's own win check.
    state.check_win();
    Ok(())
}

/// Recompute Longest Road for everyone and settle the title. The holder keeps
/// it on ties; if the holder's route is severed, the unique new maximum (≥5)
/// takes it, and a tie sets the title aside.
pub(crate) fn award_longest_road(state: &mut GameState) {
    let lens: Vec<u8> = (0..PLAYER_COUNT).map(|s| longest_route(state, s)).collect();
    let max = *lens.iter().max().expect("four seats");
    let previous = state.longest_road.map(|(s, _)| s);

    let new_holder: Option<Seat> = match state.longest_road {
        Some((holder, _)) => {
            let hl = lens[holder as usize];
            if hl >= LONGEST_ROAD_MIN && hl >= max {
                Some(holder) // retains on ties
            } else if max >= LONGEST_ROAD_MIN {
                let leaders: Vec<Seat> = (0..PLAYER_COUNT)
                    .filter(|&s| lens[s as usize] == max)
                    .collect();
                (leaders.len() == 1).then(|| leaders[0])
            } else {
                None
            }
        }
        None => {
            if max >= LONGEST_ROAD_MIN {
                let leaders: Vec<Seat> = (0..PLAYER_COUNT)
                    .filter(|&s| lens[s as usize] == max)
                    .collect();
                (leaders.len() == 1).then(|| leaders[0])
            } else {
                None
            }
        }
    };

    let new_entry = new_holder.map(|s| (s, lens[s as usize]));
    if new_entry != state.longest_road {
        state.longest_road = new_entry;
        state.push_event(
            Visibility::Public,
            CatanEvent::LongestRoadClaimed {
                seat: new_holder,
                length: new_holder.map(|s| lens[s as usize]).unwrap_or(max),
                previous,
            },
        );
    } else if let Some((holder, _)) = state.longest_road {
        // Same holder, possibly longer route — keep the stored length honest.
        state.longest_road = Some((holder, lens[holder as usize]));
    }
}

/// Largest Army: first to three knights takes it; only a strictly greater
/// count takes it away.
pub(crate) fn award_largest_army(state: &mut GameState, seat: Seat) {
    let knights = state.players[seat as usize].knights_played;
    let take = match state.largest_army {
        None => knights >= LARGEST_ARMY_MIN,
        Some((holder, count)) => holder != seat && knights > count,
    };
    let held_already = state.largest_army.map(|(s, _)| s) == Some(seat);
    if take {
        let previous = state.largest_army.map(|(s, _)| s);
        state.largest_army = Some((seat, knights));
        state.push_event(
            Visibility::Public,
            CatanEvent::LargestArmyClaimed {
                seat,
                knights,
                previous,
            },
        );
    } else if held_already {
        state.largest_army = Some((seat, knights));
    }
}
