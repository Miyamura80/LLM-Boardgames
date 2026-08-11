//! Seeded board layout: terrain and number-token assignment (honoring the
//! no-adjacent-6/8 rule), port placement on the coastal ring, and the robber's
//! desert start. Deterministic given the layout RNG.

use super::graph::{graph, EdgeId, HexId, HEX_COUNT, PORT_COUNT};
use crate::catan::types::{Resource, Terrain};
use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A harbor: settle on either endpoint vertex to unlock its trade rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Port {
    /// 3:1, any resource.
    Generic,
    /// 2:1 for one specific resource.
    Resource { resource: Resource },
}

/// One game's board: which terrain/number sits on each hex, where the ports
/// are, and where the robber starts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardLayout {
    /// Terrain per hex, indexed by [`super::graph::HEX_COORDS`] order.
    pub terrains: Vec<Terrain>,
    /// Number token per hex (`None` for the desert).
    pub numbers: Vec<Option<u8>>,
    /// Ports on coastal edges.
    pub ports: Vec<(EdgeId, Port)>,
    /// The desert hex — the robber's starting position.
    pub desert: HexId,
}

/// The standard terrain pool: 4 fields, 4 forest, 4 pasture, 3 hills,
/// 3 mountains, 1 desert.
fn terrain_pool() -> Vec<Terrain> {
    let mut t = Vec::with_capacity(HEX_COUNT);
    t.extend(std::iter::repeat_n(Terrain::Fields, 4));
    t.extend(std::iter::repeat_n(Terrain::Forest, 4));
    t.extend(std::iter::repeat_n(Terrain::Pasture, 4));
    t.extend(std::iter::repeat_n(Terrain::Hills, 3));
    t.extend(std::iter::repeat_n(Terrain::Mountains, 3));
    t.push(Terrain::Desert);
    t
}

/// The standard token pool (no 7; 2 and 12 once, the rest twice).
const NUMBER_POOL: [u8; 18] = [2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12];

/// Coastal-ring positions of the nine ports (spacing 3,4,3,3,4,3,3,4,3).
const PORT_RING_SLOTS: [usize; PORT_COUNT] = [0, 3, 7, 10, 13, 17, 20, 23, 27];

impl BoardLayout {
    /// Generate a seeded layout. Number placement rejects arrangements where
    /// two 6/8 tokens sit on adjacent hexes (the official setup rule); the
    /// retry cap is unreachable in practice and exists only to guarantee
    /// termination.
    pub fn generate(rng: &mut ChaCha8Rng) -> Self {
        let g = graph();

        let mut terrains = terrain_pool();
        terrains.shuffle(rng);
        let desert = terrains
            .iter()
            .position(|&t| t == Terrain::Desert)
            .expect("pool holds one desert") as HexId;

        // Number tokens over the non-desert hexes, no adjacent 6/8.
        let hot = |n: Option<u8>| matches!(n, Some(6) | Some(8));
        let mut numbers: Vec<Option<u8>> = vec![None; HEX_COUNT];
        for attempt in 0..10_000 {
            let mut pool = NUMBER_POOL.to_vec();
            pool.shuffle(rng);
            let mut it = pool.into_iter();
            for (h, slot) in numbers.iter_mut().enumerate() {
                *slot = if h as HexId == desert {
                    None
                } else {
                    Some(it.next().expect("18 tokens for 18 non-desert hexes"))
                };
            }
            let ok = (0..HEX_COUNT).all(|h| {
                !hot(numbers[h])
                    || g.hex_neighbors[h]
                        .iter()
                        .all(|&n| !hot(numbers[n as usize]))
            });
            if ok {
                break;
            }
            assert!(attempt < 9_999, "no valid number layout found");
        }

        // Ports: fixed ring slots, shuffled kinds.
        let mut kinds: Vec<Port> = vec![
            Port::Generic,
            Port::Generic,
            Port::Generic,
            Port::Generic,
            Port::Resource {
                resource: Resource::Brick,
            },
            Port::Resource {
                resource: Resource::Lumber,
            },
            Port::Resource {
                resource: Resource::Wool,
            },
            Port::Resource {
                resource: Resource::Grain,
            },
            Port::Resource {
                resource: Resource::Ore,
            },
        ];
        kinds.shuffle(rng);
        // A seeded rotation of the ring so port slots differ between boards.
        let offset = rng.gen_range(0..g.coast_ring.len());
        let ports: Vec<(EdgeId, Port)> = PORT_RING_SLOTS
            .iter()
            .zip(kinds)
            .map(|(&slot, kind)| {
                let e = g.coast_ring[(slot + offset) % g.coast_ring.len()];
                (e, kind)
            })
            .collect();

        Self {
            terrains,
            numbers,
            ports,
            desert,
        }
    }

    /// Production probability weight of a number token (out of 36 rolls).
    pub fn pips(number: u8) -> u8 {
        6 - (7i16 - number as i16).unsigned_abs() as u8
    }

    /// The port (if any) reachable from a vertex.
    pub fn port_at_vertex(&self, v: super::graph::VertexId) -> Option<Port> {
        let g = graph();
        self.ports.iter().find_map(|&(e, p)| {
            let (a, b) = g.edge_ends(e);
            (a == v || b == v).then_some(p)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn generated_layouts_are_valid_and_seed_stable() {
        for seed in 0..25u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let a = BoardLayout::generate(&mut rng);
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let b = BoardLayout::generate(&mut rng);
            assert_eq!(format!("{a:?}"), format!("{b:?}"), "same seed, same board");

            assert_eq!(a.terrains.len(), HEX_COUNT);
            assert_eq!(a.numbers.iter().filter(|n| n.is_none()).count(), 1);
            assert_eq!(a.ports.len(), PORT_COUNT);
            // No adjacent 6/8.
            let g = graph();
            for h in 0..HEX_COUNT {
                if matches!(a.numbers[h], Some(6) | Some(8)) {
                    for &n in &g.hex_neighbors[h] {
                        assert!(!matches!(a.numbers[n as usize], Some(6) | Some(8)));
                    }
                }
            }
            // Ports sit on distinct edges.
            let mut edges: Vec<EdgeId> = a.ports.iter().map(|&(e, _)| e).collect();
            edges.sort_unstable();
            edges.dedup();
            assert_eq!(edges.len(), PORT_COUNT);
        }
    }

    #[test]
    fn pip_weights_match_dice_odds() {
        assert_eq!(BoardLayout::pips(2), 1);
        assert_eq!(BoardLayout::pips(6), 5);
        assert_eq!(BoardLayout::pips(8), 5);
        assert_eq!(BoardLayout::pips(12), 1);
    }
}
