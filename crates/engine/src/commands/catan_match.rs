//! Catan match commands: resumable batch runs (init → continue → cleanup),
//! leaderboards, and run/game listings. All require Postgres.

use crate::catan::rating::{leaderboard, LeaderboardRow};
use crate::catan::runner::AgentFactory;
use crate::catan::runner::{
    advance_match, finalize_match, AgentSpec, GameConfig, MatchProgress, MatchSpec,
};
use crate::catan::store::{database_url, CatanStore, GameSummary, ModelMetricSummary, RunSummary};
use crate::commands::catan_game::{parse_model_spec, turn_caps_from_config};
use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub(crate) async fn open_catan_store() -> Result<CatanStore, CommandError> {
    let url = database_url().ok_or_else(|| {
        CommandError::InvalidInput(
            "no database configured: set database_url in config or DATABASE_URL".into(),
        )
    })?;
    CatanStore::connect(&url)
        .await
        .map_err(|e| CommandError::Other(format!("store connect failed: {e}")))
}

fn resolve_pool(cfg: &app_config::AppConfig, name: &str) -> Result<Vec<AgentSpec>, CommandError> {
    let pool = cfg.catan.pools.get(name).ok_or_else(|| {
        CommandError::InvalidInput(format!(
            "unknown catan pool '{name}' (configured: {:?})",
            cfg.catan.pools.keys().collect::<Vec<_>>()
        ))
    })?;
    pool.iter()
        .map(|a| AgentSpec::from_config(a).map_err(CommandError::InvalidInput))
        .collect()
}

// ---------------------------------------------------------------------------
// catan_run_match
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CatanRunMatch;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CatanRunMatchInput {
    /// Run id; reusing an id resumes the stored spec. Default: derived from
    /// the request id.
    pub run_id: Option<String>,
    /// `controlled` (default) or `arena`.
    pub mode: Option<String>,
    /// Controlled: the candidate seat (`provider/model` or `bot:<kind>`).
    pub candidate: Option<String>,
    /// Controlled: named 3-anchor pool from config (default `pool-a`).
    pub pool: Option<String>,
    /// Controlled: distinct boards (default 3) and reps per cell (default 1).
    pub boards: Option<u32>,
    pub k: Option<u32>,
    /// Arena: explicit models, or a named `model_sets` entry via `set`.
    pub models: Option<Vec<String>>,
    pub set: Option<String>,
    /// Arena: number of games (default 8).
    pub games: Option<u32>,
    pub match_seed: Option<u64>,
    /// Play at most this many new games this call (chunked continue).
    pub max_games: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CatanRunMatchOutput {
    pub run_id: String,
    pub progress: MatchProgress,
    /// Present once the run is complete (all games stored).
    pub leaderboard: Option<Vec<LeaderboardRow>>,
    pub metrics: Option<Vec<ModelMetricSummary>>,
}

#[async_trait]
impl Command for CatanRunMatch {
    type Input = CatanRunMatchInput;
    type Output = CatanRunMatchOutput;

    fn name(&self) -> &'static str {
        "catan_run_match"
    }
    fn description(&self) -> &'static str {
        "Run or resume a Catan eval match (controlled candidate-vs-anchors or arena), resumable at game granularity; finalizes seat-conditioned FFA ratings when all games are stored."
    }

    async fn run(
        &self,
        input: CatanRunMatchInput,
        cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        let store = open_catan_store().await?;
        let run_id = input
            .run_id
            .clone()
            .unwrap_or_else(|| format!("catan-run-{}", cx.request_id));

        // Resume from the stored spec, else build one from the input.
        let spec: MatchSpec = match store
            .get_run_spec(&run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?
        {
            Some((_, stored)) => serde_json::from_value(stored)
                .map_err(|e| CommandError::Other(format!("stored spec no longer parses: {e}")))?,
            None => {
                let mode = input.mode.as_deref().unwrap_or("controlled");
                let match_seed = input.match_seed.unwrap_or(0xCA7A);
                let spec = match mode {
                    "controlled" => {
                        let candidate = input.candidate.as_deref().ok_or_else(|| {
                            CommandError::InvalidInput("controlled mode needs a candidate".into())
                        })?;
                        let pool_name = input.pool.as_deref().unwrap_or("pool-a");
                        MatchSpec::Controlled {
                            candidate: parse_model_spec(candidate)?,
                            pool_name: pool_name.into(),
                            pool: resolve_pool(cfg, pool_name)?,
                            boards: input.boards.unwrap_or(3),
                            k: input.k.unwrap_or(1),
                            match_seed,
                        }
                    }
                    "arena" => {
                        let names: Vec<String> = match (&input.models, &input.set) {
                            (Some(m), _) => m.clone(),
                            (None, Some(set)) => {
                                cfg.catan.model_sets.get(set).cloned().ok_or_else(|| {
                                    CommandError::InvalidInput(format!(
                                        "unknown catan model set '{set}'"
                                    ))
                                })?
                            }
                            (None, None) => {
                                return Err(CommandError::InvalidInput(
                                    "arena mode needs models or set".into(),
                                ))
                            }
                        };
                        let models = names
                            .iter()
                            .map(|m| parse_model_spec(m))
                            .collect::<Result<Vec<_>, _>>()?;
                        MatchSpec::Arena {
                            models,
                            games: input.games.unwrap_or(8),
                            match_seed,
                        }
                    }
                    other => {
                        return Err(CommandError::InvalidInput(format!(
                            "unknown mode '{other}' (controlled | arena)"
                        )))
                    }
                };
                store
                    .create_run(
                        &run_id,
                        spec.mode(),
                        &serde_json::to_value(&spec).expect("spec serializes"),
                    )
                    .await
                    .map_err(|e| CommandError::Other(e.to_string()))?;
                spec
            }
        };

        let factory = AgentFactory::from_app_config(cfg);
        let base = GameConfig {
            retry_budget: cfg.catan.retry_budget,
            caps: turn_caps_from_config(&cfg.catan),
            ..GameConfig::default()
        };
        let progress = advance_match(
            &store,
            &run_id,
            &spec,
            &factory,
            &base,
            input.max_games.unwrap_or(usize::MAX),
        )
        .await
        .map_err(CommandError::Other)?;

        let (rows, metrics) = if progress.remaining == 0 {
            let (rows, _) = finalize_match(&store, &run_id, cfg.catan.rating_k)
                .await
                .map_err(CommandError::Other)?;
            let metrics = store
                .model_metric_summary(&run_id)
                .await
                .map_err(|e| CommandError::Other(e.to_string()))?;
            (Some(rows), Some(metrics))
        } else {
            (None, None)
        };

        Ok(CatanRunMatchOutput {
            run_id,
            progress,
            leaderboard: rows,
            metrics,
        })
    }
}

register_command!(CatanRunMatch);

// ---------------------------------------------------------------------------
// catan_leaderboard
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CatanLeaderboard;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CatanLeaderboardInput {
    pub run_id: String,
    /// Conservatism factor in μ − kσ (default from config).
    pub k: Option<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CatanLeaderboardOutput {
    pub run_id: String,
    pub rows: Vec<LeaderboardRow>,
    pub metrics: Vec<ModelMetricSummary>,
    pub uncertainty_note: Option<String>,
}

#[async_trait]
impl Command for CatanLeaderboard {
    type Input = CatanLeaderboardInput;
    type Output = CatanLeaderboardOutput;

    fn name(&self) -> &'static str {
        "catan_leaderboard"
    }
    fn description(&self) -> &'static str {
        "Seat-conditioned FFA leaderboard and per-model metric summary for a stored Catan run."
    }

    async fn run(
        &self,
        input: CatanLeaderboardInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        let store = open_catan_store().await?;
        let table = store
            .load_ratings(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        let rows = leaderboard(&table, input.k.unwrap_or(cfg.catan.rating_k));
        let metrics = store
            .model_metric_summary(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        let uncertainty_note = crate::catan::rating::uncertainty_note(&rows);
        Ok(CatanLeaderboardOutput {
            run_id: input.run_id,
            rows,
            metrics,
            uncertainty_note,
        })
    }
}

register_command!(CatanLeaderboard);

// ---------------------------------------------------------------------------
// catan_list_runs / catan_list_games
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CatanListRuns;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CatanListRunsInput {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CatanListRunsOutput {
    pub runs: Vec<RunSummary>,
}

#[async_trait]
impl Command for CatanListRuns {
    type Input = CatanListRunsInput;
    type Output = CatanListRunsOutput;

    fn name(&self) -> &'static str {
        "catan_list_runs"
    }
    fn description(&self) -> &'static str {
        "List stored Catan runs with status and game counts."
    }

    async fn run(
        &self,
        _input: CatanListRunsInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let store = open_catan_store().await?;
        let runs = store
            .list_runs()
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        Ok(CatanListRunsOutput { runs })
    }
}

register_command!(CatanListRuns);

#[derive(Default)]
pub struct CatanListGames;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CatanListGamesInput {
    pub run_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CatanListGamesOutput {
    pub games: Vec<GameSummary>,
}

#[async_trait]
impl Command for CatanListGames {
    type Input = CatanListGamesInput;
    type Output = CatanListGamesOutput;

    fn name(&self) -> &'static str {
        "catan_list_games"
    }
    fn description(&self) -> &'static str {
        "List a stored Catan run's games."
    }

    async fn run(
        &self,
        input: CatanListGamesInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let store = open_catan_store().await?;
        let games = store
            .list_games(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        Ok(CatanListGamesOutput { games })
    }
}

register_command!(CatanListGames);
