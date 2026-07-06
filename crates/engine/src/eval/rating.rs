//! Weng-Lin (OpenSkill-family) rating, with the rated entity = **(model, role)**
//! (US-009). Each game is a two-team match — the four Liberal `(model, Liberal)`
//! entities versus the three Fascist `(model, Fascist|Hitler)` entities — updated
//! from the win/loss outcome. A games-weighted **Overall** is derived per model.
//!
//! The rating is deliberately reported *with* its uncertainty: μ (`rating`) and
//! σ (`uncertainty`), plus a conservative `μ − kσ` (k=2). Faction win rates are
//! always reported alongside, never folded into the single number.

use crate::eval::record::GameRecord;
use crate::game::{Faction, Role};
use serde::{Deserialize, Serialize};
use skillratings::weng_lin::{weng_lin_two_teams, WengLinConfig, WengLinRating};
use skillratings::Outcomes;
use std::collections::HashMap;

/// Conservatism factor for the reported score (μ − kσ).
pub const CONSERVATISM_K: f64 = 2.0;

type Key = (String, Role);

/// Per-role rating + descriptive win stats for one model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleRating {
    pub role: Role,
    pub games: u32,
    pub wins: u32,
    pub mu: f64,
    pub sigma: f64,
}

impl RoleRating {
    pub fn conservative(&self) -> f64 {
        self.mu - CONSERVATISM_K * self.sigma
    }
    pub fn win_rate(&self) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            self.wins as f64 / self.games as f64
        }
    }
}

/// A model's full rating card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRating {
    pub model: String,
    pub roles: Vec<RoleRating>,
    pub total_games: u32,
    /// Games-weighted mean of the per-role conservative scores.
    pub overall: f64,
    pub liberal_win_rate: f64,
    /// Regular-Fascist + Hitler games combined.
    pub fascist_win_rate: f64,
}

/// The full leaderboard plus an uncertainty caveat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Leaderboard {
    pub models: Vec<ModelRating>,
    pub total_games: u32,
    /// Set when σ is large relative to rating gaps — separations are unreliable.
    pub uncertainty_warning: Option<String>,
}

/// Compute a leaderboard from a chronologically-ordered slice of game records.
pub fn compute_ratings(records: &[GameRecord]) -> Leaderboard {
    let mut ratings: HashMap<Key, WengLinRating> = HashMap::new();
    let mut games: HashMap<Key, u32> = HashMap::new();
    let mut wins: HashMap<Key, u32> = HashMap::new();
    let config = WengLinConfig::new();

    for rec in records {
        let key = |seat: usize| (rec.agent_of(seat).to_string(), rec.role_of(seat));
        let lib_seats: Vec<usize> = (0..rec.seats.len())
            .filter(|&s| rec.role_of(s).faction() == Faction::Liberal)
            .collect();
        let fasc_seats: Vec<usize> = (0..rec.seats.len())
            .filter(|&s| rec.role_of(s).faction() == Faction::Fascist)
            .collect();

        let t1_keys: Vec<Key> = lib_seats.iter().map(|&s| key(s)).collect();
        let t2_keys: Vec<Key> = fasc_seats.iter().map(|&s| key(s)).collect();
        let t1: Vec<WengLinRating> = t1_keys
            .iter()
            .map(|k| *ratings.entry(k.clone()).or_default())
            .collect();
        let t2: Vec<WengLinRating> = t2_keys
            .iter()
            .map(|k| *ratings.entry(k.clone()).or_default())
            .collect();

        let outcome = if rec.winner == Faction::Liberal {
            Outcomes::WIN
        } else {
            Outcomes::LOSS
        };
        let (n1, n2) = weng_lin_two_teams(&t1, &t2, &outcome, &config);
        write_back(&mut ratings, &t1_keys, &n1);
        write_back(&mut ratings, &t2_keys, &n2);

        // Descriptive stats.
        for k in t1_keys {
            *games.entry(k.clone()).or_default() += 1;
            if rec.winner == Faction::Liberal {
                *wins.entry(k).or_default() += 1;
            }
        }
        for k in t2_keys {
            *games.entry(k.clone()).or_default() += 1;
            if rec.winner == Faction::Fascist {
                *wins.entry(k).or_default() += 1;
            }
        }
    }

    build_leaderboard(&ratings, &games, &wins, records.len() as u32)
}

/// Write updated ratings back, averaging when an entity held multiple seats on
/// the same team this game (same model, same role, two seats).
fn write_back(ratings: &mut HashMap<Key, WengLinRating>, keys: &[Key], updated: &[WengLinRating]) {
    let mut acc: HashMap<Key, (f64, f64, u32)> = HashMap::new();
    for (k, r) in keys.iter().zip(updated.iter()) {
        let e = acc.entry(k.clone()).or_default();
        e.0 += r.rating;
        e.1 += r.uncertainty;
        e.2 += 1;
    }
    for (k, (sum_mu, sum_sigma, n)) in acc {
        ratings.insert(
            k,
            WengLinRating {
                rating: sum_mu / n as f64,
                uncertainty: sum_sigma / n as f64,
            },
        );
    }
}

fn build_leaderboard(
    ratings: &HashMap<Key, WengLinRating>,
    games: &HashMap<Key, u32>,
    wins: &HashMap<Key, u32>,
    total_games: u32,
) -> Leaderboard {
    // Group per-role cards by model.
    let mut by_model: HashMap<String, Vec<RoleRating>> = HashMap::new();
    for ((model, role), r) in ratings {
        let g = games.get(&(model.clone(), *role)).copied().unwrap_or(0);
        let w = wins.get(&(model.clone(), *role)).copied().unwrap_or(0);
        by_model.entry(model.clone()).or_default().push(RoleRating {
            role: *role,
            games: g,
            wins: w,
            mu: r.rating,
            sigma: r.uncertainty,
        });
    }

    let mut models: Vec<ModelRating> = by_model
        .into_iter()
        .map(|(model, mut roles)| {
            roles.sort_by_key(|r| role_order(r.role));
            let total: u32 = roles.iter().map(|r| r.games).sum();
            let overall = if total == 0 {
                0.0
            } else {
                roles
                    .iter()
                    .map(|r| r.conservative() * r.games as f64)
                    .sum::<f64>()
                    / total as f64
            };
            let lib = faction_win_rate(&roles, &[Role::Liberal]);
            let fasc = faction_win_rate(&roles, &[Role::Fascist, Role::Hitler]);
            ModelRating {
                model,
                roles,
                total_games: total,
                overall,
                liberal_win_rate: lib,
                fascist_win_rate: fasc,
            }
        })
        .collect();

    models.sort_by(|a, b| {
        b.overall
            .partial_cmp(&a.overall)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let uncertainty_warning = warn_if_thin(&models);
    Leaderboard {
        models,
        total_games,
        uncertainty_warning,
    }
}

fn faction_win_rate(roles: &[RoleRating], of: &[Role]) -> f64 {
    let (g, w): (u32, u32) = roles
        .iter()
        .filter(|r| of.contains(&r.role))
        .fold((0, 0), |(g, w), r| (g + r.games, w + r.wins));
    if g == 0 {
        0.0
    } else {
        w as f64 / g as f64
    }
}

fn warn_if_thin(models: &[ModelRating]) -> Option<String> {
    let thin = models
        .iter()
        .flat_map(|m| m.roles.iter())
        .any(|r| r.games < 20);
    thin.then(|| {
        "Some (model, role) cells have <20 games; σ is wide and adjacent ranks are not separable."
            .to_string()
    })
}

fn role_order(role: Role) -> u8 {
    match role {
        Role::Liberal => 0,
        Role::Fascist => 1,
        Role::Hitler => 2,
    }
}
