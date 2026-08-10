//! The Catan transcript: every observable fact as a visibility-tagged event.
//! Quantitative hiding: hand/dev contents ride in `Private` events; counts are
//! `Public`. Rendering lives in [`super::render`].

use super::board::{EdgeId, HexId, Port, VertexId};
use super::types::{DevCard, Resource, ResourceSet, Seat, Terrain};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::game_core::Visibility;

/// One transcript entry (the shared envelope carrying a Catan event payload).
pub type EventRecord = crate::game_core::EventRecord<CatanEvent>;

/// One hex of the laid board, with everything a renderer needs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HexSpec {
    pub hex: HexId,
    pub q: i8,
    pub r: i8,
    pub terrain: Terrain,
    pub number: Option<u8>,
    /// The hex's six vertices, clockwise from north.
    pub vertices: [VertexId; 6],
}

/// One port of the laid board.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PortSpec {
    pub edge: EdgeId,
    pub vertices: (VertexId, VertexId),
    pub port: Port,
}

/// The two sides of a proposed exchange, always from the proposer's
/// perspective: the proposer gives `give` and receives `receive`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TradeOffer {
    pub give: ResourceSet,
    pub receive: ResourceSet,
}

/// A responder's verdict on an offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TradeResponse {
    Accept,
    Reject,
    /// A counter-offer, again from the *original proposer's* perspective.
    Counter {
        offer: TradeOffer,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum CatanEvent {
    GameStarted {
        players: u8,
    },
    /// The full board, emitted once at game start (public — boards are open
    /// information).
    BoardLaid {
        hexes: Vec<HexSpec>,
        ports: Vec<PortSpec>,
        desert: HexId,
    },
    TurnStarted {
        seat: Seat,
        turn: u32,
    },
    SetupSettlementPlaced {
        seat: Seat,
        vertex: VertexId,
        round: u8,
    },
    SetupRoadPlaced {
        seat: Seat,
        edge: EdgeId,
    },
    /// Second-settlement starting resources.
    SetupResourcesGranted {
        seat: Seat,
        gained: ResourceSet,
    },
    DiceRolled {
        seat: Seat,
        d1: u8,
        d2: u8,
    },
    /// Per-seat production of one roll (public; empty gains omitted).
    ResourcesProduced {
        gains: Vec<(Seat, ResourceSet)>,
    },
    /// The bank could not pay `resource` to more than one claimant, so nobody
    /// received it this roll (official shortage rule).
    ResourceShortage {
        resource: Resource,
    },
    /// Seats over the hand limit after a 7 (public counts).
    MustDiscard {
        seats: Vec<(Seat, u8)>,
    },
    Discarded {
        seat: Seat,
        count: u8,
    },
    /// Private: what the seat actually discarded.
    DiscardedContents {
        seat: Seat,
        resources: ResourceSet,
    },
    RobberMoved {
        seat: Seat,
        hex: HexId,
    },
    /// Public: a card changed hands via the robber.
    CardStolen {
        from: Seat,
        to: Seat,
    },
    /// Private to each party: which card it was.
    CardStolenContents {
        from: Seat,
        to: Seat,
        resource: Resource,
    },
    RoadBuilt {
        seat: Seat,
        edge: EdgeId,
        /// Placed via Road Building (no cost).
        free: bool,
    },
    SettlementBuilt {
        seat: Seat,
        vertex: VertexId,
    },
    CityBuilt {
        seat: Seat,
        vertex: VertexId,
    },
    /// Public: a dev card was bought (count observable).
    DevCardBought {
        seat: Seat,
    },
    /// Private: which card was drawn.
    DevCardDrawn {
        seat: Seat,
        card: DevCard,
    },
    DevCardPlayed {
        seat: Seat,
        card: DevCard,
    },
    YearOfPlentyTaken {
        seat: Seat,
        first: Resource,
        second: Resource,
    },
    MonopolyResolved {
        seat: Seat,
        resource: Resource,
        taken: Vec<(Seat, u8)>,
    },
    BankTraded {
        seat: Seat,
        gave: ResourceSet,
        got: ResourceSet,
        rate: u8,
    },
    TradeProposed {
        seat: Seat,
        /// `None` = open offer to every other player.
        to: Option<Seat>,
        offer: TradeOffer,
        message: Option<String>,
    },
    TradeResponded {
        seat: Seat,
        response: TradeResponse,
        message: Option<String>,
    },
    /// An executed exchange: the proposer gave `offer.give` to `with` and
    /// received `offer.receive`.
    TradeExecuted {
        proposer: Seat,
        with: Seat,
        offer: TradeOffer,
    },
    TradeWindowClosed {
        seat: Seat,
    },
    /// Free-form table talk by the active player (always public).
    Said {
        seat: Seat,
        text: String,
    },
    LongestRoadClaimed {
        seat: Option<Seat>,
        length: u8,
        previous: Option<Seat>,
    },
    LargestArmyClaimed {
        seat: Seat,
        knights: u8,
        previous: Option<Seat>,
    },
    /// An agent exhausted its retry budget; the engine applied the
    /// deterministic legal default (metric-exempt, public).
    ForcedDefault {
        seat: Seat,
        decision: String,
    },
    GameEnded {
        winner: Seat,
        /// Final total VP per seat, hidden VP cards included.
        vps: Vec<u8>,
        turns: u32,
    },
}
