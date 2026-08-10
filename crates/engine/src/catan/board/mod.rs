//! Board topology (static) and layout (per-game, seeded).

mod graph;
mod layout;

pub use graph::{
    graph, BoardGraph, EdgeId, HexId, VertexId, COAST_EDGE_COUNT, EDGE_COUNT, HEX_COORDS,
    HEX_COUNT, PORT_COUNT, VERTEX_COUNT,
};
pub use layout::{BoardLayout, Port};
