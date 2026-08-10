//! Per-seat observations: exactly what a seat is entitled to see — own hand
//! and dev cards in full, opponents only as counts, hidden VP excluded from
//! others' scores — plus the visibility-filtered event history.

use super::board::{BoardLayout, EdgeId, HexId, VertexId};
use super::events::EventRecord;
use super::state::GameState;
use super::types::{DevCard, ResourceSet, Seat, PLAYER_COUNT};

/// What everyone can see about a seat.
#[derive(Debug, Clone)]
pub struct PublicPlayer {
    pub seat: Seat,
    /// Board-visible VP only (no hidden VP cards).
    pub public_vp: u8,
    pub hand_count: u8,
    pub dev_count: u8,
    pub knights_played: u8,
    pub roads_left: u8,
    pub settlements_left: u8,
    pub cities_left: u8,
}

/// Current occupancy, small enough to hand to prompts and bots directly.
#[derive(Debug, Clone, Default)]
pub struct BoardOccupancy {
    /// `(vertex, owner, is_city)`
    pub buildings: Vec<(VertexId, Seat, bool)>,
    /// `(edge, owner)`
    pub roads: Vec<(EdgeId, Seat)>,
}

#[derive(Debug, Clone)]
pub struct Observation {
    pub seat: Seat,
    pub hand: ResourceSet,
    pub devs_playable: Vec<DevCard>,
    pub devs_new: Vec<DevCard>,
    /// Own total VP including hidden VP cards.
    pub my_vp: u8,
    pub players: Vec<PublicPlayer>,
    pub bank: ResourceSet,
    pub dev_deck_remaining: u8,
    pub layout: BoardLayout,
    pub occupancy: BoardOccupancy,
    pub robber: HexId,
    pub turn: u32,
    pub active: Seat,
    pub phase: &'static str,
    pub longest_road: Option<(Seat, u8)>,
    pub largest_army: Option<(Seat, u8)>,
    /// Whether the active player has already played a dev card this turn
    /// (public — dev plays are table-visible).
    pub dev_played_this_turn: bool,
    /// Everything this seat legitimately witnessed, in order.
    pub history: Vec<EventRecord>,
}

impl Observation {
    /// Rebuild the dense occupancy arrays the legality helpers consume.
    pub fn buildings_array(&self) -> Vec<Option<(Seat, bool)>> {
        let mut b = vec![None; super::board::VERTEX_COUNT];
        for &(v, owner, is_city) in &self.occupancy.buildings {
            b[v as usize] = Some((owner, is_city));
        }
        b
    }

    pub fn roads_array(&self) -> Vec<Option<Seat>> {
        let mut r = vec![None; super::board::EDGE_COUNT];
        for &(e, owner) in &self.occupancy.roads {
            r[e as usize] = Some(owner);
        }
        r
    }

    /// Legal settlement sites for this seat right now (normal play).
    pub fn settlement_sites(&self) -> Vec<VertexId> {
        super::sites::settlement_sites(&self.buildings_array(), &self.roads_array(), self.seat)
    }

    /// Legal road edges for this seat right now.
    pub fn road_sites(&self) -> Vec<EdgeId> {
        super::sites::road_sites(&self.buildings_array(), &self.roads_array(), self.seat)
    }

    /// Open vertices under the distance rule (setup placements).
    pub fn setup_settlement_sites(&self) -> Vec<VertexId> {
        super::sites::setup_settlement_sites(&self.buildings_array())
    }

    /// Edges attached to a just-placed setup settlement.
    pub fn setup_road_sites(&self, settlement: VertexId) -> Vec<EdgeId> {
        super::sites::setup_road_sites(&self.roads_array(), settlement)
    }

    /// This seat's best bank rate for a resource (port-aware).
    pub fn bank_rate(&self, resource: super::types::Resource) -> u8 {
        use super::board::Port;
        let mut rate = 4;
        let buildings = self.buildings_array();
        for (v, b) in buildings.iter().enumerate() {
            if !matches!(b, Some((owner, _)) if *owner == self.seat) {
                continue;
            }
            match self.layout.port_at_vertex(v as VertexId) {
                Some(Port::Resource { resource: r }) if r == resource => return 2,
                Some(Port::Generic) => rate = rate.min(3),
                _ => {}
            }
        }
        rate
    }
}

impl GameState {
    pub fn observe(&self, seat: Seat) -> Observation {
        let me = &self.players[seat as usize];
        let mut occupancy = BoardOccupancy::default();
        for (v, b) in self.buildings.iter().enumerate() {
            if let Some((owner, is_city)) = b {
                occupancy.buildings.push((v as VertexId, *owner, *is_city));
            }
        }
        for (e, r) in self.roads.iter().enumerate() {
            if let Some(owner) = r {
                occupancy.roads.push((e as EdgeId, *owner));
            }
        }

        Observation {
            seat,
            hand: me.resources,
            devs_playable: me.devs_playable.clone(),
            devs_new: me.devs_new.clone(),
            my_vp: self.victory_points(seat, true),
            players: (0..PLAYER_COUNT)
                .map(|s| {
                    let p = &self.players[s as usize];
                    PublicPlayer {
                        seat: s,
                        public_vp: self.victory_points(s, false),
                        hand_count: p.resources.total(),
                        dev_count: p.dev_count(),
                        knights_played: p.knights_played,
                        roads_left: p.roads_left(),
                        settlements_left: p.settlements_left(),
                        cities_left: p.cities_left(),
                    }
                })
                .collect(),
            bank: self.bank,
            dev_deck_remaining: self.dev_deck.len() as u8,
            layout: self.layout.clone(),
            occupancy,
            robber: self.robber,
            turn: self.turn,
            active: self.active,
            phase: self.phase.name(),
            longest_road: self.longest_road,
            largest_army: self.largest_army,
            dev_played_this_turn: self.dev_played_this_turn,
            history: self
                .events
                .iter()
                .filter(|r| r.visibility.visible_to(seat))
                .cloned()
                .collect(),
        }
    }
}
