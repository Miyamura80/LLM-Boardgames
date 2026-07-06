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
    /// Role-frequency-weighted aggregate σ (same weights as `overall_mu`).
    pub overall_sigma: f64,
    pub overall_conservative: f64,
    pub total_games: u32,
    /// True when this row's μ±kσ interval overlaps an adjacent row's — i.e.
    /// the rating is still too uncertain to separate it from a neighbour.
    /// On a single-row board (no neighbour to compare) this falls back to
    /// "σ still near the prior". Do not over-read close ranks when set.
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
            let mut overall_sigma = 0.0;
            let mut total_games = 0;
            let mut is_anchor = false;
            for (i, r) in roles.iter().enumerate() {
                let st = r.as_ref().unwrap_or(&default);
                overall_mu += weights[i] * st.mu;
                overall_sigma += weights[i] * st.sigma;
                total_games += st.games;
                is_anchor |= st.is_anchor;
            }
            LeaderboardRow {
                model_id,
                is_anchor,
                liberal: roles[0].as_ref().map(|s| role_line(s, k)),
                fascist: roles[1].as_ref().map(|s| role_line(s, k)),
                hitler: roles[2].as_ref().map(|s| role_line(s, k)),
                overall_mu,
                overall_sigma,
                overall_conservative: overall_mu - k * overall_sigma,
                total_games,
                // Filled in the neighbour pass below once rows are ranked.
                high_uncertainty: false,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.overall_conservative
            .partial_cmp(&a.overall_conservative)
            .unwrap()
    });
    flag_neighbour_overlap(&mut rows, k);
    rows
}

/// Flag a row when its μ±kσ interval overlaps an adjacent (ranked) row's —
/// the honest reading of "too uncertain to separate close models". Two
/// intervals `[μ-kσ, μ+kσ]` overlap iff neither sits entirely beyond the
/// other. A lone row has no neighbour, so it falls back to "σ near the prior".
fn flag_neighbour_overlap(rows: &mut [LeaderboardRow], k: f64) {
    let lo = |r: &LeaderboardRow| r.overall_mu - k * r.overall_sigma;
    let hi = |r: &LeaderboardRow| r.overall_mu + k * r.overall_sigma;
    let overlaps = |a: &LeaderboardRow, b: &LeaderboardRow| lo(a) <= hi(b) && lo(b) <= hi(a);

    let flags: Vec<bool> = (0..rows.len())
        .map(|i| {
            let above = i.checked_sub(1).map(|j| overlaps(&rows[i], &rows[j]));
            let below = rows.get(i + 1).map(|r| overlaps(&rows[i], r));
            match (above, below) {
                // At least one neighbour exists: flag iff an interval overlaps.
                (Some(a), Some(b)) => a || b,
                (Some(a), None) => a,
                (None, Some(b)) => b,
                // Single-row board: no separation to judge — fall back to the
                // prior-width heuristic (σ still mostly the 25/3 Weng-Lin prior).
                (None, None) => rows[i].overall_sigma > 25.0 / 3.0 * 0.75,
            }
        })
        .collect();
    for (row, flag) in rows.iter_mut().zip(flags) {
        row.high_uncertainty = flag;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(table: &mut RatingTable, model: &str, role: Role, mu: f64, sigma: f64) {
        table.entities.insert(
            (model.to_string(), role),
            RatingState {
                mu,
                sigma,
                games: 10,
                wins: 5,
                is_anchor: false,
            },
        );
    }

    /// Two well-separated, confident models: neither interval overlaps, so
    /// neither is flagged.
    #[test]
    fn separated_confident_rows_not_flagged() {
        let mut table = RatingTable::default();
        for role in [Role::Liberal, Role::Fascist, Role::Hitler] {
            entity(&mut table, "strong", role, 35.0, 1.0);
            entity(&mut table, "weak", role, 15.0, 1.0);
        }
        let rows = leaderboard(&table, 2.0);
        assert!(rows.iter().all(|r| !r.high_uncertainty));
    }

    /// Two models a hair apart with wide σ: intervals overlap → both flagged,
    /// even though each row's σ alone might pass an absolute threshold.
    #[test]
    fn overlapping_intervals_flagged() {
        let mut table = RatingTable::default();
        for role in [Role::Liberal, Role::Fascist, Role::Hitler] {
            entity(&mut table, "a", role, 25.5, 3.0);
            entity(&mut table, "b", role, 24.5, 3.0);
        }
        let rows = leaderboard(&table, 2.0);
        assert!(rows.iter().all(|r| r.high_uncertainty));
    }

    /// A confident leader whose interval clears the runner-up's is not flagged,
    /// while the still-uncertain runner-up is.
    #[test]
    fn confident_leader_over_uncertain_field() {
        let mut table = RatingTable::default();
        for role in [Role::Liberal, Role::Fascist, Role::Hitler] {
            entity(&mut table, "leader", role, 40.0, 0.5);
            entity(&mut table, "midA", role, 25.0, 3.0);
            entity(&mut table, "midB", role, 24.0, 3.0);
        }
        let rows = leaderboard(&table, 2.0);
        let by = |id: &str| rows.iter().find(|r| r.model_id == id).unwrap();
        assert!(!by("leader").high_uncertainty);
        assert!(by("midA").high_uncertainty);
        assert!(by("midB").high_uncertainty);
    }
}
