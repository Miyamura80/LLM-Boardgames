//! The fixed golden event set: one instance of every event variant, hashed
//! into the scaffold version so any render-wording edit moves the scaffold id.

use crate::catan::events::TradeOffer;

/// A deterministic set of events covering every render arm.
pub(super) fn sample_events() -> Vec<crate::catan::events::CatanEvent> {
    use crate::catan::events::{CatanEvent as E, TradeResponse};
    use crate::catan::types::{DevCard, Resource, ResourceSet};
    let set = ResourceSet::of(Resource::Brick, 2);
    let offer = TradeOffer {
        give: set,
        receive: ResourceSet::of(Resource::Ore, 1),
    };
    vec![
        E::GameStarted { players: 4 },
        E::TurnStarted { seat: 0, turn: 1 },
        E::SetupSettlementPlaced {
            seat: 0,
            vertex: 1,
            round: 0,
        },
        E::SetupRoadPlaced { seat: 0, edge: 2 },
        E::SetupResourcesGranted {
            seat: 0,
            gained: set,
        },
        E::DiceRolled {
            seat: 0,
            d1: 3,
            d2: 4,
        },
        E::ResourcesProduced {
            gains: vec![(0, set)],
        },
        E::ResourceShortage {
            resource: Resource::Ore,
        },
        E::MustDiscard {
            seats: vec![(1, 4)],
        },
        E::Discarded { seat: 1, count: 4 },
        E::DiscardedContents {
            seat: 1,
            resources: set,
        },
        E::RobberMoved { seat: 0, hex: 3 },
        E::CardStolen { from: 1, to: 0 },
        E::CardStolenContents {
            from: 1,
            to: 0,
            resource: Resource::Wool,
        },
        E::RoadBuilt {
            seat: 0,
            edge: 5,
            free: false,
        },
        E::RoadBuilt {
            seat: 0,
            edge: 6,
            free: true,
        },
        E::SettlementBuilt { seat: 0, vertex: 9 },
        E::CityBuilt { seat: 0, vertex: 9 },
        E::DevCardBought { seat: 0 },
        E::DevCardDrawn {
            seat: 0,
            card: DevCard::Knight,
        },
        E::DevCardPlayed {
            seat: 0,
            card: DevCard::Monopoly,
        },
        E::YearOfPlentyTaken {
            seat: 0,
            first: Resource::Grain,
            second: Resource::Ore,
        },
        E::MonopolyResolved {
            seat: 0,
            resource: Resource::Wool,
            taken: vec![(1, 2), (2, 1)],
        },
        E::BankTraded {
            seat: 0,
            gave: ResourceSet::of(Resource::Brick, 4),
            got: ResourceSet::of(Resource::Ore, 1),
            rate: 4,
        },
        E::TradeProposed {
            seat: 0,
            to: Some(1),
            offer,
            message: Some("deal?".into()),
        },
        E::TradeResponded {
            seat: 1,
            response: TradeResponse::Accept,
            message: Some("ok".into()),
        },
        E::TradeResponded {
            seat: 2,
            response: TradeResponse::Reject,
            message: None,
        },
        E::TradeResponded {
            seat: 3,
            response: TradeResponse::Counter { offer },
            message: None,
        },
        E::TradeExecuted {
            proposer: 0,
            with: 1,
            offer,
        },
        E::TradeWindowClosed { seat: 0 },
        E::Said {
            seat: 2,
            text: "no more ore for P0".into(),
        },
        E::LongestRoadClaimed {
            seat: Some(1),
            length: 5,
            previous: None,
        },
        E::LongestRoadClaimed {
            seat: Some(2),
            length: 6,
            previous: Some(1),
        },
        E::LongestRoadClaimed {
            seat: None,
            length: 4,
            previous: Some(2),
        },
        E::LargestArmyClaimed {
            seat: 3,
            knights: 3,
            previous: None,
        },
        E::ForcedDefault {
            seat: 1,
            decision: "turn".into(),
        },
        E::GameEnded {
            winner: 0,
            vps: vec![10, 7, 6, 5],
            turns: 61,
        },
    ]
}
