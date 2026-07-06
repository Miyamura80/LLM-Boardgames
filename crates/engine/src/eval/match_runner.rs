//! The match runner (US-008): plays many games over a roster with balanced
//! role/seat rotation, enforcing the **no-model-on-both-teams** invariant (each
//! game seats 7 *distinct* models, so a win/loss attributes cleanly). Structured
//! as init → advance → finalize for resumability at match granularity.

use crate::eval::agent::Agent;
use crate::eval::metrics::{aggregate, game_metrics, ModelRoleMetrics};
use crate::eval::rating::{compute_ratings, Leaderboard};
use crate::eval::record::GameRecord;
use crate::eval::runner::{run_game, RunnerConfig};
use crate::game::{GameConfig, GameState, Role, NUM_PLAYERS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Base faction layout rotated per game: seats 0–3 Liberal, 4–5 Fascist, 6 Hitler.
const BASE_LAYOUT: [Role; NUM_PLAYERS] = [
    Role::Liberal,
    Role::Liberal,
    Role::Liberal,
    Role::Liberal,
    Role::Fascist,
    Role::Fascist,
    Role::Hitler,
];

#[derive(Debug, Clone)]
pub struct MatchConfig {
    pub games: usize,
    pub base_seed: u64,
    /// Model specs (e.g. `gemini/…`, or distinct baseline names). Must contain
    /// **at least 7 distinct** entries so every game seats 7 distinct models.
    pub roster: Vec<String>,
    pub runner: RunnerConfig,
}

/// One scheduled game: seed, the model in each seat, and the seat→role layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeatPlan {
    pub seed: u64,
    pub seat_models: Vec<String>,
    pub roles: [Role; NUM_PLAYERS],
    pub first_president: usize,
}

/// The realized (model → role) count distribution, for the fairness log.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Distribution {
    /// model → (role → games in that role)
    pub by_model: BTreeMap<String, BTreeMap<String, u32>>,
}

/// Everything a finished match produces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchOutcome {
    pub records: Vec<GameRecord>,
    pub leaderboard: Leaderboard,
    pub metrics: Vec<ModelRoleMetrics>,
    pub distribution: Distribution,
}

/// Build the balanced schedule. Rotates both the seat→model map and the faction
/// layout each game so, over enough games, every model rotates through seats and
/// both factions. Panics if the roster lacks 7 distinct models.
pub fn plan_match(cfg: &MatchConfig) -> Vec<SeatPlan> {
    let distinct: std::collections::BTreeSet<&String> = cfg.roster.iter().collect();
    assert!(
        distinct.len() >= NUM_PLAYERS,
        "roster needs >= {NUM_PLAYERS} distinct models to avoid a model on both teams (got {})",
        distinct.len()
    );
    let n = cfg.roster.len();

    (0..cfg.games)
        .map(|k| {
            // 7 consecutive roster indices (mod n, n>=7) are distinct.
            let seat_models: Vec<String> = (0..NUM_PLAYERS)
                .map(|j| cfg.roster[(k * NUM_PLAYERS + j) % n].clone())
                .collect();
            // Rotate the faction layout so models cycle through roles.
            let mut roles = [Role::Liberal; NUM_PLAYERS];
            for (seat, r) in roles.iter_mut().enumerate() {
                *r = BASE_LAYOUT[(seat + k) % NUM_PLAYERS];
            }
            SeatPlan {
                seed: cfg.base_seed.wrapping_add(k as u64),
                seat_models,
                roles,
                first_president: k % NUM_PLAYERS,
            }
        })
        .collect()
}

/// Builds a fresh agent for a model spec (an LLM agent, or a baseline).
type AgentBuilder<'a> = Box<dyn Fn(&str) -> Box<dyn Agent> + 'a>;

/// Resumable match state.
pub struct Match<'a> {
    plans: Vec<SeatPlan>,
    next: usize,
    records: Vec<GameRecord>,
    runner: RunnerConfig,
    build_agent: AgentBuilder<'a>,
}

impl<'a> Match<'a> {
    /// init: build the schedule.
    pub fn init(cfg: &MatchConfig, build_agent: impl Fn(&str) -> Box<dyn Agent> + 'a) -> Self {
        Self {
            plans: plan_match(cfg),
            next: 0,
            records: Vec::new(),
            runner: cfg.runner.clone(),
            build_agent: Box::new(build_agent),
        }
    }

    pub fn remaining(&self) -> usize {
        self.plans.len().saturating_sub(self.next)
    }

    /// advance: play the next scheduled game, returning its record.
    pub async fn advance(&mut self) -> Option<&GameRecord> {
        let plan = self.plans.get(self.next)?.clone();
        let agents: Vec<Box<dyn Agent>> = plan
            .seat_models
            .iter()
            .map(|m| (self.build_agent)(m))
            .collect();
        let game = GameState::new(
            GameConfig::new(plan.seed)
                .with_first_president(plan.first_president)
                .with_roles(plan.roles),
        );
        let rec = run_game(game, &agents, &self.runner).await;
        self.records.push(rec);
        self.next += 1;
        self.records.last()
    }

    /// Play all remaining games.
    pub async fn run_all(&mut self) {
        while self.remaining() > 0 {
            self.advance().await;
        }
    }

    /// finalize: compute ratings, metrics, and the fairness distribution.
    pub fn finalize(self) -> MatchOutcome {
        let leaderboard = compute_ratings(&self.records);
        let per_game: Vec<_> = self.records.iter().map(game_metrics).collect();
        let metrics = aggregate(&per_game);
        let distribution = realized_distribution(&self.records);
        MatchOutcome {
            records: self.records,
            leaderboard,
            metrics,
            distribution,
        }
    }
}

/// Convenience: run a whole match end to end.
pub async fn run_match(
    cfg: &MatchConfig,
    build_agent: impl Fn(&str) -> Box<dyn Agent>,
) -> MatchOutcome {
    let mut m = Match::init(cfg, build_agent);
    m.run_all().await;
    m.finalize()
}

fn realized_distribution(records: &[GameRecord]) -> Distribution {
    let mut d = Distribution::default();
    for rec in records {
        for seat in &rec.seats {
            let role = role_name(seat.role);
            *d.by_model
                .entry(seat.agent.clone())
                .or_default()
                .entry(role.to_string())
                .or_default() += 1;
        }
    }
    d
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Liberal => "liberal",
        Role::Fascist => "fascist",
        Role::Hitler => "hitler",
    }
}
