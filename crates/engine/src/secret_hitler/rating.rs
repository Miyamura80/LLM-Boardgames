//! Role-conditioned Weng-Lin (OpenSkill) ratings.
//!
//! Rating entity = `(model_id, role)` per `docs/rating-design.md` §1. Every
//! game is a two-team update (Liberal seats vs Fascist seats, Hitler included).
//! When one entity occupies several seats in a game, each seat's update is
//! computed independently and the deltas are **averaged** into the entity —
//! self-play games therefore contribute ~zero net signal by construction.

use super::runner::record::GameRecord;
use super::types::{Party, Role};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skillratings::weng_lin::{weng_lin_two_teams, WengLinConfig, WengLinRating};
use skillratings::Outcomes;
use std::collections::BTreeMap;

/// Mutable rating state for one `(model_id, role)` entity.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RatingState {
    pub mu: f64,
    pub sigma: f64,
    /// Seat-occurrences scored (a duplicate model counts once per seat).
    pub games: u32,
    pub wins: u32,
    pub is_anchor: bool,
}

impl Default for RatingState {
    fn default() -> Self {
        let r = WengLinRating::new();
        Self {
            mu: r.rating,
            sigma: r.uncertainty,
            games: 0,
            wins: 0,
            is_anchor: false,
        }
    }
}

/// Rating table over every entity seen in a run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RatingTable {
    pub entities: BTreeMap<(String, Role), RatingState>,
}

impl RatingTable {
    /// Apply one finished game.
    pub fn update(&mut self, record: &GameRecord) {
        let mut lib_seats = Vec::new();
        let mut fasc_seats = Vec::new();
        for seat in &record.seats {
            match seat.role.party() {
                Party::Liberal => lib_seats.push(seat),
                Party::Fascist => fasc_seats.push(seat),
            }
        }

        let key = |s: &&super::runner::record::SeatRecord| (s.model_id.clone(), s.role);
        let current = |table: &Self, k: &(String, Role)| {
            let st = table.entities.get(k).cloned().unwrap_or_default();
            WengLinRating {
                rating: st.mu,
                uncertainty: st.sigma,
            }
        };

        let team_one: Vec<WengLinRating> =
            lib_seats.iter().map(|s| current(self, &key(s))).collect();
        let team_two: Vec<WengLinRating> =
            fasc_seats.iter().map(|s| current(self, &key(s))).collect();
        let outcome = if record.winner == Party::Liberal {
            Outcomes::WIN
        } else {
            Outcomes::LOSS
        };
        let (new_one, new_two) =
            weng_lin_two_teams(&team_one, &team_two, &outcome, &WengLinConfig::new());

        // Collect per-seat deltas, then average per entity.
        let mut deltas: BTreeMap<(String, Role), Vec<(f64, f64)>> = BTreeMap::new();
        for (seat, (old, new)) in lib_seats
            .iter()
            .zip(team_one.iter().zip(new_one.iter()))
            .chain(fasc_seats.iter().zip(team_two.iter().zip(new_two.iter())))
        {
            deltas
                .entry(key(seat))
                .or_default()
                .push((new.rating - old.rating, new.uncertainty - old.uncertainty));
        }

        for (k, ds) in deltas {
            let n = ds.len() as f64;
            let (dmu, dsigma) = ds
                .iter()
                .fold((0.0, 0.0), |(a, b), (x, y)| (a + x / n, b + y / n));
            let entry = self.entities.entry(k).or_default();
            entry.mu += dmu;
            entry.sigma = (entry.sigma + dsigma).max(0.1);
        }

        // Seat-occurrence stats + anchor flags.
        for seat in &record.seats {
            let entry = self
                .entities
                .entry((seat.model_id.clone(), seat.role))
                .or_default();
            entry.games += 1;
            if seat.won {
                entry.wins += 1;
            }
            entry.is_anchor |= seat.is_anchor;
        }
    }
}

/// One leaderboard row per model, role-conditioned per the design doc.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LeaderboardRow {
    pub model_id: String,
    pub is_anchor: bool,
    pub liberal: Option<RoleLine>,
    pub fascist: Option<RoleLine>,
    pub hitler: Option<RoleLine>,
    /// Role-frequency-weighted aggregate: 4/7·Lib + 2/7·Fasc + 1/7·Hitler
    /// (missing roles fall back to the Weng-Lin prior μ=25).
    pub overall_mu: f64,
    pub overall_conservative: f64,
    pub total_games: u32,
    /// True when σ is still too wide to separate this row from its
    /// leaderboard neighbours — do not over-read close ranks.
    pub high_uncertainty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoleLine {
    pub mu: f64,
    pub sigma: f64,
    /// μ − kσ.
    pub conservative: f64,
    pub games: u32,
    pub wins: u32,
    pub win_rate: f64,
}

fn role_line(state: &RatingState, k: f64) -> RoleLine {
    RoleLine {
        mu: state.mu,
        sigma: state.sigma,
        conservative: state.mu - k * state.sigma,
        games: state.games,
        wins: state.wins,
        win_rate: if state.games > 0 {
            state.wins as f64 / state.games as f64
        } else {
            0.0
        },
    }
}

/// Flatten the table into per-model rows sorted by conservative overall score.
pub fn leaderboard(table: &RatingTable, k: f64) -> Vec<LeaderboardRow> {
    let default = RatingState::default();
    let mut models: BTreeMap<String, [Option<RatingState>; 3]> = BTreeMap::new();
    for ((model, role), st) in &table.entities {
        let slot = match role {
            Role::Liberal => 0,
            Role::Fascist => 1,
            Role::Hitler => 2,
        };
        models.entry(model.clone()).or_default()[slot] = Some(st.clone());
    }

    let mut rows: Vec<LeaderboardRow> = models
        .into_iter()
        .map(|(model_id, roles)| {
            let weights = [4.0 / 7.0, 2.0 / 7.0, 1.0 / 7.0];
            let mut overall_mu = 0.0;
            let mut overall_cons = 0.0;
            let mut total_games = 0;
            let mut is_anchor = false;
            let mut max_sigma: f64 = 0.0;
            for (i, r) in roles.iter().enumerate() {
                let st = r.as_ref().unwrap_or(&default);
                overall_mu += weights[i] * st.mu;
                overall_cons += weights[i] * (st.mu - k * st.sigma);
                total_games += st.games;
                is_anchor |= st.is_anchor;
                max_sigma = max_sigma.max(st.sigma);
            }
            LeaderboardRow {
                model_id,
                is_anchor,
                liberal: roles[0].as_ref().map(|s| role_line(s, k)),
                fascist: roles[1].as_ref().map(|s| role_line(s, k)),
                hitler: roles[2].as_ref().map(|s| role_line(s, k)),
                overall_mu,
                overall_conservative: overall_cons,
                total_games,
                // σ barely below the 25/3 prior means the rating is still
                // mostly prior — warn.
                high_uncertainty: max_sigma > 25.0 / 3.0 * 0.75,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.overall_conservative
            .partial_cmp(&a.overall_conservative)
            .unwrap()
    });
    rows
}
