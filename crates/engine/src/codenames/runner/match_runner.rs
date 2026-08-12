//! Codenames match orchestration: init → continue → cleanup over a schedule,
//! resumable at game granularity (already-stored games are skipped).
//!
//! ```text
//!   init      codenames_run_match  →  create_run(spec)      (id = run_id)
//!   continue  advance_match(..., max_games)  ─ chunked, idempotent
//!               │ game id = {run_id}-{plan.game_id}
//!               │ stored already? → skip; else play → insert
//!   cleanup   finalize_match  →  ratings + summary, status = complete
//! ```
//!
//! The spec is stored, not the schedule: a resume rebuilds the same plan from
//! `(match_seed, cell)` because [`super::schedule`] is a pure function.

use super::game_loop::{run_game, GameConfig};
use super::pools::{AgentFactory, AgentSpec};
use super::record::GameRecord;
use super::schedule::{
    arena_schedule, controlled_schedule, planned_distribution, rotation_imbalance_note, GamePlan,
};
use crate::codenames::metrics::{key_card_for, score_game};
use crate::codenames::rating::{leaderboard, uncertainty_note, LeaderboardRow, RatingTable};
use crate::codenames::store::CodenamesStore;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A match specification — everything needed to (re)build the schedule.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum MatchSpec {
    /// One candidate vs a frozen 3-anchor pool: boards × 2 sides × 2 roles × K.
    Controlled {
        candidate: AgentSpec,
        pool_name: String,
        pool: Vec<AgentSpec>,
        boards: u32,
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
                boards,
                k,
                match_seed,
                ..
            } => controlled_schedule(*match_seed, candidate, pool, *boards, *k),
            MatchSpec::Arena {
                models,
                games,
                match_seed,
            } => arena_schedule(*match_seed, models, *games),
        }
    }
}

/// A [`MatchSpec`] as it is stored on the run row, plus the fingerprint of the
/// effective game config the run was created under.
///
/// `#[serde(flatten)]` keeps the stored JSON shape the schedule rebuilds from
/// (`{"mode": "controlled", …}`) so a plain `MatchSpec` still parses out of it;
/// the fingerprint rides alongside as one extra key, which is why persisting it
/// needs no schema change (`spec` is JSONB).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSpec {
    #[serde(flatten)]
    pub spec: MatchSpec,
    /// Absent on rows written before fingerprinting; such a run resumes
    /// without the check rather than becoming unresumable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_fingerprint: Option<String>,
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
    store: &CodenamesStore,
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
        // The key card is engine state, so scoring reconstructs it from the
        // seed and the pool this run played on.
        let key = key_card_for(&record, &game_cfg_base.wordlist).ok_or_else(|| {
            format!(
                "game {} does not reconstruct against the configured wordlist \
                 (hash {}) — refusing to score it against a foreign key",
                plan.game_id, record.wordlist_hash
            )
        })?;
        let metrics = score_game(&record, &key);
        // Tokens were spent by THIS worker regardless of who wins the insert
        // race below — usage reports real cost, `played` stays insert-aware.
        for seat in &record.seats {
            prompt_tokens += seat.usage.prompt_tokens;
            completion_tokens += seat.usage.completion_tokens;
        }
        let inserted = store
            .insert_game(run_id, &record, &metrics)
            .await
            .map_err(|e| e.to_string())?;
        if !inserted {
            // A concurrent resume persisted this game first — theirs counts.
            skipped += 1;
            continue;
        }
        played += 1;
        tracing::info!(
            game = plan.game_id,
            label = plan.label,
            winner = record.winner.as_str(),
            "codenames game complete"
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
    Ok(run_game(&cfg, &mut agents, &anchor_flags).await)
}

/// Recompute ratings from every stored game of a run and produce the summary.
pub async fn finalize_match(
    store: &CodenamesStore,
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
    // Realized coverage comes from the persisted seat rows, planned coverage
    // from the schedule — the two are reported side by side so a gap is
    // visible rather than implied (US-CN08).
    let realized = store
        .realized_distribution(run_id)
        .await
        .map_err(|e| e.to_string())?;
    let plans = store
        .get_run_spec(run_id)
        .await
        .map_err(|e| e.to_string())?
        .and_then(|(_, s)| serde_json::from_value::<MatchSpec>(s).ok())
        .and_then(|spec| spec.schedule().ok());
    let planned = plans.as_deref().map(planned_distribution);

    let summary = serde_json::json!({
        "leaderboard": rows,
        "games": records.len(),
        "uncertainty_note": uncertainty_note(&rows),
        "realized_distribution": realized,
        "planned_distribution": planned,
        // A schedule that is not a whole number of rotations covers seats
        // unevenly; say so beside the distribution rather than let the reader
        // infer it from the counts.
        "coverage_note": plans.as_deref().and_then(rotation_imbalance_note),
    });
    store
        .set_run_status(run_id, "complete", Some(&summary))
        .await
        .map_err(|e| e.to_string())?;
    Ok((rows, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::agents::CodenamesAgentKind;

    fn pool() -> Vec<AgentSpec> {
        (0..3)
            .map(|_| AgentSpec::bot(CodenamesAgentKind::Random))
            .collect()
    }

    /// The spec — not the schedule — is what a resume rebuilds from, so it must
    /// round-trip through JSON and reproduce the identical plan.
    #[test]
    fn a_stored_spec_rebuilds_the_same_schedule() {
        let spec = MatchSpec::Controlled {
            candidate: AgentSpec::llm("cand"),
            pool_name: "pool-a".into(),
            pool: pool(),
            boards: 2,
            k: 1,
            match_seed: 77,
        };
        let json = serde_json::to_value(&spec).expect("spec serializes");
        assert_eq!(json["mode"], "controlled");
        let restored: MatchSpec = serde_json::from_value(json).expect("spec parses back");
        let (a, b) = (spec.schedule().unwrap(), restored.schedule().unwrap());
        assert_eq!(a.len(), 2 * 4);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.seed, y.seed);
            assert_eq!(x.game_id, y.game_id);
            assert_eq!(x.label, y.label);
        }
    }

    /// Every way an arena spec can fail to describe a schedule worth rating —
    /// no models, the wrong number of them, no games, or absurdly many — is
    /// rejected by `schedule()`, which every caller runs before a run row
    /// exists.
    #[test]
    fn a_degenerate_arena_spec_is_rejected() {
        let arena = |models: Vec<AgentSpec>, games: u32| MatchSpec::Arena {
            models,
            games,
            match_seed: 1,
        };
        let four: Vec<AgentSpec> = (0..4).map(|i| AgentSpec::llm(&format!("m{i}"))).collect();

        assert_eq!(arena(vec![], 4).mode(), "arena");
        assert!(arena(vec![], 4).schedule().is_err(), "no models");
        assert!(
            arena(four[..3].to_vec(), 4).schedule().is_err(),
            "three models"
        );
        assert!(arena(four.clone(), 0).schedule().is_err(), "no games");
        assert!(
            arena(four.clone(), u32::MAX).schedule().is_err(),
            "unbounded"
        );
        assert_eq!(arena(four, 8).schedule().expect("a real batch").len(), 8);
    }

    /// The controlled schedule is materialized in full before anything is
    /// persisted, so its size is bounded at the same point.
    #[test]
    fn an_unbounded_controlled_spec_is_rejected() {
        let spec = MatchSpec::Controlled {
            candidate: AgentSpec::llm("cand"),
            pool_name: "pool-a".into(),
            pool: pool(),
            boards: u32::MAX,
            k: u32::MAX,
            match_seed: 1,
        };
        let err = spec.schedule().expect_err("boards × 4 × k overflows");
        assert!(err.contains("the limit is"), "{err}");
    }
}
