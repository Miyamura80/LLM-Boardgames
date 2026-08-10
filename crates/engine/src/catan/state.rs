//! The engine-authoritative game state: the god-object every rule reads and
//! `apply` mutates. Randomness is split into independent seeded streams so
//! controlled schedules can mirror boards, dev decks, and dice across
//! candidates regardless of how play diverges.

use super::board::{graph, BoardLayout, EdgeId, HexId, VertexId, EDGE_COUNT, VERTEX_COUNT};
use super::events::{CatanEvent, EventRecord, HexSpec, PortSpec, TradeOffer, TradeResponse};
use super::types::*;
use crate::game_core::Visibility;
#[allow(unused_imports)]
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Per-turn cost bounds (config-driven; defaults here keep tests hermetic).
#[derive(Debug, Clone)]
pub struct TurnCaps {
    /// Actions per turn before `end_turn` is forced.
    pub actions: u32,
    /// Player-trade windows the active player may open per turn.
    pub trade_windows: u8,
    /// Free-form `say` actions per turn.
    pub says: u8,
    /// Hard game-length cap: past this turn the game ends and the highest
    /// total VP wins (ties break to the earlier seat). A backstop against
    /// stalled all-bot games, far above natural game length.
    pub max_turns: u32,
}

impl Default for TurnCaps {
    fn default() -> Self {
        Self {
            actions: 12,
            trade_windows: 3,
            says: 2,
            max_turns: 200,
        }
    }
}

/// Where the game is. Decision points derive from this (see `actions.rs`).
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Snake-draft setup: placement index 0..8 (settlement then road each);
    /// `road_for` holds the just-placed settlement awaiting its road.
    Setup {
        placements: u8,
        road_for: Option<VertexId>,
    },
    /// Active player may play a Knight before rolling.
    PreRoll,
    /// A 7 was rolled; every listed seat must discard (simultaneous).
    Discard {
        pending: Vec<(Seat, u8)>,
    },
    MoveRobber,
    /// More than one robber victim is adjacent — the active player chooses.
    ChooseVictim {
        victims: Vec<Seat>,
    },
    /// The post-roll action loop.
    Turn,
    /// Free road placements from Road Building.
    FreeRoad,
    /// An offer is out; listed seats respond in seat order.
    TradeResponse {
        offer: TradeOffer,
        open: bool,
        to_respond: Vec<Seat>,
        responses: Vec<(Seat, TradeResponse)>,
    },
    /// All responses are in; the proposer picks a partner/counter or cancels.
    ResolveTrade {
        offer: TradeOffer,
        accepters: Vec<Seat>,
        counters: Vec<(Seat, TradeOffer)>,
    },
    GameOver,
}

impl Phase {
    pub fn name(&self) -> &'static str {
        match self {
            Phase::Setup { .. } => "setup",
            Phase::PreRoll => "pre-roll",
            Phase::Discard { .. } => "discard",
            Phase::MoveRobber => "move-robber",
            Phase::ChooseVictim { .. } => "choose-victim",
            Phase::Turn => "turn",
            Phase::FreeRoad => "free-road",
            Phase::TradeResponse { .. } => "trade-response",
            Phase::ResolveTrade { .. } => "resolve-trade",
            Phase::GameOver => "game-over",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameState {
    pub seed: u64,
    /// General play-time randomness (forced-default picks).
    pub rng: ChaCha8Rng,
    /// The dice stream — independent of play, so seeds mirror dice sequences.
    pub rng_dice: ChaCha8Rng,
    /// Robber steal picks — separate so a steal never perturbs the dice.
    pub rng_steal: ChaCha8Rng,
    pub layout: BoardLayout,
    pub players: Vec<PlayerState>,
    pub bank: ResourceSet,
    pub dev_deck: Vec<DevCard>,
    pub robber: HexId,
    /// Per-vertex building: `(owner, is_city)`.
    pub buildings: Vec<Option<(Seat, bool)>>,
    /// Per-edge road owner.
    pub roads: Vec<Option<Seat>>,
    /// Turn counter (0 during setup, then 1, 2, …).
    pub turn: u32,
    pub active: Seat,
    pub phase: Phase,
    pub longest_road: Option<(Seat, u8)>,
    pub largest_army: Option<(Seat, u8)>,
    pub actions_this_turn: u32,
    pub trade_windows_this_turn: u8,
    pub says_this_turn: u8,
    pub free_roads: u8,
    /// At most one non-VP dev card per turn.
    pub dev_played_this_turn: bool,
    /// Whether the active player has rolled yet this turn.
    pub rolled: bool,
    /// Testkit override: pre-scripted dice consumed before the dice stream.
    pub scripted_dice: std::collections::VecDeque<(u8, u8)>,
    pub winner: Option<Seat>,
    pub caps: TurnCaps,
    pub events: Vec<EventRecord>,
}

impl GameState {
    pub fn new(seed: u64) -> Self {
        Self::with_caps(seed, TurnCaps::default())
    }

    pub fn with_caps(seed: u64, caps: TurnCaps) -> Self {
        // Construction-time randomness (board, dev deck) is its own stream so
        // play-time draws can never shift it.
        let mut rng_setup = ChaCha8Rng::seed_from_u64(seed ^ 0xB0A2_D5E7_1A2B_3C4D);
        let layout = BoardLayout::generate(&mut rng_setup);
        let mut dev_deck = dev_deck_unshuffled();
        use rand::seq::SliceRandom;
        dev_deck.shuffle(&mut rng_setup);

        let robber = layout.desert;
        let mut state = Self {
            seed,
            rng: ChaCha8Rng::seed_from_u64(seed ^ 0x6E4E_11AA_22BB_33CC),
            rng_dice: ChaCha8Rng::seed_from_u64(seed ^ 0xD1CE_D1CE_D1CE_D1CE),
            rng_steal: ChaCha8Rng::seed_from_u64(seed ^ 0x57EA_1057_EA10_57EA),
            layout,
            players: (0..PLAYER_COUNT).map(|_| PlayerState::default()).collect(),
            bank: ResourceSet {
                brick: BANK_PER_RESOURCE,
                lumber: BANK_PER_RESOURCE,
                wool: BANK_PER_RESOURCE,
                grain: BANK_PER_RESOURCE,
                ore: BANK_PER_RESOURCE,
            },
            dev_deck,
            robber,
            buildings: vec![None; VERTEX_COUNT],
            roads: vec![None; EDGE_COUNT],
            turn: 0,
            active: 0,
            phase: Phase::Setup {
                placements: 0,
                road_for: None,
            },
            longest_road: None,
            largest_army: None,
            actions_this_turn: 0,
            trade_windows_this_turn: 0,
            says_this_turn: 0,
            free_roads: 0,
            dev_played_this_turn: false,
            rolled: false,
            scripted_dice: std::collections::VecDeque::new(),
            winner: None,
            caps,
            events: Vec::new(),
        };

        state.push_event(
            Visibility::Public,
            CatanEvent::GameStarted {
                players: PLAYER_COUNT,
            },
        );
        state.push_event(Visibility::Public, state.board_laid_event());
        state
    }

    pub(crate) fn board_laid_event(&self) -> CatanEvent {
        let g = graph();
        let hexes = (0..super::board::HEX_COUNT)
            .map(|h| HexSpec {
                hex: h as HexId,
                q: super::board::HEX_COORDS[h].0,
                r: super::board::HEX_COORDS[h].1,
                terrain: self.layout.terrains[h],
                number: self.layout.numbers[h],
                vertices: g.hex_vertices[h],
            })
            .collect();
        let ports = self
            .layout
            .ports
            .iter()
            .map(|&(edge, port)| PortSpec {
                edge,
                vertices: g.edge_ends(edge),
                port,
            })
            .collect();
        CatanEvent::BoardLaid {
            hexes,
            ports,
            desert: self.layout.desert,
        }
    }

    pub fn push_event(&mut self, visibility: Visibility, event: CatanEvent) {
        self.events.push(EventRecord {
            idx: self.events.len() as u32,
            round: self.turn,
            visibility,
            event,
        });
    }

    pub fn push_forced_default(&mut self, seat: Seat, decision: &str) {
        self.push_event(
            Visibility::Public,
            CatanEvent::ForcedDefault {
                seat,
                decision: decision.to_string(),
            },
        );
    }

    pub fn is_over(&self) -> bool {
        self.winner.is_some()
    }

    /// Snake-draft seat for setup placement index 0..7: 0,1,2,3,3,2,1,0.
    pub fn setup_seat(placements: u8) -> Seat {
        if placements < PLAYER_COUNT {
            placements
        } else {
            2 * PLAYER_COUNT - 1 - placements
        }
    }

    /// Total victory points for a seat. `include_hidden` adds unplayed VP
    /// cards (the seat's own view and the win check see them; opponents don't).
    pub fn victory_points(&self, seat: Seat, include_hidden: bool) -> u8 {
        let p = &self.players[seat as usize];
        let mut vp = p.settlements.len() as u8 + 2 * p.cities.len() as u8;
        if self.longest_road.map(|(s, _)| s) == Some(seat) {
            vp += 2;
        }
        if self.largest_army.map(|(s, _)| s) == Some(seat) {
            vp += 2;
        }
        if include_hidden {
            vp += p.vp_cards();
        }
        vp
    }

    /// Win check — only the active player can win, on their own turn.
    pub fn check_win(&mut self) {
        if self.winner.is_none()
            && self.phase != Phase::GameOver
            && self.victory_points(self.active, true) >= VP_TO_WIN
        {
            let winner = self.active;
            let vps: Vec<u8> = (0..PLAYER_COUNT)
                .map(|s| self.victory_points(s, true))
                .collect();
            self.winner = Some(winner);
            self.phase = Phase::GameOver;
            self.push_event(
                Visibility::Public,
                CatanEvent::GameEnded {
                    winner,
                    vps,
                    turns: self.turn,
                },
            );
        }
    }

    // ---- Placement legality (shared with Observation via `sites`) ----------

    pub fn vertex_open_for_settlement(&self, v: VertexId) -> bool {
        super::sites::vertex_open_for_settlement(&self.buildings, v)
    }

    pub fn settlement_sites(&self, seat: Seat) -> Vec<VertexId> {
        super::sites::settlement_sites(&self.buildings, &self.roads, seat)
    }

    pub fn road_sites(&self, seat: Seat) -> Vec<EdgeId> {
        super::sites::road_sites(&self.buildings, &self.roads, seat)
    }

    pub fn setup_settlement_sites(&self) -> Vec<VertexId> {
        super::sites::setup_settlement_sites(&self.buildings)
    }

    pub fn setup_road_sites(&self, settlement: VertexId) -> Vec<EdgeId> {
        super::sites::setup_road_sites(&self.roads, settlement)
    }

    /// The bank trade rate for a seat and resource: 2 with its 2:1 port, 3
    /// with any 3:1 port, else 4.
    pub fn bank_rate(&self, seat: Seat, resource: Resource) -> u8 {
        use super::board::Port;
        let mut rate = 4;
        for &v in self.players[seat as usize]
            .settlements
            .iter()
            .chain(self.players[seat as usize].cities.iter())
        {
            match self.layout.port_at_vertex(v) {
                Some(Port::Resource { resource: r }) if r == resource => return 2,
                Some(Port::Generic) => rate = rate.min(3),
                _ => {}
            }
        }
        rate
    }

    /// Seats adjacent to a hex with at least one resource card (robber
    /// victims), excluding the thief.
    pub fn robber_victims(&self, thief: Seat, hex: HexId) -> Vec<Seat> {
        let g = graph();
        let mut victims: Vec<Seat> = Vec::new();
        for &v in &g.hex_vertices[hex as usize] {
            if let Some((owner, _)) = self.buildings[v as usize] {
                if owner != thief
                    && !self.players[owner as usize].resources.is_empty()
                    && !victims.contains(&owner)
                {
                    victims.push(owner);
                }
            }
        }
        victims
    }
}
