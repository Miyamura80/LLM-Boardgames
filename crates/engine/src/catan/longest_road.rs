//! Longest-road computation: the longest continuous route in a seat's road
//! subgraph. A route may branch through the seat's own (or empty) vertices but
//! is severed by an opponent's settlement or city. Exhaustive DFS — trivial at
//! Catan's size (≤15 edges per player).

use super::board::{graph, EdgeId, VertexId};
use super::state::GameState;
use super::types::Seat;

/// Length (in roads) of the seat's longest continuous route.
pub fn longest_route(state: &GameState, seat: Seat) -> u8 {
    let g = graph();
    let own: Vec<EdgeId> = (0..state.roads.len() as EdgeId)
        .filter(|&e| state.roads[e as usize] == Some(seat))
        .collect();

    let mut best = 0u8;
    for &start in &own {
        let (a, b) = g.edge_ends(start);
        for from in [a, b] {
            let mut used: u128 = 1 << start;
            let other = if from == a { b } else { a };
            best = best.max(1 + dfs(state, seat, other, &mut used, g));
        }
    }
    best
}

/// Longest extension from `at` using unused own edges. Traversal stops at an
/// opponent-occupied vertex (the edge reaching it still counted).
fn dfs(
    state: &GameState,
    seat: Seat,
    at: VertexId,
    used: &mut u128,
    g: &'static super::board::BoardGraph,
) -> u8 {
    if let Some((owner, _)) = state.buildings[at as usize] {
        if owner != seat {
            return 0;
        }
    }
    let mut best = 0u8;
    for &e in &g.vertex_edges[at as usize] {
        if *used & (1 << e) != 0 || state.roads[e as usize] != Some(seat) {
            continue;
        }
        let (a, b) = g.edge_ends(e);
        let next = if a == at { b } else { a };
        *used |= 1 << e;
        best = best.max(1 + dfs(state, seat, next, used, g));
        *used &= !(1 << e);
    }
    best
}
