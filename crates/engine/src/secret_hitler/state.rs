//! Authoritative game state for a single 7-player game.

use super::deck::Deck;
use super::events::{EventRecord, GameEvent, Visibility};
use super::types::*;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    pub role: Role,
    pub alive: bool,
}

/// Phase of the current government round. The engine is a state machine: the
/// runner asks for pending decisions, collects agent actions, and applies them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Phase {
    /// The President must nominate a Chancellor.
    Nomination,
    /// Simultaneous ballots are being collected (hidden until all are in).
    Election {
        nominee: Seat,
        votes: BTreeMap<Seat, bool>,
    },
    /// The President holds 3 tiles and must discard 1.
    LegislativePresident {
        tiles: Vec<Party>,
    },
    /// The Chancellor holds 2 tiles and must enact 1 (or propose a veto once,
    /// when unlocked).
    LegislativeChancellor {
        tiles: Vec<Party>,
        veto_proposed: bool,
    },
    /// The President must approve or decline the Chancellor's veto.
    VetoConsent {
        tiles: Vec<Party>,
    },
    /// The President must use a granted power.
    ExecutiveAction {
        power: Power,
    },
    GameOver,
}

impl Phase {
    /// Short kebab name for logs, errors, and observations.
    pub fn name(&self) -> &'static str {
        match self {
            Phase::Nomination => "nomination",
            Phase::Election { .. } => "election",
            Phase::LegislativePresident { .. } => "legislative-president",
            Phase::LegislativeChancellor { .. } => "legislative-chancellor",
            Phase::VetoConsent { .. } => "veto-consent",
            Phase::ExecutiveAction { .. } => "executive-action",
            Phase::GameOver => "game-over",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub seed: u64,
    pub(super) rng: ChaCha8Rng,
    pub players: Vec<PlayerState>,
    pub deck: Deck,
    pub liberal_policies: u8,
    pub fascist_policies: u8,
    pub election_tracker: u8,
    /// Seat of the current Presidential candidate.
    pub presidency: Seat,
    /// Where normal rotation resumes after a Special Election round.
    pub special_return: Option<Seat>,
    /// Term limits: the last **elected** President and Chancellor.
    pub term_limited_president: Option<Seat>,
    pub term_limited_chancellor: Option<Seat>,
    /// Seats already investigated (a player may only be investigated once).
    pub investigated: Vec<Seat>,
    /// The sitting government while a legislative session is in progress.
    pub gov_chancellor: Option<Seat>,
    pub phase: Phase,
    pub round: u32,
    pub winner: Option<WinCondition>,
    pub events: Vec<EventRecord>,
}

impl GameState {
    /// Deal roles and shuffle the deck from a single seed. Every downstream
    /// random draw (deck order, reshuffles, forced-default tile picks) flows
    /// from this RNG, so a seed fully determines the game given agent actions.
    pub fn new(seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);

        let mut roles = vec![Role::Liberal; LIBERAL_COUNT as usize];
        roles.extend(vec![Role::Fascist; REGULAR_FASCIST_COUNT as usize]);
        roles.push(Role::Hitler);
        roles.shuffle(&mut rng);
        Self::build(seed, rng, roles)
    }

    /// Build a game with a caller-chosen role arrangement (the controlled
    /// scheduler forces the candidate's role-seat cell); the deck still
    /// shuffles from the seed, so mirrored-seed games share deck order.
    pub fn with_roles(seed: u64, roles: [Role; PLAYER_COUNT as usize]) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        // Consume the same number of RNG draws a role shuffle would, keeping
        // deck order identical between `new` and `with_roles` for one seed.
        let mut scratch: Vec<u8> = (0..PLAYER_COUNT).collect();
        scratch.shuffle(&mut rng);
        Self::build(seed, rng, roles.to_vec())
    }

    fn build(seed: u64, mut rng: ChaCha8Rng, roles: Vec<Role>) -> Self {
        debug_assert_eq!(
            roles.iter().filter(|r| **r == Role::Hitler).count(),
            1,
            "exactly one Hitler"
        );

        let deck = Deck::new(&mut rng);

        let mut state = Self {
            seed,
            rng,
            players: roles
                .iter()
                .map(|&role| PlayerState { role, alive: true })
                .collect(),
            deck,
            liberal_policies: 0,
            fascist_policies: 0,
            election_tracker: 0,
            // Seat 0 always opens; the match scheduler rotates candidates
            // through seats, which is what controls position effects.
            presidency: 0,
            special_return: None,
            term_limited_president: None,
            term_limited_chancellor: None,
            investigated: Vec::new(),
            gov_chancellor: None,
            phase: Phase::Nomination,
            round: 1,
            winner: None,
            events: Vec::new(),
        };

        state.push_event(
            Visibility::Public,
            GameEvent::GameStarted {
                players: PLAYER_COUNT,
            },
        );
        state.deal_knowledge();
        state
    }

    /// Emit each seat's private role knowledge. 7-player knowledge graph:
    /// regular Fascists know each other and Hitler; **Hitler knows nobody**;
    /// Liberals know nobody.
    fn deal_knowledge(&mut self) {
        for seat in 0..PLAYER_COUNT {
            let role = self.players[seat as usize].role;
            let known_teammates: Vec<(Seat, Role)> = match role {
                Role::Fascist => (0..PLAYER_COUNT)
                    .filter(|&s| s != seat)
                    .filter_map(|s| {
                        let r = self.players[s as usize].role;
                        matches!(r, Role::Fascist | Role::Hitler).then_some((s, r))
                    })
                    .collect(),
                Role::Hitler | Role::Liberal => Vec::new(),
            };
            self.push_event(
                Visibility::Private(seat),
                GameEvent::RolesDealt {
                    seat,
                    role,
                    known_teammates,
                },
            );
        }
    }

    /// Re-emit per-seat knowledge after a scripted role rewrite (testkit).
    pub(super) fn redeal_knowledge_for_test(&mut self) {
        self.deal_knowledge();
    }

    // -- event log ----------------------------------------------------------

    pub(super) fn push_event(&mut self, visibility: Visibility, event: GameEvent) {
        self.events.push(EventRecord {
            idx: self.events.len() as u32,
            round: self.round,
            visibility,
            event,
        });
    }

    /// Append one revealed discussion utterance (runner-managed phase; the
    /// engine only records it so observations can replay it).
    pub fn add_utterance(&mut self, seat: Seat, discussion_round: u8, text: String, pass: bool) {
        self.push_event(
            Visibility::Public,
            GameEvent::Utterance {
                seat,
                discussion_round,
                text,
                pass,
            },
        );
    }

    /// Record that a seat's decision fell through to the forced legal default
    /// (public, for transcript auditability).
    pub fn push_forced_default(&mut self, seat: Seat, decision: &str) {
        self.push_event(
            Visibility::Public,
            GameEvent::ForcedDefault {
                seat,
                decision: decision.to_string(),
            },
        );
    }

    // -- seat helpers ---------------------------------------------------------

    pub fn alive_seats(&self) -> Vec<Seat> {
        (0..PLAYER_COUNT)
            .filter(|&s| self.players[s as usize].alive)
            .collect()
    }

    pub fn alive_count(&self) -> u8 {
        self.alive_seats().len() as u8
    }

    pub fn is_alive(&self, seat: Seat) -> bool {
        self.players[seat as usize].alive
    }

    pub fn hitler_seat(&self) -> Seat {
        (0..PLAYER_COUNT)
            .find(|&s| self.players[s as usize].role == Role::Hitler)
            .expect("a game always has Hitler")
    }

    /// The next living seat clockwise after `seat`.
    pub fn next_alive_after(&self, seat: Seat) -> Seat {
        let mut s = seat;
        loop {
            s = (s + 1) % PLAYER_COUNT;
            if self.players[s as usize].alive {
                return s;
            }
        }
    }

    /// Seats eligible for Chancellor nomination: alive, not the President, and
    /// not term-limited. The last elected President is exempt from the limit
    /// once only 5 or fewer players remain.
    pub fn eligible_chancellors(&self) -> Vec<Seat> {
        let relax = self.alive_count() <= 5;
        self.alive_seats()
            .into_iter()
            .filter(|&s| s != self.presidency)
            .filter(|&s| Some(s) != self.term_limited_chancellor)
            .filter(|&s| relax || Some(s) != self.term_limited_president)
            .collect()
    }

    /// Legal targets for the pending executive power.
    pub fn power_targets(&self, power: Power) -> Vec<Seat> {
        self.alive_seats()
            .into_iter()
            .filter(|&s| s != self.presidency)
            .filter(|&s| power != Power::InvestigateLoyalty || !self.investigated.contains(&s))
            .collect()
    }

    /// Advance the presidency to the next round's candidate and reset the
    /// per-round government bookkeeping.
    pub(super) fn advance_presidency(&mut self) {
        self.gov_chancellor = None;
        self.round += 1;
        self.presidency = match self.special_return.take() {
            // Normal order resumes after a Special Election. The stored seat
            // may have been executed during the special round; walk from the
            // seat *before* it so a dead return-seat resolves to the next
            // living player.
            Some(return_seat) => {
                if self.is_alive(return_seat) {
                    return_seat
                } else {
                    self.next_alive_after(return_seat)
                }
            }
            None => self.next_alive_after(self.presidency),
        };
        self.phase = Phase::Nomination;
    }

    pub(super) fn end_game(&mut self, condition: WinCondition) {
        self.winner = Some(condition);
        self.phase = Phase::GameOver;
        self.push_event(
            Visibility::Public,
            GameEvent::GameEnded {
                winner: condition.winner(),
                condition,
                roles: self.players.iter().map(|p| p.role).collect(),
            },
        );
    }

    pub fn is_over(&self) -> bool {
        self.winner.is_some()
    }
}
