//! The parse → legal-check → rethink loop: one decision through a bounded
//! retry budget, falling back to the engine's deterministic forced legal
//! default. Identical for every game; per-game loops layer their own metric
//! capture on the returned [`DecisionOutcome`].

use super::agent::{AgentError, DecisionAgent};
use super::state::{DecisionOps, EngineState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Reliability counters, kept strictly separate from play-quality metrics.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
pub struct Reliability {
    pub malformed_outputs: u32,
    pub illegal_moves: u32,
    pub forced_defaults: u32,
    pub transport_failures: u32,
}

/// The model's private reasoning for one decision, captured verbatim from the
/// `thought_process` field of its reply. Observability only — never fed back
/// into any observation, so it cannot influence play.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ThoughtRecord {
    pub round: u32,
    /// Transcript position when the decision was made, for interleaving
    /// thoughts into a replay timeline.
    pub at_event: u32,
    /// Decision kind (`nominate`, `vote`, `turn`, …).
    pub decision: String,
    pub text: String,
}

/// How a decision was resolved. `forced` outcomes carry no thought and must be
/// excluded from play-quality metrics.
#[derive(Debug, Clone)]
pub struct DecisionOutcome<A> {
    pub action: A,
    pub forced: bool,
    pub thought: Option<String>,
}

/// Resolve one decision: each malformed parse, transport failure, or illegal
/// move consumes one attempt and feeds its reason back to the agent; on budget
/// exhaustion the forced legal default is applied and logged. The action has
/// already been applied to `state` when this returns.
pub async fn resolve_decision<S, A>(
    retry_budget: u32,
    state: &mut S,
    agent: &mut A,
    decision: &S::Decision,
    reliability: &mut Reliability,
) -> DecisionOutcome<S::Action>
where
    S: EngineState,
    A: DecisionAgent<S> + ?Sized,
{
    let seat = decision.seat();
    let mut feedback: Option<String> = None;

    for _ in 0..retry_budget {
        let obs = state.observe(seat);
        match agent.decide(&obs, decision, feedback.as_deref()).await {
            Err(AgentError::Malformed(m)) => {
                reliability.malformed_outputs += 1;
                feedback = Some(m);
            }
            Err(AgentError::Transport(e)) => {
                reliability.transport_failures += 1;
                tracing::warn!(seat, error = %e, "agent transport failure");
            }
            Ok(reply) => match state.apply(seat, reply.action.clone()) {
                Ok(()) => {
                    return DecisionOutcome {
                        action: reply.action,
                        forced: false,
                        thought: reply.thought,
                    }
                }
                Err(illegal) => {
                    reliability.illegal_moves += 1;
                    feedback = Some(illegal.0);
                }
            },
        }
    }

    // Budget exhausted: deterministic forced legal default, metric-exempt.
    let action = state.forced_default(decision);
    reliability.forced_defaults += 1;
    state.push_forced_default(seat, decision.kind());
    state
        .apply(seat, action.clone())
        .expect("forced default must be legal");
    DecisionOutcome {
        action,
        forced: true,
        thought: None,
    }
}
