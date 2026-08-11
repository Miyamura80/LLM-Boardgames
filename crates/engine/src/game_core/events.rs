//! The transcript spine: an append-only event log with per-event visibility.
//!
//! Every observable fact in a game — public or private — is an [`EventRecord`].
//! Per-seat observations are produced by filtering this log by visibility, so
//! information asymmetry is enforced structurally rather than by prompt
//! etiquette.

use super::state::Seat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Who may see an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Visibility {
    Public,
    /// Visible only to one seat (e.g. a stolen card's identity).
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

/// One transcript entry wrapping a game-specific event payload.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventRecord<E> {
    /// Position in the log (stable across replays).
    pub idx: u32,
    /// Game round the event belongs to (0 = setup).
    pub round: u32,
    pub visibility: Visibility,
    pub event: E,
}
