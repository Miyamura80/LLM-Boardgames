//! The state contract a rules engine implements to be driven by the generic
//! decision loop. Agents never see the state itself — only the per-seat
//! observation it produces.

/// A seat index. Every game in the harness has a fixed, small player count.
pub type Seat = u8;

/// A structurally valid but rule-breaking move. The message is fed back to the
/// agent verbatim on the rethink re-prompt.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
#[error("illegal move: {0}")]
pub struct IllegalMove(pub String);

/// What the generic loop needs to know about a pending decision.
pub trait DecisionOps {
    /// The seat that must act.
    fn seat(&self) -> Seat;
    /// Short kebab name for logs and reliability counters.
    fn kind(&self) -> &'static str;
}

/// An engine-authoritative game state drivable by the generic rethink loop.
///
/// `apply` is the single mutation entry point and the sole legality authority;
/// `forced_default` must always return a legal action (its randomness draws
/// from the game RNG so seeded runs stay reproducible).
pub trait EngineState: Send {
    type Action: Clone + Send + Sync;
    type Observation: Send + Sync;
    type Decision: DecisionOps + Clone + Send + Sync;

    /// Every decision the engine is currently waiting on. Multiple entries
    /// exist only while simultaneous decisions are being collected.
    fn pending_decisions(&self) -> Vec<Self::Decision>;
    /// The seat's visibility-filtered view; never leaks hidden information.
    fn observe(&self, seat: Seat) -> Self::Observation;
    /// Validate and apply one action.
    fn apply(&mut self, seat: Seat, action: Self::Action) -> Result<(), IllegalMove>;
    /// The deterministic forced legal default for a decision (retry budget
    /// exhausted).
    fn forced_default(&mut self, decision: &Self::Decision) -> Self::Action;
    /// Record the public forced-default event in the transcript.
    fn push_forced_default(&mut self, seat: Seat, kind: &str);
    fn is_over(&self) -> bool;
}
