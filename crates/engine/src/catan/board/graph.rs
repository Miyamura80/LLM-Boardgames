//! The static topology of the standard 19-hex board: hexes on axial
//! coordinates, with canonical vertex and edge ids derived deterministically.
//!
//! Pointy-top axial coordinates (`q` east, `r` south-east). Every physical
//! vertex is the **N** (top) corner of exactly one hex or the **S** (bottom)
//! corner of exactly one hex — possibly an ocean hex just off the board — so
//! `(q, r, top?)` is a collision-free canonical key. The six corners of hex
//! `(q, r)` clockwise from north are:
//! `Top(q,r), Bottom(q+1,r-1), Top(q,r+1), Bottom(q,r), Top(q-1,r+1), Bottom(q,r-1)`.
//!
//! Ids are assigned in first-encounter order over the fixed hex list, so they
//! are stable forever (prompt renderings and stored records depend on this).

use std::collections::BTreeMap;
use std::sync::OnceLock;

pub type HexId = u8;
pub type VertexId = u8;
pub type EdgeId = u8;

pub const HEX_COUNT: usize = 19;
pub const VERTEX_COUNT: usize = 54;
pub const EDGE_COUNT: usize = 72;
/// The coastal ring is 30 edges; 9 of them carry ports.
pub const COAST_EDGE_COUNT: usize = 30;
pub const PORT_COUNT: usize = 9;

/// The 19 board hexes in reading order (row by row, west to east).
pub const HEX_COORDS: [(i8, i8); HEX_COUNT] = [
    (0, -2),
    (1, -2),
    (2, -2),
    (-1, -1),
    (0, -1),
    (1, -1),
    (2, -1),
    (-2, 0),
    (-1, 0),
    (0, 0),
    (1, 0),
    (2, 0),
    (-2, 1),
    (-1, 1),
    (0, 1),
    (1, 1),
    (-2, 2),
    (-1, 2),
    (0, 2),
];

/// Canonical corner key: the hex for which this vertex is the top (N) or
/// bottom (S) corner. That hex may lie outside the board (ocean).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CornerKey {
    q: i8,
    r: i8,
    top: bool,
}

/// The six corners of hex `(q, r)`, clockwise from north.
fn hex_corners(q: i8, r: i8) -> [CornerKey; 6] {
    let c = |q, r, top| CornerKey { q, r, top };
    [
        c(q, r, true),          // N
        c(q + 1, r - 1, false), // NE
        c(q, r + 1, true),      // SE
        c(q, r, false),         // S
        c(q - 1, r + 1, true),  // SW
        c(q, r - 1, false),     // NW
    ]
}

/// The static board topology, built once.
#[derive(Debug)]
pub struct BoardGraph {
    /// The 6 vertices of each hex, clockwise from north.
    pub hex_vertices: [[VertexId; 6]; HEX_COUNT],
    /// Board hexes touching each vertex (1–3).
    pub vertex_hexes: Vec<Vec<HexId>>,
    /// Edges incident to each vertex (2–3).
    pub vertex_edges: Vec<Vec<EdgeId>>,
    /// Vertices adjacent to each vertex (2–3) — the distance-rule neighborhood.
    pub vertex_neighbors: Vec<Vec<VertexId>>,
    /// Endpoint vertices of each edge (lower id first).
    pub edge_vertices: Vec<(VertexId, VertexId)>,
    /// Neighboring board hexes of each hex (2–6).
    pub hex_neighbors: Vec<Vec<HexId>>,
    /// The 30 coastal edges, ordered as a ring walk around the island —
    /// the deterministic frame ports are placed on.
    pub coast_ring: Vec<EdgeId>,
}

impl BoardGraph {
    fn build() -> Self {
        let mut vertex_ids: BTreeMap<CornerKey, VertexId> = BTreeMap::new();
        let mut order: Vec<CornerKey> = Vec::new(); // first-encounter order
        let mut hex_vertices = [[0u8; 6]; HEX_COUNT];

        for (h, &(q, r)) in HEX_COORDS.iter().enumerate() {
            for (c, key) in hex_corners(q, r).into_iter().enumerate() {
                let next = order.len() as VertexId;
                let id = *vertex_ids.entry(key).or_insert_with(|| {
                    order.push(key);
                    next
                });
                hex_vertices[h][c] = id;
            }
        }
        let vertex_count = order.len();
        assert_eq!(vertex_count, VERTEX_COUNT);

        // Edges: consecutive corners around each hex, deduped.
        let mut edge_ids: BTreeMap<(VertexId, VertexId), EdgeId> = BTreeMap::new();
        let mut edge_vertices: Vec<(VertexId, VertexId)> = Vec::new();
        let mut edge_hex_count: Vec<u8> = Vec::new();
        for hv in &hex_vertices {
            for c in 0..6 {
                let (a, b) = (hv[c], hv[(c + 1) % 6]);
                let key = (a.min(b), a.max(b));
                let id = *edge_ids.entry(key).or_insert_with(|| {
                    edge_vertices.push(key);
                    edge_hex_count.push(0);
                    (edge_vertices.len() - 1) as EdgeId
                });
                edge_hex_count[id as usize] += 1;
            }
        }
        assert_eq!(edge_vertices.len(), EDGE_COUNT);

        let mut vertex_hexes = vec![Vec::new(); vertex_count];
        for (h, hv) in hex_vertices.iter().enumerate() {
            for &v in hv {
                vertex_hexes[v as usize].push(h as HexId);
            }
        }

        let mut vertex_edges = vec![Vec::new(); vertex_count];
        let mut vertex_neighbors = vec![Vec::new(); vertex_count];
        for (e, &(a, b)) in edge_vertices.iter().enumerate() {
            vertex_edges[a as usize].push(e as EdgeId);
            vertex_edges[b as usize].push(e as EdgeId);
            vertex_neighbors[a as usize].push(b);
            vertex_neighbors[b as usize].push(a);
        }

        let mut hex_neighbors = vec![Vec::new(); HEX_COUNT];
        for (i, &(qi, ri)) in HEX_COORDS.iter().enumerate() {
            for (j, &(qj, rj)) in HEX_COORDS.iter().enumerate() {
                if i == j {
                    continue;
                }
                let (dq, dr) = (qj - qi, rj - ri);
                let adjacent = matches!(
                    (dq, dr),
                    (1, 0) | (-1, 0) | (0, 1) | (0, -1) | (1, -1) | (-1, 1)
                );
                if adjacent {
                    hex_neighbors[i].push(j as HexId);
                }
            }
        }

        // Coastal ring: the 30 edges bounding exactly one hex, chained into a
        // deterministic walk starting from the lowest-id coastal edge.
        let coastal: Vec<EdgeId> = (0..EDGE_COUNT as EdgeId)
            .filter(|&e| edge_hex_count[e as usize] == 1)
            .collect();
        assert_eq!(coastal.len(), COAST_EDGE_COUNT);
        let mut ring = vec![coastal[0]];
        let mut used = [false; EDGE_COUNT];
        used[coastal[0] as usize] = true;
        while ring.len() < COAST_EDGE_COUNT {
            let last = *ring.last().expect("ring is non-empty");
            let (a, b) = edge_vertices[last as usize];
            let next = coastal
                .iter()
                .find(|&&e| {
                    let (x, y) = edge_vertices[e as usize];
                    !used[e as usize] && (x == a || x == b || y == a || y == b)
                })
                .copied()
                .expect("coastal edges form a closed ring");
            used[next as usize] = true;
            ring.push(next);
        }

        Self {
            hex_vertices,
            vertex_hexes,
            vertex_edges,
            vertex_neighbors,
            edge_vertices,
            hex_neighbors,
            coast_ring: ring,
        }
    }

    /// The two endpoint vertices of an edge.
    pub fn edge_ends(&self, e: EdgeId) -> (VertexId, VertexId) {
        self.edge_vertices[e as usize]
    }

    /// The edge joining two adjacent vertices, if any.
    pub fn edge_between(&self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        let key = (a.min(b), a.max(b));
        self.edge_vertices
            .iter()
            .position(|&ends| ends == key)
            .map(|i| i as EdgeId)
    }
}

/// The shared, lazily-built board topology.
pub fn graph() -> &'static BoardGraph {
    static GRAPH: OnceLock<BoardGraph> = OnceLock::new();
    GRAPH.get_or_init(BoardGraph::build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_has_standard_counts() {
        let g = graph();
        assert_eq!(g.edge_vertices.len(), EDGE_COUNT);
        assert_eq!(g.vertex_hexes.len(), VERTEX_COUNT);
        assert_eq!(g.coast_ring.len(), COAST_EDGE_COUNT);
        // Every vertex touches 1–3 board hexes and 2–3 edges.
        for v in 0..VERTEX_COUNT {
            assert!((1..=3).contains(&g.vertex_hexes[v].len()));
            assert!((2..=3).contains(&g.vertex_edges[v].len()));
        }
        // The center hex has six neighbors; corner hexes have three.
        let center = HEX_COORDS
            .iter()
            .position(|&c| c == (0, 0))
            .expect("center");
        assert_eq!(g.hex_neighbors[center].len(), 6);
    }

    #[test]
    fn coast_ring_is_a_closed_walk() {
        let g = graph();
        for w in 0..COAST_EDGE_COUNT {
            let (a1, b1) = g.edge_ends(g.coast_ring[w]);
            let (a2, b2) = g.edge_ends(g.coast_ring[(w + 1) % COAST_EDGE_COUNT]);
            assert!(
                a1 == a2 || a1 == b2 || b1 == a2 || b1 == b2,
                "consecutive coastal edges must share a vertex"
            );
        }
    }
}
