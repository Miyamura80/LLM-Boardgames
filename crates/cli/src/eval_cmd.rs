//! `shbench eval` — run Secret Hitler games/matches with LLM and/or baseline
//! agents, compute ratings + metrics, persist to Postgres, and write a JSON
//! bundle for the frontend / observability artifact.

use anyhow::{Context, Result};
use clap::Args;
use engine::eval::llm::{route, LlmAgent, LlmClient, ProviderKeys, RetryConfig};
use engine::eval::{
    outcome_from_records, run_game, run_match, Agent, BaselineAgent, MatchConfig, MatchOutcome,
    RunnerConfig,
};
use engine::game::{GameConfig, GameState, Role};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Args)]
pub struct EvalArgs {
    /// Comma-separated model specs for a multi-model leaderboard (needs ≥7
    /// distinct, e.g. `gemini/gemini-3-flash-preview,openai/gpt-5,…`).
    #[arg(long)]
    pub models: Option<String>,

    /// One model played in all 7 seats (self-play; rating is degenerate but the
    /// transcript, metrics, and suspicion heatmap are all valid).
    #[arg(long)]
    pub self_play: Option<String>,

    /// Seat all 7 with distinct baselines (zero-token smoke test).
    #[arg(long)]
    pub baseline: bool,

    /// Number of games.
    #[arg(long, default_value_t = 1)]
    pub games: usize,

    /// Discussion rounds before each vote.
    #[arg(long, default_value_t = 3)]
    pub rounds: u8,

    /// Base RNG seed.
    #[arg(long, default_value_t = 1)]
    pub seed: u64,

    /// Skip private belief elicitation (cheaper; disables the suspicion heatmap).
    #[arg(long)]
    pub no_beliefs: bool,

    /// Where to write the JSON bundle. Defaults to `./eval-output/bundle.json`.
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// Skip Postgres persistence even if DATABASE_URL is set.
    #[arg(long)]
    pub no_persist: bool,

    /// Reasoning-effort cap for thinking models ("low"|"medium"|"high"|"none").
    /// Low keeps games fast and structured replies within budget.
    #[arg(long, default_value = "low")]
    pub reasoning: String,

    /// Per-call max output tokens (includes model thinking).
    #[arg(long, default_value_t = 900)]
    pub max_tokens: u32,
}

pub async fn run(args: EvalArgs) {
    if let Err(e) = run_inner(args).await {
        eprintln!("eval error: {e:#}");
        std::process::exit(1);
    }
}

async fn run_inner(args: EvalArgs) -> Result<()> {
    let config = app_config::try_get_config().context("loading config (for API keys)")?;
    let keys = ProviderKeys {
        openai: config.openai_api_key().map(str::to_string),
        anthropic: config.anthropic_api_key().map(str::to_string),
        gemini: config.gemini_api_key().map(str::to_string),
        groq: config.groq_api_key().map(str::to_string),
        perplexity: config.perplexity_api_key().map(str::to_string),
    };
    let retry = RetryConfig {
        max_attempts: config.llm_config.retry.max_attempts.max(1) as u32,
        min_wait_ms: (config.llm_config.retry.min_wait_seconds.max(0) as u64) * 1000,
        max_wait_ms: (config.llm_config.retry.max_wait_seconds.max(1) as u64) * 1000,
    };
    let client = Arc::new(LlmClient::new(retry).context("building LLM client")?);
    let temperature = config.default_llm.default_temperature;
    let max_tokens = args.max_tokens;
    let reasoning: Option<String> = match args.reasoning.as_str() {
        "" | "none" => None,
        other => Some(other.to_string()),
    };

    let runner = RunnerConfig {
        max_attempts: 3,
        discussion_rounds: args.rounds,
        elicit_beliefs: !args.no_beliefs,
    };

    // Agent builder: `baseline*` → scripted; else route as an LLM.
    let keys_ref = keys.clone();
    let client_ref = client.clone();
    let reasoning_ref = reasoning.clone();
    let build_agent = move |spec: &str| -> Box<dyn Agent> {
        if spec.starts_with("baseline") {
            return Box::new(BaselineAgent::named(spec));
        }
        match route(spec, &keys_ref) {
            Ok(endpoint) => Box::new(LlmAgent::with_reasoning(
                client_ref.clone(),
                endpoint,
                temperature,
                max_tokens,
                reasoning_ref.clone(),
            )),
            Err(e) => {
                eprintln!("warning: {spec}: {e}; falling back to a baseline");
                Box::new(BaselineAgent::named(spec))
            }
        }
    };

    let outcome = if args.baseline {
        let roster: Vec<String> = (0..7).map(|i| format!("baseline-{i}")).collect();
        eprintln!("running {} baseline game(s)…", args.games);
        run_match(
            &MatchConfig {
                games: args.games,
                base_seed: args.seed,
                roster,
                runner,
            },
            build_agent,
        )
        .await
    } else if let Some(model) = args.self_play.clone() {
        eprintln!("running {} self-play game(s) of {model}…", args.games);
        self_play(&model, args.games, args.seed, &runner, &build_agent).await
    } else {
        let roster: Vec<String> = args
            .models
            .as_deref()
            .context("provide --models, --self-play, or --baseline")?
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        eprintln!(
            "running {} game(s) over {} models…",
            args.games,
            roster.len()
        );
        run_match(
            &MatchConfig {
                games: args.games,
                base_seed: args.seed,
                roster,
                runner,
            },
            build_agent,
        )
        .await
    };

    report(&outcome);
    persist(&outcome, &args).await;
    write_bundle(&outcome, &args)?;
    Ok(())
}

/// Self-play: seat all 7 with the same model, N games from `seed`.
async fn self_play(
    model: &str,
    games: usize,
    seed: u64,
    runner: &RunnerConfig,
    build_agent: &impl Fn(&str) -> Box<dyn Agent>,
) -> MatchOutcome {
    // Rotate the faction layout each game for role coverage.
    const BASE: [Role; 7] = [
        Role::Liberal,
        Role::Liberal,
        Role::Liberal,
        Role::Liberal,
        Role::Fascist,
        Role::Fascist,
        Role::Hitler,
    ];
    let mut records = Vec::new();
    for k in 0..games {
        let mut roles = [Role::Liberal; 7];
        for (seat, r) in roles.iter_mut().enumerate() {
            *r = BASE[(seat + k) % 7];
        }
        let agents: Vec<Box<dyn Agent>> = (0..7).map(|_| build_agent(model)).collect();
        let game = GameState::new(
            GameConfig::new(seed.wrapping_add(k as u64))
                .with_first_president(k % 7)
                .with_roles(roles),
        );
        let rec = run_game(game, &agents, runner).await;
        eprintln!(
            "  game {}/{}: {:?} win ({:?}), {} tokens",
            k + 1,
            games,
            rec.winner,
            rec.win_reason,
            rec.usage.total_tokens()
        );
        records.push(rec);
    }
    outcome_from_records(records)
}

fn report(outcome: &MatchOutcome) {
    eprintln!("\n=== Leaderboard (μ − 2σ) ===");
    for m in &outcome.leaderboard.models {
        eprintln!(
            "  {:<40} overall {:>7.1}  ({} games, Lib {:.0}% / Fasc {:.0}%)",
            m.model,
            m.overall,
            m.total_games,
            m.liberal_win_rate * 100.0,
            m.fascist_win_rate * 100.0
        );
    }
    if let Some(w) = &outcome.leaderboard.uncertainty_warning {
        eprintln!("  ⚠ {w}");
    }
    let total: u64 = outcome.records.iter().map(|r| r.usage.total_tokens()).sum();
    eprintln!("total LLM tokens: {total}");
}

async fn persist(outcome: &MatchOutcome, args: &EvalArgs) {
    if args.no_persist {
        return;
    }
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL unset; skipping Postgres persistence (bundle still written).");
        return;
    };
    match crate::store::Store::connect(&url).await {
        Ok(store) => {
            let payload = serde_json::to_value(outcome).unwrap_or_default();
            match store.insert_match(outcome.records.len(), &payload).await {
                Ok(match_id) => {
                    for rec in &outcome.records {
                        if let Err(e) = store.insert_game(rec, Some(match_id)).await {
                            eprintln!("warning: failed to persist a game: {e:#}");
                        }
                    }
                    eprintln!(
                        "persisted match {match_id} + {} games to Postgres.",
                        outcome.records.len()
                    );
                }
                Err(e) => eprintln!("warning: failed to persist match: {e:#}"),
            }
        }
        Err(e) => eprintln!("warning: Postgres unavailable ({e:#}); bundle still written."),
    }
}

fn write_bundle(outcome: &MatchOutcome, args: &EvalArgs) -> Result<()> {
    let path = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from("eval-output/bundle.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let json = serde_json::to_string_pretty(outcome)?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    eprintln!("wrote bundle → {}", path.display());
    Ok(())
}
