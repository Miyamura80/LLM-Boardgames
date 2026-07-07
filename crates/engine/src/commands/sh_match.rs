//! Secret Hitler match commands: run/resume a match, inspect status,
//! leaderboard, and the run list. All require a configured Postgres store
//! (`docker compose up -d`; `APP__DATABASE_URL` or `DATABASE_URL`).

use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use crate::secret_hitler::rating::{leaderboard, LeaderboardRow};
use crate::secret_hitler::runner::{
    advance_match, finalize_match, AgentFactory, AgentSpec, GameConfig, MatchProgress, MatchSpec,
};
use crate::secret_hitler::store::{database_url, ModelMetricSummary, RunSummary, Store};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Canonical store opener shared by every `sh_*` command.
pub(crate) async fn open_store() -> Result<Store, CommandError> {
    let url = database_url().ok_or_else(|| {
        CommandError::Unsupported(
            "no Postgres configured: start it with `docker compose up -d` and set APP__DATABASE_URL (see .env.example)".into(),
        )
    })?;
    Store::connect(&url)
        .await
        .map_err(|e| CommandError::Other(format!("store: {e}")))
}

/// Parse `bot:<kind>` or an LLM `provider/model` string into a seat spec.
/// Fails fast on unknown bot kinds — before a run row or any game exists.
pub(crate) fn parse_model_spec(s: &str) -> Result<AgentSpec, CommandError> {
    match s.strip_prefix("bot:") {
        Some(kind) => {
            let kind: app_config::AgentKind = kind.parse().map_err(CommandError::InvalidInput)?;
            if kind == app_config::AgentKind::Llm {
                return Err(CommandError::InvalidInput(
                    "LLM seats are written as `provider/model`, not `bot:llm`".into(),
                ));
            }
            Ok(AgentSpec::bot(kind))
        }
        None => Ok(AgentSpec::llm(s)),
    }
}

fn game_config(
    cfg: &app_config::AppConfig,
    input_rounds: Option<u8>,
    beliefs: Option<bool>,
) -> GameConfig {
    GameConfig {
        discussion_rounds: input_rounds.unwrap_or(cfg.secret_hitler.discussion_rounds),
        retry_budget: cfg.secret_hitler.retry_budget,
        belief_checkpoints: beliefs.unwrap_or(true),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// sh_run_match
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ShRunMatch;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShRunMatchInput {
    /// Run id; reuse one to resume an interrupted match. Defaults to a
    /// deterministic id derived from the spec.
    pub run_id: Option<String>,
    /// `controlled` (candidate vs anchor pool) or `arena` (mixed candidates).
    pub mode: String,
    /// Controlled mode: the candidate — `provider/model` or `bot:<kind>`.
    pub candidate: Option<String>,
    /// Controlled mode: named anchor pool from config (default `pool-a`).
    pub pool: Option<String>,
    /// Controlled mode: repetitions per role-seat cell (21·K games).
    pub k: Option<u32>,
    /// Arena mode: the models to seat. Mutually exclusive with `set`.
    pub models: Option<Vec<String>>,
    /// Arena mode: a named model set from config (`model_sets`, e.g.
    /// `frontier` / `cheap`) to seat instead of listing `models`.
    pub set: Option<String>,
    /// Arena mode: number of games.
    pub games: Option<u32>,
    pub match_seed: Option<u64>,
    /// Play at most this many new games this call (chunked continue).
    pub max_games: Option<u32>,
    pub discussion_rounds: Option<u8>,
    pub belief_checkpoints: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ShRunMatchOutput {
    pub run_id: String,
    pub progress: MatchProgress,
    /// Present once every scheduled game is stored (the run finalizes).
    pub leaderboard: Option<Vec<LeaderboardRow>>,
    pub metrics: Option<Vec<ModelMetricSummary>>,
}

#[async_trait]
impl Command for ShRunMatch {
    type Input = ShRunMatchInput;
    type Output = ShRunMatchOutput;

    fn name(&self) -> &'static str {
        "sh_run_match"
    }
    fn description(&self) -> &'static str {
        "Run (or resume) a Secret Hitler eval match: controlled anchor-pool mode or mixed arena mode."
    }

    async fn run(
        &self,
        input: ShRunMatchInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        let match_seed = input.match_seed.unwrap_or(42);

        let spec = match input.mode.as_str() {
            "controlled" => {
                let candidate = input.candidate.as_deref().ok_or_else(|| {
                    CommandError::InvalidInput("controlled mode needs `candidate`".into())
                })?;
                let pool_name = input.pool.clone().unwrap_or_else(|| "pool-a".into());
                let pool_cfg = cfg.secret_hitler.pools.get(&pool_name).ok_or_else(|| {
                    CommandError::InvalidInput(format!(
                        "unknown pool '{pool_name}' (configured: {:?})",
                        cfg.secret_hitler.pools.keys().collect::<Vec<_>>()
                    ))
                })?;
                MatchSpec::Controlled {
                    candidate: parse_model_spec(candidate)?,
                    pool_name,
                    pool: pool_cfg.iter().map(AgentSpec::from_anchor).collect(),
                    k: input.k.unwrap_or(1),
                    match_seed,
                }
            }
            "arena" => {
                // Seat from an explicit `models` list or a named `set` from
                // config — the two are mutually exclusive; providing both is
                // ambiguous, so refuse rather than pick one.
                if input.set.is_some() && input.models.is_some() {
                    return Err(CommandError::InvalidInput(
                        "arena mode takes `models` or `set`, not both".into(),
                    ));
                }
                let models = match (&input.models, &input.set) {
                    (Some(m), _) => m.clone(),
                    (None, Some(set)) => cfg
                        .secret_hitler
                        .model_sets
                        .get(set)
                        .cloned()
                        .ok_or_else(|| {
                            CommandError::InvalidInput(format!(
                                "unknown model set '{set}' (configured: {:?})",
                                cfg.secret_hitler.model_sets.keys().collect::<Vec<_>>()
                            ))
                        })?,
                    (None, None) => Vec::new(),
                };
                if models.is_empty() {
                    return Err(CommandError::InvalidInput(
                        "arena mode needs `models` or a named `set`".into(),
                    ));
                }
                MatchSpec::Arena {
                    models: models
                        .iter()
                        .map(|m| parse_model_spec(m))
                        .collect::<Result<Vec<_>, _>>()?,
                    games: input.games.unwrap_or(21),
                    match_seed,
                }
            }
            other => {
                return Err(CommandError::InvalidInput(format!(
                    "mode must be `controlled` or `arena`, got `{other}`"
                )))
            }
        };

        // Default run id hashes the whole spec: the same seed with a different
        // candidate/pool must be a different run, or resume would silently
        // skip the new candidate's games.
        let run_id = input.run_id.clone().unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            let digest = Sha256::digest(serde_json::to_string(&spec).unwrap_or_default());
            let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
            format!("run-{}-{hex}", spec.mode())
        });
        let store = open_store().await?;
        let spec_json = serde_json::to_value(&spec).unwrap();
        // Resuming an existing run with a different spec would silently mix
        // schedules (stored game ids dedupe against the new plan) — refuse.
        // Insert first (ON CONFLICT DO NOTHING), then validate against
        // whichever spec actually owns the row: checking before the insert
        // would let two concurrent first-time calls both pass.
        store
            .create_run(&run_id, spec.mode(), &spec_json)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        if let Some((_, stored)) = store
            .get_run_spec(&run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?
        {
            if stored != spec_json {
                return Err(CommandError::InvalidInput(format!(
                    "run '{run_id}' already exists with a different match spec; pick a new run_id or repeat the original spec"
                )));
            }
        }

        let factory = AgentFactory::from_app_config(cfg);
        let game_cfg = game_config(cfg, input.discussion_rounds, input.belief_checkpoints);
        let progress = advance_match(
            &store,
            &run_id,
            &spec,
            &factory,
            &game_cfg,
            input.max_games.unwrap_or(u32::MAX) as usize,
        )
        .await
        .map_err(CommandError::Other)?;

        let (board, metrics) = if progress.remaining == 0 {
            let (mut rows, _) = finalize_match(&store, &run_id, cfg.secret_hitler.rating_k)
                .await
                .map_err(CommandError::Other)?;
            // Anchors are off-leaderboard by design; `sh_leaderboard` with
            // `include_anchors` is the explicit opt-in for anchor rows.
            rows.retain(|r| !r.is_anchor);
            let mut metrics = store
                .model_metric_summary(&run_id)
                .await
                .map_err(|e| CommandError::Other(e.to_string()))?;
            metrics.retain(|m| !m.is_anchor);
            (Some(rows), Some(metrics))
        } else {
            (None, None)
        };

        Ok(ShRunMatchOutput {
            run_id,
            progress,
            leaderboard: board,
            metrics,
        })
    }
}

register_command!(ShRunMatch);

// ---------------------------------------------------------------------------
// sh_leaderboard
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ShLeaderboard;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShLeaderboardInput {
    pub run_id: String,
    /// Conservatism factor k in μ − kσ (default from config).
    pub k: Option<f64>,
    /// Include anchor rows (hidden by default).
    #[serde(default)]
    pub include_anchors: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ShLeaderboardOutput {
    pub run_id: String,
    pub rows: Vec<LeaderboardRow>,
    pub metrics: Vec<ModelMetricSummary>,
    /// Reminder that σ-wide rows cannot be separated — printed, not hidden.
    pub uncertainty_note: String,
}

#[async_trait]
impl Command for ShLeaderboard {
    type Input = ShLeaderboardInput;
    type Output = ShLeaderboardOutput;

    fn name(&self) -> &'static str {
        "sh_leaderboard"
    }
    fn description(&self) -> &'static str {
        "Role-conditioned Weng-Lin leaderboard (μ±σ, conservative score, win rates) plus the objective metric summary for a run."
    }

    async fn run(
        &self,
        input: ShLeaderboardInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let cfg = app_config::get_config();
        let store = open_store().await?;
        let table = store
            .load_ratings(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        let k = input.k.unwrap_or(cfg.secret_hitler.rating_k);
        let mut rows = leaderboard(&table, k);
        if !input.include_anchors {
            rows.retain(|r| !r.is_anchor);
        }
        let mut metrics = store
            .model_metric_summary(&input.run_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        if !input.include_anchors {
            metrics.retain(|m| !m.is_anchor);
        }
        Ok(ShLeaderboardOutput {
            run_id: input.run_id,
            rows,
            metrics,
            uncertainty_note: "Ratings are relative to this run's pool; rows flagged high_uncertainty cannot be separated from their neighbours yet — play more games before reading rank order.".into(),
        })
    }
}

register_command!(ShLeaderboard);

// ---------------------------------------------------------------------------
// sh_list_runs
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ShListRuns;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ShListRunsInput {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ShListRunsOutput {
    pub runs: Vec<RunSummary>,
}

#[async_trait]
impl Command for ShListRuns {
    type Input = ShListRunsInput;
    type Output = ShListRunsOutput;

    fn name(&self) -> &'static str {
        "sh_list_runs"
    }
    fn description(&self) -> &'static str {
        "List stored Secret Hitler eval runs with status and game counts."
    }

    async fn run(
        &self,
        _input: ShListRunsInput,
        _cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        let store = open_store().await?;
        let runs = store
            .list_runs()
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        Ok(ShListRunsOutput { runs })
    }
}

register_command!(ShListRuns);
