//! The transcript spine: an append-only event log with per-event visibility.
//!
//! Every observable fact in a game — public or private — is an [`EventRecord`].
//! Role-conditioned observations are produced by filtering this log by
//! visibility, so information asymmetry is enforced structurally rather than by
//! prompt etiquette.

use super::types::{Party, Power, Role, Seat, WinCondition};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Who may see an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Visibility {
    Public,
    /// Visible only to one seat (e.g. a President's investigation result).
    Private(Seat),
}

impl Visibility {
    pub fn visible_to(&self, seat: Seat) -> bool {
        match self {
            Visibility::Public => true,
            Visibility::Private(s) => *s == seat,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventRecord {
    /// Position in the log (stable across replays).
    pub idx: u32,
    /// Government round the event belongs to (0 = setup).
    pub round: u32,
    pub visibility: Visibility,
    pub event: GameEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum GameEvent {
    GameStarted {
        players: u8,
    },
    /// Private per-seat deal: the seat's own role plus everything the role is
    /// entitled to know (regular Fascists see each other and Hitler; Hitler and
    /// Liberals see nobody at 7 players).
    RolesDealt {
        seat: Seat,
        role: Role,
        known_teammates: Vec<(Seat, Role)>,
    },
    ChancellorNominated {
        president: Seat,
        nominee: Seat,
    },
    /// All simultaneous ballots, revealed together (seat-ordered pairs; a
    /// plain map would break this internally-tagged enum's JSON round trip
    /// for integer keys).
    VotesRevealed {
        nominee: Seat,
        votes: Vec<(Seat, bool)>,
        passed: bool,
    },
    ElectionTrackerAdvanced {
        value: u8,
    },
    /// Election tracker reached 3: top tile enacted, no power granted,
    /// term-limit memory cleared.
    TopDeckEnacted {
        policy: Party,
    },
    GovernmentFormed {
        president: Seat,
        chancellor: Seat,
    },
    /// Private: the 3 tiles the President drew.
    PresidentDrew {
        tiles: Vec<Party>,
    },
    /// Private: which tile the President discarded.
    PresidentDiscarded {
        policy: Party,
    },
    /// Private: the 2 tiles passed to the Chancellor.
    ChancellorReceived {
        tiles: Vec<Party>,
    },
    PolicyEnacted {
        policy: Party,
    },
    DeckReshuffled {
        draw_count: u8,
    },
    PowerGranted {
        president: Seat,
        power: Power,
    },
    /// Public fact that an investigation happened (target is public knowledge).
    Investigated {
        president: Seat,
        target: Seat,
    },
    /// Private to the investigating President: the target's party membership
    /// (Hitler's card reads Fascist). Never the secret role.
    InvestigationResult {
        target: Seat,
        party: Party,
    },
    SpecialElectionCalled {
        president: Seat,
        target: Seat,
    },
    /// Dead players' roles stay hidden unless the game ends (Hitler).
    Executed {
        president: Seat,
        target: Seat,
        was_hitler: bool,
    },
    VetoProposed {
        chancellor: Seat,
    },
    VetoDecided {
        president: Seat,
        approved: bool,
    },
    /// One discussion utterance (or explicit pass) in a simultaneous-reveal
    /// round. Appended in canonical seat order after the whole round resolves.
    Utterance {
        seat: Seat,
        discussion_round: u8,
        text: String,
        pass: bool,
    },
    /// A decision that fell through to the engine's forced legal default after
    /// the retry budget was exhausted. Public in the transcript for
    /// auditability; excluded from play-quality metrics.
    ForcedDefault {
        seat: Seat,
        decision: String,
    },
    GameEnded {
        winner: Party,
        condition: WinCondition,
        /// Full role reveal, by seat.
        roles: Vec<Role>,
    },
}

impl GameEvent {
    /// Human-readable line for prompts and replays, from `seat`'s perspective.
    pub fn render(&self) -> String {
        match self {
            GameEvent::GameStarted { players } => {
                format!("Game started with {players} players.")
            }
            GameEvent::RolesDealt {
                role,
                known_teammates,
                ..
            } => {
                let mut s = format!("You were dealt the role: {}.", role.as_str());
                if known_teammates.is_empty() {
                    s.push_str(" You know no other player's role.");
                } else {
                    for (seat, r) in known_teammates {
                        s.push_str(&format!(" Player {} is {}.", seat, r.as_str()));
                    }
                }
                s
            }
            GameEvent::ChancellorNominated { president, nominee } => {
                format!("President Player {president} nominated Player {nominee} as Chancellor.")
            }
            GameEvent::VotesRevealed {
                nominee,
                votes,
                passed,
            } => {
                let list = votes
                    .iter()
                    .map(|(s, v)| format!("P{}={}", s, if *v { "Ja" } else { "Nein" }))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "Vote on Chancellor Player {nominee}: {list} -> {}.",
                    if *passed { "PASSED" } else { "FAILED" }
                )
            }
            GameEvent::ElectionTrackerAdvanced { value } => {
                format!("Election tracker advanced to {value}/3.")
            }
            GameEvent::TopDeckEnacted { policy } => format!(
                "Election tracker hit 3: top policy auto-enacted ({}). No power granted; term limits reset.",
                policy.as_str()
            ),
            GameEvent::GovernmentFormed {
                president,
                chancellor,
            } => format!("Government formed: President Player {president}, Chancellor Player {chancellor}."),
            GameEvent::PresidentDrew { tiles } => format!(
                "You drew 3 policy tiles: {}.",
                tiles.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(", ")
            ),
            GameEvent::PresidentDiscarded { policy } => {
                format!("You discarded a {} policy.", policy.as_str())
            }
            GameEvent::ChancellorReceived { tiles } => format!(
                "You received 2 policy tiles: {}.",
                tiles.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(", ")
            ),
            GameEvent::PolicyEnacted { policy } => {
                format!("A {} policy was enacted.", policy.as_str())
            }
            GameEvent::DeckReshuffled { draw_count } => {
                format!("The discard pile was shuffled back into the deck ({draw_count} tiles).")
            }
            GameEvent::PowerGranted { president, power } => format!(
                "President Player {president} must use the power: {power:?}."
            ),
            GameEvent::Investigated { president, target } => format!(
                "President Player {president} investigated Player {target}'s party membership."
            ),
            GameEvent::InvestigationResult { target, party } => format!(
                "Private: Player {target}'s party membership card is {}.",
                party.as_str()
            ),
            GameEvent::SpecialElectionCalled { president, target } => format!(
                "President Player {president} called a Special Election: Player {target} is the next Presidential candidate."
            ),
            GameEvent::Executed {
                president,
                target,
                was_hitler,
            } => {
                let mut s =
                    format!("President Player {president} executed Player {target}.");
                if *was_hitler {
                    s.push_str(" They were Hitler!");
                } else {
                    s.push_str(" They were not Hitler; their role stays hidden.");
                }
                s
            }
            GameEvent::VetoProposed { chancellor } => {
                format!("Chancellor Player {chancellor} proposed to veto the agenda.")
            }
            GameEvent::VetoDecided {
                president,
                approved,
            } => format!(
                "President Player {president} {} the veto.",
                if *approved { "approved" } else { "declined" }
            ),
            GameEvent::Utterance {
                seat,
                text,
                pass,
                ..
            } => {
                if *pass {
                    format!("Player {seat}: (passes)")
                } else {
                    format!("Player {seat}: {text}")
                }
            }
            GameEvent::ForcedDefault { seat, decision } => {
                format!("Player {seat} failed to act; the engine applied the default {decision}.")
            }
            GameEvent::GameEnded {
                winner, condition, ..
            } => format!(
                "Game over: {} team wins ({condition:?}).",
                winner.as_str()
            ),
        }
    }
}
