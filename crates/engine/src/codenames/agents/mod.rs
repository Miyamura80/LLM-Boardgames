//! The Codenames seat-agent contract: anything that can occupy one of the four
//! seats — scripted bots now, LLM players later — implements [`SeatAgent`].
//!
//! Agents never see [`GameState`]; they receive only their role-conditioned
//! [`Observation`], so an operative structurally cannot read the key card.
//! There is no discussion surface (PRD-codenames-evals §5 rules out operative
//! table talk), so `decide` is the whole interface — the Catan shape, not the
//! Secret Hitler one.

mod random_bot;

pub use random_bot::RandomLegalBot;

use super::actions::{Action, DecisionPoint};
use super::observation::Observation;
use super::state::GameState;
use crate::llm::TokenUsage;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::game_core::AgentError;

/// A decision reply: the action plus the model's stated reasoning.
pub type AgentReply = crate::game_core::Reply<Action>;

/// One seat's player. Agents never see [`GameState`]; they receive only their
/// visibility-filtered [`Observation`].
#[async_trait]
pub trait SeatAgent: Send {
    /// Agent family, e.g. `bot:codenames-random` or `llm`.
    fn kind(&self) -> &'static str;
    /// Rated model identity, e.g. `gemini/gemini-3-flash-preview` for LLM
    /// seats or `bot:codenames-random` for scripted seats.
    fn model_id(&self) -> String;
    /// Version hash of prompt templates + persona + temperature ("scaffold").
    fn scaffold_version(&self) -> String;
    /// Decide at a decision point. `feedback` carries the parse/illegal-move
    /// reason from the previous failed attempt (the rethink loop).
    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError>;
    /// Cumulative token usage (zero for bots).
    fn usage(&self) -> TokenUsage {
        TokenUsage::default()
    }
}

/// Lets the shared `game_core` rethink loop drive any boxed Codenames seat
/// agent.
#[async_trait]
impl crate::game_core::DecisionAgent<GameState> for Box<dyn SeatAgent> {
    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        SeatAgent::decide(&mut **self, obs, decision, feedback).await
    }
}

/// The closed set of Codenames seat-agent kinds, validated at the game
/// boundary (PRD-codenames-evals US-CN06; Catan PRD decision #10 — bot
/// taxonomies never accrete in `crates/config`).
///
/// The kind strings are game-qualified for the bots (`codenames-random`,
/// `codenames-embedding`) because a shared config file names seats across all
/// three games; `llm` is the one cross-game kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CodenamesAgentKind {
    /// [`RandomLegalBot`] — the zero-token legality floor.
    #[serde(rename = "codenames-random")]
    Random,
    /// The embedding-greedy anchor (US-CN06; not yet implemented).
    #[serde(rename = "codenames-embedding")]
    Embedding,
    /// A live model seat.
    #[serde(rename = "llm")]
    Llm,
}

impl CodenamesAgentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodenamesAgentKind::Random => "codenames-random",
            CodenamesAgentKind::Embedding => "codenames-embedding",
            CodenamesAgentKind::Llm => "llm",
        }
    }

    /// Parse a config-provided kind string, failing before tokens are spent.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "codenames-random" => Ok(Self::Random),
            "codenames-embedding" => Ok(Self::Embedding),
            "llm" => Ok(Self::Llm),
            other => Err(format!(
                "unknown codenames agent kind '{other}' (expected codenames-random | \
                 codenames-embedding | llm)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boundary strings are load-bearing: config files and the serde
    /// representation must agree with `as_str`/`parse` in both directions.
    #[test]
    fn agent_kinds_round_trip_through_their_boundary_strings() {
        for kind in [
            CodenamesAgentKind::Random,
            CodenamesAgentKind::Embedding,
            CodenamesAgentKind::Llm,
        ] {
            let s = kind.as_str();
            assert_eq!(CodenamesAgentKind::parse(s), Ok(kind));
            assert_eq!(
                serde_json::to_string(&kind).expect("kind serializes"),
                format!("\"{s}\"")
            );
        }
        let err = CodenamesAgentKind::parse("random-legal").expect_err("catan's name");
        assert!(err.contains("unknown codenames agent kind"), "{err}");
        assert!(err.contains("codenames-random"), "{err}");
    }
}
