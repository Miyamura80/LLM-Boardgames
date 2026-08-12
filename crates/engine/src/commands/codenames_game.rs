//! Codenames single-game commands: play one ad-hoc 4-seat game with any mix of
//! LLM seats and bots, and replay a stored one out of the `codenames_*` tables.

use crate::codenames::runner::{
    rules_from_config, run_game, wordlist_from_config, AgentFactory, AgentSpec, CodenamesAgentKind,
    GameConfig, GameRecord, SeatRecord,
};
use crate::codenames::types::{EndReason, Team, SEAT_COUNT};
use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parse a seat spec: `bot:<kind>` or a `provider/model` string.
pub(crate) fn parse_model_spec(s: &str) -> Result<AgentSpec, CommandError> {
    // A blank model would build an `llm` seat that no provider can route,
    // whose every call fails and whose every decision becomes a forced
    // default — a "successful" game that measured nothing.
    if s.trim().is_empty() {
        return Err(CommandError::InvalidInput(
            "a codenames seat spec must be a non-empty 'provider/model' string or 'bot:<kind>'"
                .into(),
        ));
    }
    if let Some(kind) = s.strip_prefix("bot:") {
        let kind = CodenamesAgentKind::parse(kind).map_err(CommandError::InvalidInput)?;
        if kind == CodenamesAgentKind::Llm {
            return Err(CommandError::InvalidInput(
                "bot:llm is not a bot; pass the model string directly".into(),
            ));
        }
        Ok(AgentSpec::bot(kind))
    } else {
        Ok(AgentSpec::llm(s))
    }
}

// ---------------------------------------------------------------------------
// codenames_play_game
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CodenamesPlayGame;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodenamesPlayGameInput {
    /// Seat specs in seat order — 0 = team A spymaster, 1 = team A operative,
    /// 2 = team B spymaster, 3 = team B operative. Each is `provider/model` or
    /// `bot:codenames-random`. Fewer than 4 entries cycle around the table.
    /// Default: all `bot:codenames-random`.
    pub models: Option<Vec<String>>,
    /// A named `codenames.model_sets` entry from config, used when `models` is
    /// absent.
    pub set: Option<String>,
    pub seed: Option<u64>,
    /// Return the full transcript record (large); off by default.
    #[serde(default)]
    pub include_record: bool,
}

/// One seat's headline result, so a caller sees who played what without asking
/// for the whole record.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SeatSummary {
    pub seat: u8,
    pub team: Team,
    pub role: String,
    pub model_id: String,
    pub agent_kind: String,
    pub scaffold_version: String,
    pub won: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodenamesPlayGameOutput {
    pub game_id: String,
    pub seed: u64,
    pub rules_version: String,
    pub wordlist_hash: String,
    pub winner: Team,
    pub end_reason: EndReason,
    /// Team turns played (one clue plus its guesses each).
    pub turns: u32,
    pub clues_given: usize,
    pub cards_revealed: usize,
    pub seats: Vec<SeatSummary>,
    pub duration_ms: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub forced_defaults: u32,
    pub malformed_outputs: u32,
    pub illegal_moves: u32,
    pub events: usize,
    pub record: Option<GameRecord>,
}

#[async_trait]
impl Command for CodenamesPlayGame {
    type Input = CodenamesPlayGameInput;
    type Output = CodenamesPlayGameOutput;

    fn name(&self) -> &'static str {
        "codenames_play_game"
    }
    fn description(&self) -> &'static str {
        "Play one ad-hoc 4-seat Codenames game with any mix of LLM seats and bots; returns the outcome, reliability counters, and optionally the full transcript."
    }

    async fn run(
        &self,
        input: CodenamesPlayGameInput,
        cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        let models = match (&input.models, &input.set) {
            (Some(m), _) => m.clone(),
            (None, Some(set)) => cfg.codenames.model_sets.get(set).cloned().ok_or_else(|| {
                CommandError::InvalidInput(format!(
                    "unknown codenames model set '{set}' (known: {:?})",
                    cfg.codenames.model_sets.keys().collect::<Vec<_>>()
                ))
            })?,
            (None, None) => vec!["bot:codenames-random".into()],
        };
        if models.is_empty() {
            return Err(CommandError::InvalidInput(
                "models must not be empty".into(),
            ));
        }
        if models.len() > SEAT_COUNT as usize {
            return Err(CommandError::InvalidInput(format!(
                "codenames seats {SEAT_COUNT} models; got {}",
                models.len()
            )));
        }
        let seed = input.seed.unwrap_or(42);
        let factory = AgentFactory::from_app_config(cfg).map_err(CommandError::InvalidInput)?;

        let mut agents = Vec::new();
        let mut anchors = Vec::new();
        for seat in 0..SEAT_COUNT as usize {
            let spec = parse_model_spec(&models[seat % models.len()])?;
            agents.push(
                factory
                    .build(&spec, seed ^ (seat as u64) << 8)
                    .map_err(CommandError::InvalidInput)?,
            );
            anchors.push(false);
        }

        let game_cfg = GameConfig {
            game_id: format!("adhoc-{}", cx.request_id),
            seed,
            retry_budget: cfg.codenames.retry_budget,
            schedule_label: "adhoc".into(),
            rules: rules_from_config(&cfg.codenames).map_err(CommandError::InvalidInput)?,
            wordlist: wordlist_from_config(&cfg.codenames).map_err(CommandError::InvalidInput)?,
        };
        let record = run_game(&game_cfg, &mut agents, &anchors).await;

        let sum = |f: fn(&SeatRecord) -> u32| record.seats.iter().map(f).sum::<u32>();
        let clues_given = record
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.event,
                    crate::codenames::events::CodenamesEvent::ClueGiven { .. }
                )
            })
            .count();
        let cards_revealed = record
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.event,
                    crate::codenames::events::CodenamesEvent::GuessRevealed { .. }
                )
            })
            .count();

        Ok(CodenamesPlayGameOutput {
            game_id: record.game_id.clone(),
            seed,
            rules_version: record.rules_version.clone(),
            wordlist_hash: record.wordlist_hash.clone(),
            winner: record.winner,
            end_reason: record.end_reason,
            turns: record.turns,
            clues_given,
            cards_revealed,
            seats: record
                .seats
                .iter()
                .map(|s| SeatSummary {
                    seat: s.seat,
                    team: s.team,
                    role: s.role.as_str().to_string(),
                    model_id: s.model_id.clone(),
                    agent_kind: s.agent_kind.clone(),
                    scaffold_version: s.scaffold_version.clone(),
                    won: s.won,
                })
                .collect(),
            duration_ms: record.duration_ms,
            prompt_tokens: record.seats.iter().map(|s| s.usage.prompt_tokens).sum(),
            completion_tokens: record.seats.iter().map(|s| s.usage.completion_tokens).sum(),
            forced_defaults: sum(|s| s.reliability.forced_defaults),
            malformed_outputs: sum(|s| s.reliability.malformed_outputs),
            illegal_moves: sum(|s| s.reliability.illegal_moves),
            events: record.events.len(),
            record: input.include_record.then_some(record),
        })
    }
}

register_command!(CodenamesPlayGame);

// ---------------------------------------------------------------------------
// codenames_game_replay
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CodenamesGameReplay;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodenamesGameReplayInput {
    pub game_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodenamesGameReplayOutput {
    pub record: GameRecord,
    /// Rendered transcript, one line per event. Every Codenames event is
    /// public, so this is the omniscient *and* the seat view — except for the
    /// key card, which is state and never appears here.
    pub rendered: Vec<String>,
}

#[async_trait]
impl Command for CodenamesGameReplay {
    type Input = CodenamesGameReplayInput;
    type Output = CodenamesGameReplayOutput;

    fn name(&self) -> &'static str {
        "codenames_game_replay"
    }
    fn description(&self) -> &'static str {
        "Fetch a stored Codenames game's full replayable record with rendered transcript lines."
    }

    async fn run(
        &self,
        input: CodenamesGameReplayInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let store = super::codenames_match::open_codenames_store().await?;
        let record = store
            .get_game(&input.game_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?
            .ok_or_else(|| {
                CommandError::InvalidInput(format!("no stored game '{}'", input.game_id))
            })?;
        let rendered = record.events.iter().map(|r| r.event.render()).collect();
        Ok(CodenamesGameReplayOutput { record, rendered })
    }
}

register_command!(CodenamesGameReplay);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seat_specs_split_bots_from_models() {
        let bot = parse_model_spec("bot:codenames-random").expect("known bot");
        assert_eq!(bot.kind, CodenamesAgentKind::Random);
        assert_eq!(bot.model_id(), "bot:codenames-random");

        let llm = parse_model_spec("gemini/gemini-3-flash-preview").expect("model string");
        assert_eq!(llm.kind, CodenamesAgentKind::Llm);
        assert_eq!(llm.model.as_deref(), Some("gemini/gemini-3-flash-preview"));

        assert!(matches!(
            parse_model_spec("bot:llm"),
            Err(CommandError::InvalidInput(_))
        ));
        assert!(matches!(
            parse_model_spec("bot:random-legal"),
            Err(CommandError::InvalidInput(_))
        ));
    }

    /// A blank seat spec is invalid input, not a seat that quietly forfeits
    /// every decision to the forced default.
    #[test]
    fn blank_model_names_are_rejected() {
        for blank in ["", " ", "\t", "\n  "] {
            let err = match parse_model_spec(blank) {
                Err(CommandError::InvalidInput(e)) => e,
                other => panic!("{blank:?} should be invalid input, got {other:?}"),
            };
            assert!(err.contains("non-empty"), "{err}");
        }
    }
}
