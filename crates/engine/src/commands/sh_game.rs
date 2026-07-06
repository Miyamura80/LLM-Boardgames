//! Single-game commands: play one ad-hoc game (smoke tests, demos) and fetch
//! a stored game's replayable record.

use crate::commands::sh_match::{open_store, parse_model_spec};
use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use crate::secret_hitler::metrics::{score_game, SeatMetrics};
use crate::secret_hitler::runner::record::GameRecord;
use crate::secret_hitler::runner::{run_game, AgentFactory, GameConfig};
use crate::secret_hitler::store::GameSummary;
use crate::secret_hitler::types::PLAYER_COUNT;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// sh_play_game
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ShPlayGame;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShPlayGameInput {
    /// Seat models: `provider/model` or `bot:<kind>` entries. Fewer than 7
    /// entries cycle around the table. Default: all `bot:heuristic`.
    pub models: Option<Vec<String>>,
    pub seed: Option<u64>,
    pub discussion_rounds: Option<u8>,
    pub belief_checkpoints: Option<bool>,
    /// Return the full transcript record (large); off by default.
    #[serde(default)]
    pub include_record: bool,
    /// Persist into this run id (requires Postgres) instead of only returning.
    pub store_run: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ShPlayGameOutput {
    pub game_id: String,
    pub seed: u64,
    pub winner: String,
    pub win_condition: String,
    pub rounds: u32,
    pub duration_ms: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub forced_defaults: u32,
    pub malformed_outputs: u32,
    pub illegal_moves: u32,
    pub events: usize,
    pub metrics: Vec<SeatMetrics>,
    pub record: Option<GameRecord>,
}

#[async_trait]
impl Command for ShPlayGame {
    type Input = ShPlayGameInput;
    type Output = ShPlayGameOutput;

    fn name(&self) -> &'static str {
        "sh_play_game"
    }
    fn description(&self) -> &'static str {
        "Play one ad-hoc 7-player Secret Hitler game with any mix of LLM seats and bots; returns the outcome, reliability counters, metrics, and optionally the full transcript."
    }

    async fn run(
        &self,
        input: ShPlayGameInput,
        cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        let models = input.models.unwrap_or_else(|| vec!["bot:heuristic".into()]);
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
            discussion_rounds: input
                .discussion_rounds
                .unwrap_or(cfg.secret_hitler.discussion_rounds),
            retry_budget: cfg.secret_hitler.retry_budget,
            belief_checkpoints: input.belief_checkpoints.unwrap_or(true),
            schedule_label: "adhoc".into(),
        };
        let record = run_game(&game_cfg, &mut agents, &anchors).await;
        let metrics = score_game(&record);

        if let Some(run_id) = &input.store_run {
            let store = open_store().await?;
            store
                .create_run(run_id, "adhoc", &serde_json::json!({"mode":"adhoc"}))
                .await
                .map_err(|e| CommandError::Other(e.to_string()))?;
            let inserted = store
                .insert_game(run_id, &record, &metrics)
                .await
                .map_err(|e| CommandError::Other(e.to_string()))?;
            debug_assert!(inserted, "ad-hoc game ids are request-unique");
        }

        let sum = |f: fn(&crate::secret_hitler::runner::record::SeatRecord) -> u32| {
            record.seats.iter().map(f).sum::<u32>()
        };
        Ok(ShPlayGameOutput {
            game_id: record.game_id.clone(),
            seed,
            winner: record.winner.as_str().to_string(),
            win_condition: format!("{:?}", record.win_condition),
            rounds: record.rounds,
            duration_ms: record.duration_ms,
            prompt_tokens: record.seats.iter().map(|s| s.usage.prompt_tokens).sum(),
            completion_tokens: record.seats.iter().map(|s| s.usage.completion_tokens).sum(),
            forced_defaults: sum(|s| s.reliability.forced_defaults),
            malformed_outputs: sum(|s| s.reliability.malformed_outputs),
            illegal_moves: sum(|s| s.reliability.illegal_moves),
            events: record.events.len(),
            metrics,
            record: input.include_record.then_some(record),
        })
    }
}

register_command!(ShPlayGame);

// ---------------------------------------------------------------------------
// sh_game_replay
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ShGameReplay;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShGameReplayInput {
    pub game_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ShGameReplayOutput {
    pub record: GameRecord,
    /// Canonical omniscient rendering of `record.events`, index-aligned —
    /// clients display these instead of re-rendering the event union.
    pub rendered: Vec<String>,
}

#[async_trait]
impl Command for ShGameReplay {
    type Input = ShGameReplayInput;
    type Output = ShGameReplayOutput;

    fn name(&self) -> &'static str {
        "sh_game_replay"
    }
    fn description(&self) -> &'static str {
        "Fetch a stored game's complete replayable record (transcript, seats, beliefs, metrics inputs)."
    }

    async fn run(
        &self,
        input: ShGameReplayInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let store = open_store().await?;
        let record = store
            .get_game(&input.game_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?
            .ok_or_else(|| {
                CommandError::InvalidInput(format!("unknown game: {}", input.game_id))
            })?;
        let rendered = record
            .events
            .iter()
            .map(|r| r.event.render_omniscient())
            .collect();
        Ok(ShGameReplayOutput { record, rendered })
    }
}

register_command!(ShGameReplay);

// ---------------------------------------------------------------------------
// sh_list_games
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ShListGames;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShListGamesInput {
    pub run_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ShListGamesOutput {
    pub games: Vec<GameSummary>,
}

#[async_trait]
impl Command for ShListGames {
    type Input = ShListGamesInput;
    type Output = ShListGamesOutput;

    fn name(&self) -> &'static str {
        "sh_list_games"
    }
    fn description(&self) -> &'static str {
        "List the stored games of a run (id, schedule cell, winner, rounds)."
    }

    async fn run(
        &self,
        input: ShListGamesInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let store = open_store().await?;
        let games = store
            .list_games(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        Ok(ShListGamesOutput { games })
    }
}

register_command!(ShListGames);
