//! Placement legality over raw occupancy arrays — shared by the engine
//! (`GameState`) and by agents (`Observation`), so bots and prompt renderings
//! can never disagree with the rules.

use super::board::{graph, EdgeId, VertexId, EDGE_COUNT, VERTEX_COUNT};
use super::types::Seat;

pub type Buildings = [Option<(Seat, bool)>];
pub type Roads = [Option<Seat>];

/// Vacant and clear of every neighbor vertex (the distance rule).
pub fn vertex_open_for_settlement(buildings: &Buildings, v: VertexId) -> bool {
    let g = graph();
    buildings[v as usize].is_none()
        && g.vertex_neighbors[v as usize]
            .iter()
            .all(|&n| buildings[n as usize].is_none())
}

/// Setup: open vertices anywhere on the board.
pub fn setup_settlement_sites(buildings: &Buildings) -> Vec<VertexId> {
    (0..VERTEX_COUNT as VertexId)
        .filter(|&v| vertex_open_for_settlement(buildings, v))
        .collect()
}

/// Setup: the road must attach to the just-placed settlement.
pub fn setup_road_sites(roads: &Roads, settlement: VertexId) -> Vec<EdgeId> {
    let g = graph();
    g.vertex_edges[settlement as usize]
        .iter()
        .copied()
        .filter(|&e| roads[e as usize].is_none())
        .collect()
}

/// Normal-play settlement sites: open by the distance rule AND touching one of
/// the seat's roads.
pub fn settlement_sites(buildings: &Buildings, roads: &Roads, seat: Seat) -> Vec<VertexId> {
    let g = graph();
    (0..VERTEX_COUNT as VertexId)
        .filter(|&v| {
            vertex_open_for_settlement(buildings, v)
                && g.vertex_edges[v as usize]
                    .iter()
                    .any(|&e| roads[e as usize] == Some(seat))
        })
        .collect()
}

/// Legal road edges: empty, and connected to an own building, or to an own
/// road through a vertex not blocked by an opponent's building.
pub fn road_sites(buildings: &Buildings, roads: &Roads, seat: Seat) -> Vec<EdgeId> {
    (0..EDGE_COUNT as EdgeId)
        .filter(|&e| roads[e as usize].is_none() && edge_connects(buildings, roads, seat, e))
        .collect()
}

fn edge_connects(buildings: &Buildings, roads: &Roads, seat: Seat, e: EdgeId) -> bool {
    let g = graph();
    let (a, b) = g.edge_ends(e);
    for v in [a, b] {
        match buildings[v as usize] {
            Some((owner, _)) if owner == seat => return true,
            Some(_) => continue, // opponent building blocks through-connection
            None => {
                if g.vertex_edges[v as usize]
                    .iter()
                    .any(|&other| other != e && roads[other as usize] == Some(seat))
                {
                    return true;
                }
            }
        }
    }
    false
}
