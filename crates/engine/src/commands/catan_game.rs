//! Catan single-game commands: play one ad-hoc 4-player game with any mix of
//! LLM seats and bots.

use crate::catan::runner::{run_game, AgentFactory, AgentSpec, CatanAgentKind, GameConfig};
use crate::catan::runner::{GameRecord, SeatRecord};
use crate::catan::state::TurnCaps;
use crate::catan::types::PLAYER_COUNT;
use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parse a seat spec: `bot:<kind>` or a `provider/model` string.
pub(crate) fn parse_model_spec(s: &str) -> Result<AgentSpec, CommandError> {
    if let Some(kind) = s.strip_prefix("bot:") {
        let kind = CatanAgentKind::parse(kind).map_err(CommandError::InvalidInput)?;
        if kind == CatanAgentKind::Llm {
            return Err(CommandError::InvalidInput(
                "bot:llm is not a bot; pass the model string directly".into(),
            ));
        }
        Ok(AgentSpec::bot(kind))
    } else {
        Ok(AgentSpec::llm(s))
    }
}

pub(crate) fn turn_caps_from_config(cfg: &app_config::CatanConfig) -> TurnCaps {
    TurnCaps {
        actions: cfg.action_cap,
        trade_windows: cfg.trade_windows_cap,
        says: cfg.say_cap,
        max_turns: cfg.max_turns,
    }
}

// ---------------------------------------------------------------------------
// catan_play_game
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CatanPlayGame;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CatanPlayGameInput {
    /// Seat models: `provider/model` or `bot:random-legal` / `bot:greedy`.
    /// Fewer than 4 entries cycle around the table. Default: all `bot:greedy`.
    pub models: Option<Vec<String>>,
    pub seed: Option<u64>,
    /// Return the full transcript record (large); off by default.
    #[serde(default)]
    pub include_record: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CatanPlayGameOutput {
    pub game_id: String,
    pub seed: u64,
    pub winner: u8,
    /// Final total VP per seat (hidden VP included).
    pub final_vps: Vec<u8>,
    /// Competition placement per seat (1 = winner; ties share rank).
    pub placements: Vec<u8>,
    pub turns: u32,
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
impl Command for CatanPlayGame {
    type Input = CatanPlayGameInput;
    type Output = CatanPlayGameOutput;

    fn name(&self) -> &'static str {
        "catan_play_game"
    }
    fn description(&self) -> &'static str {
        "Play one ad-hoc 4-player Catan game with any mix of LLM seats and bots; returns placements, reliability counters, and optionally the full transcript."
    }

    async fn run(
        &self,
        input: CatanPlayGameInput,
        cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        let models = input.models.unwrap_or_else(|| vec!["bot:greedy".into()]);
        if models.is_empty() {
            return Err(CommandError::InvalidInput(
                "models must not be empty".into(),
            ));
        }
        if models.len() > PLAYER_COUNT as usize {
            return Err(CommandError::InvalidInput(format!(
                "at most {PLAYER_COUNT} models fit a table; got {}",
                models.len()
            )));
        }
        let seed = input.seed.unwrap_or(42);
        let factory = AgentFactory::from_app_config(cfg);

        let mut agents = Vec::new();
        let mut anchors = Vec::new();
        for seat in 0..PLAYER_COUNT as usize {
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
            retry_budget: cfg.catan.retry_budget,
            schedule_label: "adhoc".into(),
            caps: turn_caps_from_config(&cfg.catan),
        };
        let record = run_game(&game_cfg, &mut agents, &anchors).await;

        let sum = |f: fn(&SeatRecord) -> u32| record.seats.iter().map(f).sum::<u32>();
        Ok(CatanPlayGameOutput {
            game_id: record.game_id.clone(),
            seed,
            winner: record.winner,
            final_vps: record.final_vps.clone(),
            placements: record.seats.iter().map(|s| s.placement).collect(),
            turns: record.turns,
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

register_command!(CatanPlayGame);
