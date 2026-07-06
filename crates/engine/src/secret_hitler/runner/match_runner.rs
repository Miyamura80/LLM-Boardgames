//! Match orchestration: init → continue → cleanup over a schedule of games,
//! resumable at game granularity (already-stored games are skipped).

use super::game_loop::{run_game, GameConfig};
use super::pools::{AgentFactory, AgentSpec};
use super::record::GameRecord;
use super::schedule::{arena_schedule, controlled_schedule, realized_distribution, GamePlan};
use crate::secret_hitler::metrics::score_game;
use crate::secret_hitler::rating::{leaderboard, LeaderboardRow, RatingTable};
use crate::secret_hitler::state::GameState;
use crate::secret_hitler::store::Store;
use crate::secret_hitler::types::Role;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A match specification — everything needed to (re)build the schedule.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum MatchSpec {
    /// One candidate vs a frozen 6-anchor pool: 3 roles × 7 seats × K games.
    Controlled {
        candidate: AgentSpec,
        pool_name: String,
        pool: Vec<AgentSpec>,
        k: u32,
        match_seed: u64,
    },
    /// Mixed candidate-vs-candidate games with seat rotation.
    Arena {
        models: Vec<AgentSpec>,
        games: u32,
        match_seed: u64,
    },
}

impl MatchSpec {
    pub fn mode(&self) -> &'static str {
        match self {
            MatchSpec::Controlled { .. } => "controlled",
            MatchSpec::Arena { .. } => "arena",
        }
    }

    pub fn schedule(&self) -> Result<Vec<GamePlan>, String> {
        match self {
            MatchSpec::Controlled {
                candidate,
                pool,
                k,
                match_seed,
                ..
            } => controlled_schedule(*match_seed, candidate, pool, *k),
            MatchSpec::Arena {
                models,
                games,
                match_seed,
            } => {
                if models.is_empty() {
                    return Err("arena mode needs at least one model".into());
                }
                Ok(arena_schedule(*match_seed, models, *games))
            }
        }
    }
}

/// Progress snapshot returned by `continue`/status calls.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MatchProgress {
    pub run_id: String,
    pub total_games: usize,
    pub played: usize,
    pub remaining: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Play up to `max_games` outstanding games of `spec`, persisting each one.
pub async fn advance_match(
    store: &Store,
    run_id: &str,
    spec: &MatchSpec,
    factory: &AgentFactory,
    game_cfg_base: &GameConfig,
    max_games: usize,
) -> Result<MatchProgress, String> {
    let plans = spec.schedule()?;
    let total = plans.len();
    let mut played = 0usize;
    let mut skipped = 0usize;
    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;

    for plan in &plans {
        // Scope game ids to the run: identical specs under different run ids
        // must replay rather than silently skip each other's games.
        let mut plan = plan.clone();
        plan.game_id = format!("{run_id}-{}", plan.game_id);
        if store
            .game_exists(&plan.game_id)
            .await
            .map_err(|e| e.to_string())?
        {
            skipped += 1;
            continue;
        }
        if played >= max_games {
            break;
        }
        let record = play_plan(&plan, factory, game_cfg_base).await?;
        let metrics = score_game(&record);
        let inserted = store
            .insert_game(run_id, &record, &metrics)
            .await
            .map_err(|e| e.to_string())?;
        if !inserted {
            // A concurrent resume persisted this game first — theirs counts.
            skipped += 1;
            continue;
        }
        for seat in &record.seats {
            prompt_tokens += seat.usage.prompt_tokens;
            completion_tokens += seat.usage.completion_tokens;
        }
        played += 1;
        tracing::info!(
            game = plan.game_id,
            label = plan.label,
            winner = record.winner.as_str(),
            "game complete"
        );
    }

    Ok(MatchProgress {
        run_id: run_id.to_string(),
        total_games: total,
        played,
        remaining: total - skipped - played,
        prompt_tokens,
        completion_tokens,
    })
}

/// Play one scheduled game.
pub async fn play_plan(
    plan: &GamePlan,
    factory: &AgentFactory,
    base: &GameConfig,
) -> Result<GameRecord, String> {
    let mut agents = Vec::with_capacity(plan.seats.len());
    let mut anchor_flags = Vec::with_capacity(plan.seats.len());
    for (seat, spec) in plan.seats.iter().enumerate() {
        agents.push(factory.build(spec, plan.seed ^ (seat as u64) << 8)?);
        anchor_flags.push(spec.is_anchor);
    }
    let cfg = GameConfig {
        game_id: plan.game_id.clone(),
        seed: plan.seed,
        schedule_label: plan.label.clone(),
        ..base.clone()
    };

    // Forced role arrangements (controlled cells) construct the state up
    // front; run_game re-creates from seed otherwise.
    let record = if let Some(roles) = &plan.roles {
        let roles: [Role; 7] = roles.clone().try_into().map_err(|_| "bad role vector")?;
        run_game_with_state(
            &cfg,
            GameState::with_roles(plan.seed, roles),
            &mut agents,
            &anchor_flags,
        )
        .await
    } else {
        run_game(&cfg, &mut agents, &anchor_flags).await
    };
    Ok(record)
}

/// Wrapper for pre-built states (forced roles). Mirrors `run_game`.
async fn run_game_with_state(
    cfg: &GameConfig,
    state: GameState,
    agents: &mut [Box<dyn crate::secret_hitler::agents::SeatAgent>],
    is_anchor: &[bool],
) -> GameRecord {
    super::game_loop::run_game_from(cfg, state, agents, is_anchor).await
}

/// Recompute ratings from every stored game of a run and produce the summary.
pub async fn finalize_match(
    store: &Store,
    run_id: &str,
    rating_k: f64,
) -> Result<(Vec<LeaderboardRow>, serde_json::Value), String> {
    let records = store.run_records(run_id).await.map_err(|e| e.to_string())?;
    let mut table = RatingTable::default();
    for r in &records {
        table.update(r);
    }
    store
        .save_ratings(run_id, &table)
        .await
        .map_err(|e| e.to_string())?;

    let rows = leaderboard(&table, rating_k);
    let spec = store
        .get_run_spec(run_id)
        .await
        .map_err(|e| e.to_string())?
        .map(|(_, s)| s);
    let dist = spec
        .as_ref()
        .and_then(|s| serde_json::from_value::<MatchSpec>(s.clone()).ok())
        .and_then(|spec| spec.schedule().ok())
        .map(|plans| realized_distribution(&plans));

    let summary = serde_json::json!({
        "leaderboard": rows,
        "games": records.len(),
        "realized_distribution": dist,
    });
    store
        .set_run_status(run_id, "complete", Some(&summary))
        .await
        .map_err(|e| e.to_string())?;
    Ok((rows, summary))
}
