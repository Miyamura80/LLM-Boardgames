//! Scripted-game helpers for the rules tests: a fixed classic board, a
//! scripted dice sequence, and a chosen dev deck — so tests assert exact
//! outcomes instead of fishing through seeded randomness.

use super::board::BoardLayout;
use super::state::{GameState, TurnCaps};
use super::types::DevCard;

/// The classic beginner board from the rulebook, in `HEX_COORDS` reading
/// order. Ports use the standard ring slots with a fixed kind order.
pub fn beginner_layout() -> BoardLayout {
    use super::board::{graph, Port};
    use super::types::{Resource, Terrain};
    let t = |x| x;
    let terrains = vec![
        t(Terrain::Mountains),
        t(Terrain::Pasture),
        t(Terrain::Forest),
        t(Terrain::Fields),
        t(Terrain::Hills),
        t(Terrain::Pasture),
        t(Terrain::Hills),
        t(Terrain::Fields),
        t(Terrain::Forest),
        t(Terrain::Desert),
        t(Terrain::Forest),
        t(Terrain::Mountains),
        t(Terrain::Forest),
        t(Terrain::Mountains),
        t(Terrain::Fields),
        t(Terrain::Pasture),
        t(Terrain::Hills),
        t(Terrain::Fields),
        t(Terrain::Pasture),
    ];
    let numbers = vec![
        Some(10),
        Some(2),
        Some(9),
        Some(12),
        Some(6),
        Some(4),
        Some(10),
        Some(9),
        Some(11),
        None,
        Some(3),
        Some(8),
        Some(8),
        Some(3),
        Some(4),
        Some(5),
        Some(5),
        Some(6),
        Some(11),
    ];
    let g = graph();
    let kinds = [
        Port::Generic,
        Port::Resource {
            resource: Resource::Grain,
        },
        Port::Resource {
            resource: Resource::Ore,
        },
        Port::Generic,
        Port::Resource {
            resource: Resource::Wool,
        },
        Port::Generic,
        Port::Generic,
        Port::Resource {
            resource: Resource::Brick,
        },
        Port::Resource {
            resource: Resource::Lumber,
        },
    ];
    let slots = [0usize, 3, 7, 10, 13, 17, 20, 23, 27];
    let ports = slots
        .iter()
        .zip(kinds)
        .map(|(&s, k)| (g.coast_ring[s], k))
        .collect();
    BoardLayout {
        terrains,
        numbers,
        ports,
        desert: 9,
    }
}

impl GameState {
    /// A scripted state: fixed board, scripted dice (falling back to the
    /// seeded stream when exhausted), and an explicit dev deck (drawn from the
    /// **end**).
    pub fn new_scripted(
        seed: u64,
        layout: BoardLayout,
        dice: &[(u8, u8)],
        dev_deck: Vec<DevCard>,
    ) -> Self {
        let mut state = Self::with_caps(seed, TurnCaps::default());
        state.layout = layout.clone();
        state.robber = layout.desert;
        state.dev_deck = dev_deck;
        state.scripted_dice = dice.iter().copied().collect();
        // Re-emit the board event so transcripts match the scripted layout.
        state.events.clear();
        let players = super::types::PLAYER_COUNT;
        state.push_event(
            crate::game_core::Visibility::Public,
            super::events::CatanEvent::GameStarted { players },
        );
        let board_event = state.board_laid_event();
        state.push_event(crate::game_core::Visibility::Public, board_event);
        state
    }
}
