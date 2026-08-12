//! Codenames match commands: resumable batch runs (init → continue → cleanup),
//! leaderboards, and run/game listings. All require Postgres.

use crate::codenames::rating::{leaderboard, uncertainty_note, LeaderboardRow};
use crate::codenames::runner::{
    advance_match, finalize_match, AgentFactory, MatchProgress, MatchSpec, StoredSpec,
};
use crate::codenames::store::{
    database_url, CodenamesStore, GameSummary, ModelMetricSummary, RunSummary,
};
use crate::commands::codenames_game::parse_model_spec;
use crate::commands::codenames_match_spec::{
    base_game_config, config_fingerprint, ensure_fingerprint_matches, ensure_stored_spec_matches,
    resolve_pool,
};
use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub(crate) async fn open_codenames_store() -> Result<CodenamesStore, CommandError> {
    let url = database_url().ok_or_else(|| {
        CommandError::InvalidInput(
            "no database configured: set database_url in config or DATABASE_URL".into(),
        )
    })?;
    CodenamesStore::connect(&url)
        .await
        .map_err(|e| CommandError::Other(format!("store connect failed: {e}")))
}

// ---------------------------------------------------------------------------
// codenames_run_match
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CodenamesRunMatch;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodenamesRunMatchInput {
    /// Run id; reusing an id resumes the stored spec. Default: derived from
    /// the request id.
    pub run_id: Option<String>,
    /// `controlled` (default) or `arena`.
    pub mode: Option<String>,
    /// Controlled: the candidate seat (`provider/model` or `bot:<kind>`).
    pub candidate: Option<String>,
    /// Controlled: named 3-anchor pool from config (default `pool-a`).
    pub pool: Option<String>,
    /// Controlled: distinct board buckets (default 3) and reps per cell
    /// (default 1). Games = boards × 2 sides × 2 roles × k.
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
pub struct CodenamesRunMatchOutput {
    pub run_id: String,
    pub progress: MatchProgress,
    /// Present once the run is complete (all games stored).
    pub leaderboard: Option<Vec<LeaderboardRow>>,
    pub metrics: Option<Vec<ModelMetricSummary>>,
    pub uncertainty_note: Option<String>,
}

#[async_trait]
impl Command for CodenamesRunMatch {
    type Input = CodenamesRunMatchInput;
    type Output = CodenamesRunMatchOutput;

    fn name(&self) -> &'static str {
        "codenames_run_match"
    }
    fn description(&self) -> &'static str {
        "Run or resume a Codenames eval match (controlled candidate-vs-anchors on mirrored boards, or arena), resumable at game granularity; finalizes role-conditioned two-team ratings when all games are stored."
    }

    async fn run(
        &self,
        input: CodenamesRunMatchInput,
        cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        // Resolve everything config-dependent up front: a broken wordlist,
        // vector table, or seat spec must fail before a run row exists.
        let factory = AgentFactory::from_app_config(cfg).map_err(CommandError::InvalidInput)?;
        let base = base_game_config(cfg)?;
        let store = open_codenames_store().await?;
        let run_id = input
            .run_id
            .clone()
            .unwrap_or_else(|| format!("codenames-run-{}", cx.request_id));

        // The effective config this call would play under — rules *and* the
        // agent-side settings every seat is built with; a run is pinned to the
        // one it was created with.
        let fingerprint = config_fingerprint(cfg, &base);

        // Resume from the stored spec, else build one from the input.
        let spec: MatchSpec = match store
            .get_run_spec(&run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?
        {
            Some((_, stored)) => {
                let stored: StoredSpec = serde_json::from_value(stored).map_err(|e| {
                    CommandError::Other(format!("stored spec no longer parses: {e}"))
                })?;
                ensure_fingerprint_matches(
                    &run_id,
                    stored.config_fingerprint.as_deref(),
                    &fingerprint,
                )?;
                stored.spec
            }
            None => {
                let mode = input.mode.as_deref().unwrap_or("controlled");
                let match_seed = input.match_seed.unwrap_or(0xC0DE);
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
                                cfg.codenames.model_sets.get(set).cloned().ok_or_else(|| {
                                    CommandError::InvalidInput(format!(
                                        "unknown codenames model set '{set}'"
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
                // Fail before the run row exists if the schedule is impossible
                // or a seat cannot be built (unknown kind, missing vectors).
                let plans = spec.schedule().map_err(CommandError::InvalidInput)?;
                for seat in plans.first().iter().flat_map(|p| &p.seats) {
                    factory
                        .build(seat, 0)
                        .map_err(CommandError::InvalidInput)
                        .map(drop)?;
                }
                let stored = StoredSpec {
                    spec,
                    config_fingerprint: Some(fingerprint.clone()),
                };
                let value = serde_json::to_value(&stored).expect("spec serializes");
                store
                    .create_run(&run_id, stored.spec.mode(), &value)
                    .await
                    .map_err(|e| CommandError::Other(e.to_string()))?;
                // The insert may have been a no-op (another caller got there
                // first with a different spec), so the row decides, not us.
                let (_, persisted) = store
                    .get_run_spec(&run_id)
                    .await
                    .map_err(|e| CommandError::Other(e.to_string()))?
                    .ok_or_else(|| {
                        CommandError::Other(format!("run '{run_id}' vanished after creation"))
                    })?;
                ensure_stored_spec_matches(&run_id, &value, &persisted)?;
                stored.spec
            }
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
            let (rows, _) = finalize_match(&store, &run_id, cfg.codenames.rating_k)
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

        Ok(CodenamesRunMatchOutput {
            run_id,
            progress,
            uncertainty_note: rows.as_deref().map(uncertainty_note),
            leaderboard: rows,
            metrics,
        })
    }
}

register_command!(CodenamesRunMatch);

// ---------------------------------------------------------------------------
// codenames_leaderboard
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CodenamesLeaderboard;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodenamesLeaderboardInput {
    pub run_id: String,
    /// Conservatism factor in μ − kσ (default from config).
    pub k: Option<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodenamesLeaderboardOutput {
    pub run_id: String,
    /// Rows carry both role lines plus the per-side win-rate diagnostic.
    pub rows: Vec<LeaderboardRow>,
    pub metrics: Vec<ModelMetricSummary>,
    /// Seat-game count plus, when they apply, the caveats about rows too
    /// uncertain to separate. Always present.
    pub uncertainty_note: String,
}

#[async_trait]
impl Command for CodenamesLeaderboard {
    type Input = CodenamesLeaderboardInput;
    type Output = CodenamesLeaderboardOutput;

    fn name(&self) -> &'static str {
        "codenames_leaderboard"
    }
    fn description(&self) -> &'static str {
        "Role-conditioned (spymaster/operative) two-team leaderboard and per-model metric summary for a stored Codenames run, with the per-side win-rate diagnostic."
    }

    async fn run(
        &self,
        input: CodenamesLeaderboardInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        let store = open_codenames_store().await?;
        let table = store
            .load_ratings(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        let rows = leaderboard(&table, input.k.unwrap_or(cfg.codenames.rating_k));
        let metrics = store
            .model_metric_summary(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        let note = uncertainty_note(&rows);
        Ok(CodenamesLeaderboardOutput {
            run_id: input.run_id,
            rows,
            metrics,
            uncertainty_note: note,
        })
    }
}

register_command!(CodenamesLeaderboard);

// ---------------------------------------------------------------------------
// codenames_list_runs / codenames_list_games
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CodenamesListRuns;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodenamesListRunsInput {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodenamesListRunsOutput {
    /// Each row carries `game: "codenames"`, the discriminator the frontend's
    /// cross-game run picker keys on (US-CN11).
    pub runs: Vec<RunSummary>,
}

#[async_trait]
impl Command for CodenamesListRuns {
    type Input = CodenamesListRunsInput;
    type Output = CodenamesListRunsOutput;

    fn name(&self) -> &'static str {
        "codenames_list_runs"
    }
    fn description(&self) -> &'static str {
        "List stored Codenames runs with status and game counts."
    }

    async fn run(
        &self,
        _input: CodenamesListRunsInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let store = open_codenames_store().await?;
        let runs = store
            .list_runs()
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        Ok(CodenamesListRunsOutput { runs })
    }
}

register_command!(CodenamesListRuns);

#[derive(Default)]
pub struct CodenamesListGames;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodenamesListGamesInput {
    pub run_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodenamesListGamesOutput {
    pub games: Vec<GameSummary>,
}

#[async_trait]
impl Command for CodenamesListGames {
    type Input = CodenamesListGamesInput;
    type Output = CodenamesListGamesOutput;

    fn name(&self) -> &'static str {
        "codenames_list_games"
    }
    fn description(&self) -> &'static str {
        "List a stored Codenames run's games with winner, end reason, and schedule cell."
    }

    async fn run(
        &self,
        input: CodenamesListGamesInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let store = open_codenames_store().await?;
        let games = store
            .list_games(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        Ok(CodenamesListGamesOutput { games })
    }
}

register_command!(CodenamesListGames);
