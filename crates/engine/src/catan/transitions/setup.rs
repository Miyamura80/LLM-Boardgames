//! The snake-draft setup: settlement + road per placement, with the second
//! settlement granting its adjacent hexes' resources.

use super::super::actions::IllegalMove;
use super::super::board::{graph, EdgeId, VertexId};
use super::super::events::CatanEvent;
use super::super::state::{GameState, Phase};
use super::super::types::{ResourceSet, Seat, PLAYER_COUNT};
use crate::game_core::Visibility;

pub(super) fn place_settlement(
    state: &mut GameState,
    seat: Seat,
    placements: u8,
    vertex: VertexId,
) -> Result<(), IllegalMove> {
    if vertex as usize >= state.buildings.len() {
        return Err(IllegalMove(format!("vertex {vertex} does not exist")));
    }
    if !state.vertex_open_for_settlement(vertex) {
        return Err(IllegalMove(format!(
            "vertex {vertex} is occupied or violates the distance rule (no settlement may neighbor another)"
        )));
    }
    let round = placements / PLAYER_COUNT;
    state.buildings[vertex as usize] = Some((seat, false));
    state.players[seat as usize].settlements.push(vertex);
    state.push_event(
        Visibility::Public,
        CatanEvent::SetupSettlementPlaced {
            seat,
            vertex,
            round,
        },
    );

    // The second settlement produces its adjacent hexes' resources at once.
    if round == 1 {
        let g = graph();
        let mut gained = ResourceSet::default();
        for &h in &g.vertex_hexes[vertex as usize] {
            if let Some(resource) = state.layout.terrains[h as usize].resource() {
                if state.bank.remove(resource, 1) {
                    gained.add(resource, 1);
                }
            }
        }
        state.players[seat as usize].resources.add_set(&gained);
        state.push_event(
            Visibility::Public,
            CatanEvent::SetupResourcesGranted { seat, gained },
        );
    }

    state.phase = Phase::Setup {
        placements,
        road_for: Some(vertex),
    };
    Ok(())
}

pub(super) fn place_road(
    state: &mut GameState,
    seat: Seat,
    placements: u8,
    at: VertexId,
    edge: EdgeId,
) -> Result<(), IllegalMove> {
    if !state.setup_road_sites(at).contains(&edge) {
        return Err(IllegalMove(format!(
            "edge {edge} does not attach to your new settlement at vertex {at}"
        )));
    }
    state.roads[edge as usize] = Some(seat);
    state.players[seat as usize].roads_placed += 1;
    state.push_event(
        Visibility::Public,
        CatanEvent::SetupRoadPlaced { seat, edge },
    );

    let next = placements + 1;
    if next == 2 * PLAYER_COUNT {
        // Setup complete — seat 0 opens turn 1.
        super::begin_turn(state, 0);
    } else {
        state.phase = Phase::Setup {
            placements: next,
            road_for: None,
        };
    }
    Ok(())
}
