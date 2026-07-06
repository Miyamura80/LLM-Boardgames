//! Role-conditioned observations: the only game view agents ever receive.
//!
//! An [`Observation`] is produced per seat by filtering the transcript by
//! visibility, so a seat structurally cannot see another seat's private
//! knowledge (deck order, others' drawn tiles, another President's
//! investigation results, other roles — and at 7 players, Hitler sees no
//! teammates).

use super::events::EventRecord;
use super::state::{GameState, Phase};
use super::types::{Party, Role, Seat};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Public board state every seat may see.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PublicState {
    pub round: u32,
    pub liberal_policies: u8,
    pub fascist_policies: u8,
    pub election_tracker: u8,
    pub deck_count: u8,
    pub discard_count: u8,
    pub president: Seat,
    pub chancellor: Option<Seat>,
    pub alive: Vec<Seat>,
    pub veto_unlocked: bool,
    pub phase: String,
}

/// Everything one seat is entitled to know.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Observation {
    pub seat: Seat,
    pub role: Role,
    /// Party membership printed on this seat's card (Hitler reads Fascist).
    pub party: Party,
    /// Teammates this seat knows (regular Fascists only, at 7 players).
    pub known_teammates: Vec<(Seat, Role)>,
    pub public: PublicState,
    /// The full action/discussion history this seat legitimately witnessed,
    /// in transcript order.
    pub history: Vec<EventRecord>,
}

impl GameState {
    /// Build the role-conditioned view for `seat`.
    pub fn observe(&self, seat: Seat) -> Observation {
        let role = self.players[seat as usize].role;
        let known_teammates = self
            .events
            .iter()
            .find_map(|e| match &e.event {
                super::events::GameEvent::RolesDealt {
                    seat: s,
                    known_teammates,
                    ..
                } if *s == seat => Some(known_teammates.clone()),
                _ => None,
            })
            .unwrap_or_default();

        Observation {
            seat,
            role,
            party: role.party(),
            known_teammates,
            public: PublicState {
                round: self.round,
                liberal_policies: self.liberal_policies,
                fascist_policies: self.fascist_policies,
                election_tracker: self.election_tracker,
                deck_count: self.deck.draw_count() as u8,
                discard_count: self.deck.discard_count() as u8,
                president: self.presidency,
                chancellor: self.gov_chancellor,
                alive: self.alive_seats(),
                veto_unlocked: self.veto_unlocked(),
                phase: phase_label(&self.phase).to_string(),
            },
            history: self
                .events
                .iter()
                .filter(|e| e.visibility.visible_to(seat))
                .cloned()
                .collect(),
        }
    }
}

fn phase_label(phase: &Phase) -> &'static str {
    match phase {
        Phase::Nomination => "nomination",
        Phase::Election { .. } => "election",
        Phase::LegislativePresident { .. } => "legislative-president",
        Phase::LegislativeChancellor { .. } => "legislative-chancellor",
        Phase::VetoConsent { .. } => "veto-consent",
        Phase::ExecutiveAction { .. } => "executive-action",
        Phase::GameOver => "game-over",
    }
}
