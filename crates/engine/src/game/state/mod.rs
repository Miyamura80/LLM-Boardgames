//! The authoritative game state and its transition function.
//!
//! `GameState` is the single source of truth (FR-1). It is a strict state
//! machine: [`GameState::current_decision`] exposes the one pending decision,
//! [`GameState::apply`] validates and applies an [`Action`], and every mutation
//! emits [`Event`]s into the log. All randomness flows through the seeded
//! [`Rng`], so a game is reproducible from its seed given the same decisions
//! (US-002).

use crate::game::action::{Action, Decision, DecisionKind};
use crate::game::board::{Board, Power};
use crate::game::config::{GameConfig, NUM_PLAYERS};
use crate::game::log::{Event, GameLog, WinReason};
use crate::game::observation::{KnownTeam, Observation, PlayerView};
use crate::game::policy::{Deck, Policy};
use crate::game::rng::Rng;
use crate::game::roles::{Faction, Party, Role, SEVEN_PLAYER_ROLES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod transitions;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub seat: usize,
    pub role: Role,
    pub alive: bool,
}

/// Internal phase — richer than [`DecisionKind`] because it carries hidden data
/// (drawn tiles, the pending veto hand) that only the acting seat may see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    Nomination,
    AwaitingVotes {
        nominee: usize,
    },
    PresidentLegislative {
        drawn: [Policy; 3],
    },
    ChancellorLegislative {
        hand: [Policy; 2],
        veto_available: bool,
    },
    AwaitingVetoConsent {
        hand: [Policy; 2],
    },
    PresidentialPower {
        power: Power,
    },
    GameOver,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    config: GameConfig,
    rng: Rng,
    aux: Rng,
    players: Vec<Player>,
    deck: Deck,
    board: Board,
    election_tracker: u8,

    president: usize,
    chancellor: Option<usize>,
    last_president: Option<usize>,
    last_chancellor: Option<usize>,

    phase: Phase,

    // Special-election bookkeeping.
    pending_special: Option<usize>,
    special_return: Option<usize>,
    current_is_special: bool,

    // Investigate Loyalty.
    investigated: Vec<usize>,
    investigation_results: BTreeMap<usize, Vec<(usize, Party)>>,

    winner: Option<(Faction, WinReason)>,
    log: GameLog,
}

impl GameState {
    // -----------------------------------------------------------------------
    // Setup
    // -----------------------------------------------------------------------

    /// Create and deal a fresh 7-player game. Roles are shuffled from the seed;
    /// private role/teammate knowledge is emitted into the log immediately.
    pub fn new(config: GameConfig) -> Self {
        let mut rng = Rng::new(config.seed);
        let aux = rng.fork(0xF0);

        // Deal roles.
        let mut roles = SEVEN_PLAYER_ROLES;
        rng.shuffle(&mut roles);
        let players: Vec<Player> = roles
            .iter()
            .enumerate()
            .map(|(seat, &role)| Player {
                seat,
                role,
                alive: true,
            })
            .collect();

        let deck = Deck::new(&mut rng);
        let first_president = config
            .first_president
            .unwrap_or_else(|| rng.below(NUM_PLAYERS));

        let mut state = Self {
            config,
            rng,
            aux,
            players,
            deck,
            board: Board::default(),
            election_tracker: 0,
            president: first_president,
            chancellor: None,
            last_president: None,
            last_chancellor: None,
            phase: Phase::Nomination,
            pending_special: None,
            special_return: None,
            current_is_special: false,
            investigated: Vec::new(),
            investigation_results: BTreeMap::new(),
            winner: None,
            log: GameLog::default(),
        };

        state.emit_setup();
        state
    }

    fn emit_setup(&mut self) {
        self.log.public(Event::GameStarted { seats: NUM_PLAYERS });

        // Role knowledge (private) + Fascist team reveal to the two regular
        // Fascists only. Hitler stays blind (7-player rule).
        let fascist_seats: Vec<usize> = self
            .players
            .iter()
            .filter(|p| p.role == Role::Fascist)
            .map(|p| p.seat)
            .collect();
        let hitler_seat = self
            .players
            .iter()
            .find(|p| p.role == Role::Hitler)
            .map(|p| p.seat)
            .expect("a 7-player game has exactly one Hitler");

        for i in 0..self.players.len() {
            let role = self.players[i].role;
            self.log.private(i, Event::RoleAssigned { role });
            if role == Role::Fascist {
                self.log.private(
                    i,
                    Event::FascistTeamRevealed {
                        fascists: fascist_seats.clone(),
                        hitler: hitler_seat,
                    },
                );
            }
        }

        self.log.public(Event::PresidencyBegan {
            president: self.president,
            special_election: false,
        });
    }

    // -----------------------------------------------------------------------
    // Read accessors
    // -----------------------------------------------------------------------

    pub fn config(&self) -> &GameConfig {
        &self.config
    }
    pub fn board(&self) -> &Board {
        &self.board
    }
    pub fn election_tracker(&self) -> u8 {
        self.election_tracker
    }
    pub fn president(&self) -> usize {
        self.president
    }
    pub fn chancellor(&self) -> Option<usize> {
        self.chancellor
    }
    pub fn players(&self) -> &[Player] {
        &self.players
    }
    pub fn log(&self) -> &GameLog {
        &self.log
    }
    pub fn is_over(&self) -> bool {
        self.winner.is_some()
    }
    pub fn winner(&self) -> Option<Faction> {
        self.winner.map(|(f, _)| f)
    }
    pub fn win_reason(&self) -> Option<WinReason> {
        self.winner.map(|(_, r)| r)
    }
    pub fn role_of(&self, seat: usize) -> Role {
        self.players[seat].role
    }
    pub fn is_alive(&self, seat: usize) -> bool {
        self.players[seat].alive
    }

    pub fn living_seats(&self) -> Vec<usize> {
        self.players
            .iter()
            .filter(|p| p.alive)
            .map(|p| p.seat)
            .collect()
    }

    pub fn living_count(&self) -> usize {
        self.players.iter().filter(|p| p.alive).count()
    }

    // -----------------------------------------------------------------------
    // Rotation helpers
    // -----------------------------------------------------------------------

    fn next_living_after(&self, seat: usize) -> usize {
        for step in 1..=NUM_PLAYERS {
            let s = (seat + step) % NUM_PLAYERS;
            if self.players[s].alive {
                return s;
            }
        }
        seat // unreachable while ≥1 player alive
    }

    fn first_living_at_or_after(&self, seat: usize) -> usize {
        for step in 0..NUM_PLAYERS {
            let s = (seat + step) % NUM_PLAYERS;
            if self.players[s].alive {
                return s;
            }
        }
        seat
    }

    /// Chancellor candidates: alive, not the President, not term-limited. The
    /// term-limit set relaxes to only the last Chancellor once ≤5 players live.
    fn eligible_chancellors(&self) -> Vec<usize> {
        let relaxed = self.living_count() <= 5;
        self.players
            .iter()
            .filter(|p| p.alive && p.seat != self.president)
            .filter(|p| {
                if Some(p.seat) == self.last_chancellor {
                    return false;
                }
                if !relaxed && Some(p.seat) == self.last_president {
                    return false;
                }
                true
            })
            .map(|p| p.seat)
            .collect()
    }

    fn term_limited_now(&self) -> Vec<usize> {
        let mut v = Vec::new();
        if let Some(c) = self.last_chancellor {
            if self.players[c].alive {
                v.push(c);
            }
        }
        if self.living_count() > 5 {
            if let Some(p) = self.last_president {
                if self.players[p].alive {
                    v.push(p);
                }
            }
        }
        v
    }

    // -----------------------------------------------------------------------
    // Decision surface
    // -----------------------------------------------------------------------

    /// The single pending decision, or `None` if the game is over.
    pub fn current_decision(&self) -> Option<Decision> {
        let kind = match &self.phase {
            Phase::Nomination => DecisionKind::Nominate {
                president: self.president,
                eligible: self.eligible_chancellors(),
            },
            Phase::AwaitingVotes { nominee } => DecisionKind::Vote {
                president: self.president,
                nominee: *nominee,
                voters: self.living_seats(),
            },
            Phase::PresidentLegislative { drawn } => DecisionKind::PresidentDiscard {
                president: self.president,
                drawn: *drawn,
            },
            Phase::ChancellorLegislative {
                hand,
                veto_available,
            } => DecisionKind::ChancellorEnact {
                chancellor: self.chancellor.expect("chancellor set in legislative"),
                hand: *hand,
                veto_available: *veto_available,
            },
            Phase::AwaitingVetoConsent { .. } => DecisionKind::VetoConsent {
                president: self.president,
            },
            Phase::PresidentialPower { power } => {
                let eligible = self.power_targets(*power);
                match power {
                    Power::InvestigateLoyalty => DecisionKind::Investigate {
                        president: self.president,
                        eligible,
                    },
                    Power::SpecialElection => DecisionKind::SpecialElection {
                        president: self.president,
                        eligible,
                    },
                    Power::Execution => DecisionKind::Execution {
                        president: self.president,
                        eligible,
                    },
                }
            }
            Phase::GameOver => return None,
        };
        Some(Decision { kind })
    }

    fn power_targets(&self, power: Power) -> Vec<usize> {
        self.players
            .iter()
            .filter(|p| p.alive && p.seat != self.president)
            .filter(|p| match power {
                Power::InvestigateLoyalty => !self.investigated.contains(&p.seat),
                _ => true,
            })
            .map(|p| p.seat)
            .collect()
    }

    /// Convenience: the forced legal default for the current decision (FR-4).
    pub fn forced_default(&mut self) -> Option<Action> {
        let d = self.current_decision()?;
        Some(d.forced_default(&mut self.aux))
    }

    // -----------------------------------------------------------------------
    // Observations (US-003)
    // -----------------------------------------------------------------------

    /// Build the role-conditioned observation for a seat. Only that seat's own
    /// private log entries and entitled knowledge are ever included.
    pub fn observation(&self, seat: usize) -> Observation {
        let your_role = self.players[seat].role;

        // Regular Fascists (and only they) know the team.
        let known_team = if your_role == Role::Fascist {
            let fascists: Vec<usize> = self
                .players
                .iter()
                .filter(|p| p.role == Role::Fascist)
                .map(|p| p.seat)
                .collect();
            let hitler = self
                .players
                .iter()
                .find(|p| p.role == Role::Hitler)
                .map(|p| p.seat)
                .expect("one Hitler");
            Some(KnownTeam { fascists, hitler })
        } else {
            None
        };

        let players = self
            .players
            .iter()
            .map(|p| PlayerView {
                seat: p.seat,
                alive: p.alive,
            })
            .collect();

        let your_investigations = self
            .investigation_results
            .get(&seat)
            .cloned()
            .unwrap_or_default();

        // Only surface the pending decision to the seat that must act (every
        // living seat for a simultaneous vote).
        let pending = self.current_decision().filter(|d| match d.actor() {
            Some(a) => a == seat,
            None => self.players[seat].alive, // Vote: all living seats
        });

        Observation {
            you: seat,
            your_role,
            known_team,
            players,
            board: self.board.clone(),
            election_tracker: self.election_tracker,
            president: (!self.is_over()).then_some(self.president),
            term_limited: self.term_limited_now(),
            veto_unlocked: self.board.veto_unlocked(),
            your_investigations,
            history: self.log.visible_to(seat),
            pending,
        }
    }
}

// ===========================================================================
// Test-support seams (black-box: crate-visible constructors used by the rules
// suite to control roles, deck order, and board position deterministically).
// ===========================================================================

#[cfg(test)]
impl GameState {
    pub(crate) fn test_game(
        roles: [Role; NUM_PLAYERS],
        draw: Vec<Policy>,
        first_president: usize,
    ) -> Self {
        let config = GameConfig::new(0).with_first_president(first_president);
        let rng = Rng::new(config.seed);
        let aux = rng.fork(0xF0);
        let players = roles
            .iter()
            .enumerate()
            .map(|(seat, &role)| Player {
                seat,
                role,
                alive: true,
            })
            .collect();
        let mut state = Self {
            config,
            rng,
            aux,
            players,
            deck: Deck::from_draw(draw),
            board: Board::default(),
            election_tracker: 0,
            president: first_president,
            chancellor: None,
            last_president: None,
            last_chancellor: None,
            phase: Phase::Nomination,
            pending_special: None,
            special_return: None,
            current_is_special: false,
            investigated: Vec::new(),
            investigation_results: BTreeMap::new(),
            winner: None,
            log: GameLog::default(),
        };
        state.emit_setup();
        state
    }

    /// Jump the enacted-policy counters (to reach a threshold quickly).
    pub(crate) fn test_set_board(&mut self, liberal: u8, fascist: u8) {
        self.board.liberal = liberal;
        self.board.fascist = fascist;
    }

    /// Tiles still in draw+discard piles (not yet enacted). For the tile
    /// conservation invariant.
    pub(crate) fn deck_in_circulation(&self) -> usize {
        self.deck.tiles_in_circulation()
    }
}
