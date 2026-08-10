//! Core Catan value types: resources, development cards, piece limits, and the
//! per-player state. The 4-player base game only (PRD-catan-evals §2).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::game_core::Seat;

pub const PLAYER_COUNT: u8 = 4;
pub const VP_TO_WIN: u8 = 10;
pub const MAX_ROADS: u8 = 15;
pub const MAX_SETTLEMENTS: u8 = 5;
pub const MAX_CITIES: u8 = 4;
/// Hand size above which a rolled 7 forces a discard of half (rounded down).
pub const DISCARD_LIMIT: u8 = 7;
pub const LONGEST_ROAD_MIN: u8 = 5;
pub const LARGEST_ARMY_MIN: u8 = 3;
/// Each resource's starting bank supply.
pub const BANK_PER_RESOURCE: u8 = 19;

/// The five producible resources, in canonical order (indexes [`ResourceSet`]).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    Brick,
    Lumber,
    Wool,
    Grain,
    Ore,
}

pub const RESOURCES: [Resource; 5] = [
    Resource::Brick,
    Resource::Lumber,
    Resource::Wool,
    Resource::Grain,
    Resource::Ore,
];

impl Resource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Resource::Brick => "brick",
            Resource::Lumber => "lumber",
            Resource::Wool => "wool",
            Resource::Grain => "grain",
            Resource::Ore => "ore",
        }
    }
}

/// What a hex produces. `Desert` produces nothing and hosts the robber first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Terrain {
    Hills,
    Forest,
    Pasture,
    Fields,
    Mountains,
    Desert,
}

impl Terrain {
    pub fn resource(&self) -> Option<Resource> {
        match self {
            Terrain::Hills => Some(Resource::Brick),
            Terrain::Forest => Some(Resource::Lumber),
            Terrain::Pasture => Some(Resource::Wool),
            Terrain::Fields => Some(Resource::Grain),
            Terrain::Mountains => Some(Resource::Ore),
            Terrain::Desert => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Terrain::Hills => "hills",
            Terrain::Forest => "forest",
            Terrain::Pasture => "pasture",
            Terrain::Fields => "fields",
            Terrain::Mountains => "mountains",
            Terrain::Desert => "desert",
        }
    }
}

/// A multiset of resource cards. Indexed by [`Resource`]'s canonical order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResourceSet {
    pub brick: u8,
    pub lumber: u8,
    pub wool: u8,
    pub grain: u8,
    pub ore: u8,
}

impl ResourceSet {
    pub fn of(resource: Resource, n: u8) -> Self {
        let mut s = Self::default();
        s.add(resource, n);
        s
    }

    pub fn get(&self, r: Resource) -> u8 {
        match r {
            Resource::Brick => self.brick,
            Resource::Lumber => self.lumber,
            Resource::Wool => self.wool,
            Resource::Grain => self.grain,
            Resource::Ore => self.ore,
        }
    }

    pub fn get_mut(&mut self, r: Resource) -> &mut u8 {
        match r {
            Resource::Brick => &mut self.brick,
            Resource::Lumber => &mut self.lumber,
            Resource::Wool => &mut self.wool,
            Resource::Grain => &mut self.grain,
            Resource::Ore => &mut self.ore,
        }
    }

    pub fn add(&mut self, r: Resource, n: u8) {
        *self.get_mut(r) += n;
    }

    /// Remove `n` of `r`; false (unchanged) if not held.
    pub fn remove(&mut self, r: Resource, n: u8) -> bool {
        let slot = self.get_mut(r);
        if *slot < n {
            return false;
        }
        *slot -= n;
        true
    }

    pub fn total(&self) -> u8 {
        self.brick + self.lumber + self.wool + self.grain + self.ore
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    pub fn contains(&self, other: &ResourceSet) -> bool {
        RESOURCES.iter().all(|&r| self.get(r) >= other.get(r))
    }

    /// Add every card in `other`.
    pub fn add_set(&mut self, other: &ResourceSet) {
        for r in RESOURCES {
            self.add(r, other.get(r));
        }
    }

    /// Remove every card in `other`; false (unchanged) if any is short.
    pub fn remove_set(&mut self, other: &ResourceSet) -> bool {
        if !self.contains(other) {
            return false;
        }
        for r in RESOURCES {
            self.remove(r, other.get(r));
        }
        true
    }

    /// Compact human/prompt rendering, e.g. `2 brick, 1 ore` (or `nothing`).
    pub fn describe(&self) -> String {
        let parts: Vec<String> = RESOURCES
            .iter()
            .filter(|&&r| self.get(r) > 0)
            .map(|&r| format!("{} {}", self.get(r), r.as_str()))
            .collect();
        if parts.is_empty() {
            "nothing".into()
        } else {
            parts.join(", ")
        }
    }
}

/// Development cards. 25-card deck: 14 Knights, 5 VPs, 2 of each progress card.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DevCard {
    Knight,
    VictoryPoint,
    RoadBuilding,
    YearOfPlenty,
    Monopoly,
}

impl DevCard {
    pub fn as_str(&self) -> &'static str {
        match self {
            DevCard::Knight => "knight",
            DevCard::VictoryPoint => "victory_point",
            DevCard::RoadBuilding => "road_building",
            DevCard::YearOfPlenty => "year_of_plenty",
            DevCard::Monopoly => "monopoly",
        }
    }
}

/// The full 25-card development deck, pre-shuffle.
pub fn dev_deck_unshuffled() -> Vec<DevCard> {
    let mut deck = Vec::with_capacity(25);
    deck.extend(std::iter::repeat_n(DevCard::Knight, 14));
    deck.extend(std::iter::repeat_n(DevCard::VictoryPoint, 5));
    deck.extend(std::iter::repeat_n(DevCard::RoadBuilding, 2));
    deck.extend(std::iter::repeat_n(DevCard::YearOfPlenty, 2));
    deck.extend(std::iter::repeat_n(DevCard::Monopoly, 2));
    deck
}

/// Build costs.
pub fn road_cost() -> ResourceSet {
    ResourceSet {
        brick: 1,
        lumber: 1,
        ..Default::default()
    }
}

pub fn settlement_cost() -> ResourceSet {
    ResourceSet {
        brick: 1,
        lumber: 1,
        wool: 1,
        grain: 1,
        ..Default::default()
    }
}

pub fn city_cost() -> ResourceSet {
    ResourceSet {
        grain: 2,
        ore: 3,
        ..Default::default()
    }
}

pub fn dev_cost() -> ResourceSet {
    ResourceSet {
        wool: 1,
        grain: 1,
        ore: 1,
        ..Default::default()
    }
}

/// One seat's holdings.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PlayerState {
    pub resources: ResourceSet,
    /// Unplayed dev cards bought on a previous turn (playable).
    pub devs_playable: Vec<DevCard>,
    /// Dev cards bought this turn (unplayable until next turn; VP cards still
    /// count toward the win check).
    pub devs_new: Vec<DevCard>,
    pub knights_played: u8,
    pub roads_placed: u8,
    pub settlements: Vec<u8>,
    pub cities: Vec<u8>,
}

impl PlayerState {
    pub fn dev_count(&self) -> u8 {
        (self.devs_playable.len() + self.devs_new.len()) as u8
    }

    pub fn vp_cards(&self) -> u8 {
        self.devs_playable
            .iter()
            .chain(self.devs_new.iter())
            .filter(|&&d| d == DevCard::VictoryPoint)
            .count() as u8
    }

    pub fn roads_left(&self) -> u8 {
        MAX_ROADS - self.roads_placed
    }

    pub fn settlements_left(&self) -> u8 {
        MAX_SETTLEMENTS - self.settlements.len() as u8
    }

    pub fn cities_left(&self) -> u8 {
        MAX_CITIES - self.cities.len() as u8
    }
}
