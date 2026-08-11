//! Role-conditioned **two-team** Weng-Lin (OpenSkill) ratings
//! (PRD-codenames-evals US-CN09).
//!
//! Codenames follows Secret Hitler's template, not Catan's: a game is a
//! win/loss between two teams, never a placement ranking, so
//! [`weng_lin_two_teams`] is the right primitive and `weng_lin_multi_team` is
//! not. The rating entity is `(model_id, role)` with role ∈ {spymaster,
//! operative} — the two jobs are different skills and are rated apart:
//!
//! ```text
//!   team A  = [seat 0 spymaster, seat 1 operative]   ─┐
//!                                                     ├─ one WIN/LOSS update
//!   team B  = [seat 2 spymaster, seat 3 operative]   ─┘
//!
//!   entity  = (model_id, role)          headline = mean over roles of μ − kσ
//! ```
//!
//! **Duplicate entities average their deltas** (the SH rule): when one model
//! holds the same role on both teams, each seat's update is computed and the
//! deltas are averaged into the single entity, so a self-play game contributes
//! ≈ zero net signal instead of double-counting.
//!
//! **Side is not in the key.** Team A always starts and always holds nine
//! agents, so "side" is a fixed structural advantage — but the controlled
//! schedule mirrors every candidate across both sides
//! ([`super::runner::schedule`]), which cancels it by construction. Rating it
//! would split every entity's evidence in half for no gain. It is reported
//! instead as a per-side win-rate **diagnostic** on each row: if the two side
//! win rates diverge wildly, the mirroring assumption deserves a second look.

use super::runner::record::{GameRecord, SeatRecord};
use super::types::{Role, Team};
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

/// Win/loss counts for one `(model_id, side)` pair — diagnostic only.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
pub struct SideStats {
    pub games: u32,
    pub wins: u32,
}

/// Rating table over every entity seen in a run, plus the side diagnostic.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RatingTable {
    pub entities: BTreeMap<(String, Role), RatingState>,
    /// `(model_id, side)` → win/loss counts. Never feeds the rating.
    pub sides: BTreeMap<(String, Team), SideStats>,
}

impl RatingTable {
    /// Apply one finished game as a two-team win/loss update.
    pub fn update(&mut self, record: &GameRecord) {
        let mut a_seats: Vec<&SeatRecord> = Vec::new();
        let mut b_seats: Vec<&SeatRecord> = Vec::new();
        for seat in &record.seats {
            match seat.team {
                Team::A => a_seats.push(seat),
                Team::B => b_seats.push(seat),
            }
        }

        let key = |s: &&SeatRecord| (s.model_id.clone(), s.role);
        let current = |table: &Self, k: &(String, Role)| {
            let st = table.entities.get(k).cloned().unwrap_or_default();
            WengLinRating {
                rating: st.mu,
                uncertainty: st.sigma,
            }
        };

        let team_one: Vec<WengLinRating> = a_seats.iter().map(|s| current(self, &key(s))).collect();
        let team_two: Vec<WengLinRating> = b_seats.iter().map(|s| current(self, &key(s))).collect();
        let outcome = if record.winner == Team::A {
            Outcomes::WIN
        } else {
            Outcomes::LOSS
        };
        let (new_one, new_two) =
            weng_lin_two_teams(&team_one, &team_two, &outcome, &WengLinConfig::new());

        // Collect per-seat deltas, then average per entity (the SH rule).
        let mut deltas: BTreeMap<(String, Role), Vec<(f64, f64)>> = BTreeMap::new();
        for (seat, (old, new)) in a_seats
            .iter()
            .zip(team_one.iter().zip(new_one.iter()))
            .chain(b_seats.iter().zip(team_two.iter().zip(new_two.iter())))
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

        // Seat-occurrence stats, anchor flags, and the side diagnostic.
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

            let side = self
                .sides
                .entry((seat.model_id.clone(), seat.team))
                .or_default();
            side.games += 1;
            if seat.won {
                side.wins += 1;
            }
        }
    }
}

/// One role's rating line. Both are always reported: the spymaster/operative
/// split is the honest story, the headline exists only to rank.
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

/// Per-side win-rate diagnostic (never part of the rating).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SideLine {
    /// `starting` (team A, nine agents, moves first) or `second`.
    pub side: String,
    pub games: u32,
    pub wins: u32,
    pub win_rate: f64,
}

/// One leaderboard row per model.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LeaderboardRow {
    pub model_id: String,
    pub is_anchor: bool,
    pub spymaster: Option<RoleLine>,
    pub operative: Option<RoleLine>,
    /// Role-averaged μ (equal weights: the schedule gives equal role coverage).
    /// A role never played falls back to the Weng-Lin prior μ = 25.
    pub overall_mu: f64,
    /// Role-averaged σ, same weights.
    pub overall_sigma: f64,
    /// The headline: μ − kσ of the role-averaged numbers.
    pub overall_conservative: f64,
    pub total_games: u32,
    /// Diagnostic only — starting-side and second-side win rates.
    pub sides: Vec<SideLine>,
    /// True when this row's μ±kσ interval overlaps an adjacent row's — the
    /// rating cannot yet separate it from a neighbour. On a single-row board
    /// this falls back to "σ still near the prior". Do not over-read close
    /// ranks when set.
    pub high_uncertainty: bool,
}

fn role_line(state: &RatingState, k: f64) -> RoleLine {
    RoleLine {
        mu: state.mu,
        sigma: state.sigma,
        conservative: state.mu - k * state.sigma,
        games: state.games,
        wins: state.wins,
        win_rate: rate(state.wins, state.games),
    }
}

fn rate(wins: u32, games: u32) -> f64 {
    if games > 0 {
        wins as f64 / games as f64
    } else {
        0.0
    }
}

/// Flatten the table into per-model rows sorted by the conservative headline.
pub fn leaderboard(table: &RatingTable, k: f64) -> Vec<LeaderboardRow> {
    let default = RatingState::default();
    let mut models: BTreeMap<String, [Option<RatingState>; 2]> = BTreeMap::new();
    for ((model, role), st) in &table.entities {
        let slot = match role {
            Role::Spymaster => 0,
            Role::Operative => 1,
        };
        models.entry(model.clone()).or_default()[slot] = Some(st.clone());
    }

    let mut rows: Vec<LeaderboardRow> = models
        .into_iter()
        .map(|(model_id, roles)| {
            let mut overall_mu = 0.0;
            let mut overall_sigma = 0.0;
            let mut total_games = 0;
            let mut is_anchor = false;
            for r in &roles {
                let st = r.as_ref().unwrap_or(&default);
                overall_mu += st.mu / 2.0;
                overall_sigma += st.sigma / 2.0;
                total_games += st.games;
                is_anchor |= st.is_anchor;
            }
            let sides = [Team::A, Team::B]
                .into_iter()
                .filter_map(|team| {
                    let st = table.sides.get(&(model_id.clone(), team))?;
                    Some(SideLine {
                        side: side_name(team).into(),
                        games: st.games,
                        wins: st.wins,
                        win_rate: rate(st.wins, st.games),
                    })
                })
                .collect();
            LeaderboardRow {
                model_id,
                is_anchor,
                spymaster: roles[0].as_ref().map(|s| role_line(s, k)),
                operative: roles[1].as_ref().map(|s| role_line(s, k)),
                overall_mu,
                overall_sigma,
                overall_conservative: overall_mu - k * overall_sigma,
                total_games,
                sides,
                // Filled in the neighbour pass below once rows are ranked.
                high_uncertainty: false,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.overall_conservative
            .partial_cmp(&a.overall_conservative)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    flag_neighbour_overlap(&mut rows, k);
    rows
}

/// The schedule-mirrored side a team represents.
pub fn side_name(team: Team) -> &'static str {
    match team {
        Team::A => "starting",
        Team::B => "second",
    }
}

/// Flag a row when its μ±kσ interval overlaps an adjacent (ranked) row's — the
/// honest reading of "too uncertain to separate close models" (the SH rule).
fn flag_neighbour_overlap(rows: &mut [LeaderboardRow], k: f64) {
    let lo = |r: &LeaderboardRow| r.overall_mu - k * r.overall_sigma;
    let hi = |r: &LeaderboardRow| r.overall_mu + k * r.overall_sigma;
    let overlaps = |a: &LeaderboardRow, b: &LeaderboardRow| lo(a) <= hi(b) && lo(b) <= hi(a);

    let flags: Vec<bool> = (0..rows.len())
        .map(|i| {
            let above = i.checked_sub(1).map(|j| overlaps(&rows[i], &rows[j]));
            let below = rows.get(i + 1).map(|r| overlaps(&rows[i], r));
            match (above, below) {
                (Some(a), Some(b)) => a || b,
                (Some(a), None) => a,
                (None, Some(b)) => b,
                // Single-row board: no separation to judge — fall back to the
                // prior-width heuristic (σ still mostly the 25/3 prior).
                (None, None) => rows[i].overall_sigma > 25.0 / 3.0 * 0.75,
            }
        })
        .collect();
    for (row, flag) in rows.iter_mut().zip(flags) {
        row.high_uncertainty = flag;
    }
}

/// Human-facing caveat carrying the game count, emitted next to every
/// leaderboard (US-CN09: "output includes game count and an uncertainty
/// warning").
pub fn uncertainty_note(rows: &[LeaderboardRow]) -> Option<String> {
    let games: u32 = rows.iter().map(|r| r.total_games).sum();
    let flagged = rows.iter().filter(|r| r.high_uncertainty).count();
    let thin = rows
        .iter()
        .filter(|r| r.spymaster.is_none() || r.operative.is_none())
        .count();
    if flagged == 0 && thin == 0 {
        return None;
    }
    let mut parts = vec![format!("{games} seat-games scored")];
    if flagged > 0 {
        parts.push(format!(
            "{flagged} row(s) overlap a neighbour's uncertainty interval — treat them as tied"
        ));
    }
    if thin > 0 {
        parts.push(format!(
            "{thin} row(s) have played only one role; their headline uses the μ=25 prior for the other"
        ));
    }
    Some(parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::types::{seat_role, seat_team, EndReason, Seat, SEAT_COUNT};
    use crate::game_core::Reliability;
    use crate::llm::TokenUsage;

    /// A record whose four seats carry `models[seat]` and `winner` wins.
    fn record(models: [&str; 4], winner: Team) -> GameRecord {
        GameRecord {
            game_id: "g".into(),
            seed: 0,
            rules_version: "codenames-v1".into(),
            wordlist_hash: "0".repeat(64),
            schedule_label: "test".into(),
            winner,
            end_reason: EndReason::AgentsFound,
            turns: 6,
            seats: (0..SEAT_COUNT)
                .map(|seat| SeatRecord {
                    seat: seat as Seat,
                    role: seat_role(seat),
                    team: seat_team(seat),
                    model_id: models[seat as usize].into(),
                    agent_kind: "bot".into(),
                    scaffold_version: "s".into(),
                    temperature: None,
                    is_anchor: false,
                    won: seat_team(seat) == winner,
                    reliability: Reliability::default(),
                    thoughts: vec![],
                    usage: TokenUsage::default(),
                })
                .collect(),
            events: vec![],
            duration_ms: 0,
        }
    }

    #[test]
    fn winners_climb_and_losers_fall_per_role() {
        let mut table = RatingTable::default();
        for _ in 0..10 {
            table.update(&record(["win-sm", "win-op", "lose-sm", "lose-op"], Team::A));
        }
        let base = RatingState::default().mu;
        assert!(table.entities[&("win-sm".into(), Role::Spymaster)].mu > base);
        assert!(table.entities[&("win-op".into(), Role::Operative)].mu > base);
        assert!(table.entities[&("lose-sm".into(), Role::Spymaster)].mu < base);

        let rows = leaderboard(&table, 2.0);
        assert!(rows[0].model_id.contains("win"));
        assert!(rows.last().unwrap().model_id.contains("lose"));
    }

    /// One model in both roles is two entities, not one: the spymaster and
    /// operative lines move independently.
    #[test]
    fn roles_are_rated_as_distinct_entities() {
        let mut table = RatingTable::default();
        for _ in 0..8 {
            // `dual` spymasters for the winning team and operates for the loser.
            table.update(&record(["dual", "mate", "foe", "dual"], Team::A));
        }
        let sm = &table.entities[&("dual".to_string(), Role::Spymaster)];
        let op = &table.entities[&("dual".to_string(), Role::Operative)];
        assert!(
            sm.mu > op.mu,
            "the winning role must outrank the losing one"
        );
        assert_eq!(sm.wins, 8);
        assert_eq!(op.wins, 0);

        let row = leaderboard(&table, 2.0)
            .into_iter()
            .find(|r| r.model_id == "dual")
            .expect("dual row");
        assert!(row.spymaster.is_some() && row.operative.is_some());
        assert_eq!(row.total_games, 16);
    }

    /// A model holding the same role on both teams must not double-count: the
    /// two seat deltas cancel into ≈ zero net movement.
    #[test]
    fn duplicate_entities_average_their_deltas() {
        let mut table = RatingTable::default();
        let base = RatingState::default().mu;
        table.update(&record(["same", "a-op", "same", "b-op"], Team::A));
        let st = &table.entities[&("same".to_string(), Role::Spymaster)];
        assert!(
            (st.mu - base).abs() < 1e-9,
            "self-play must contribute no net signal, moved to {}",
            st.mu
        );
        assert_eq!(st.games, 2, "both seat occurrences still count");
        assert_eq!(st.wins, 1);
    }

    /// Side is a diagnostic, not a rating key: it shows up on the row and
    /// nowhere else.
    #[test]
    fn side_win_rates_are_reported_separately() {
        let mut table = RatingTable::default();
        table.update(&record(["m", "x", "y", "z"], Team::A));
        table.update(&record(["p", "q", "m", "r"], Team::A));
        let row = leaderboard(&table, 2.0)
            .into_iter()
            .find(|r| r.model_id == "m")
            .expect("m row");
        let sides: BTreeMap<&str, &SideLine> =
            row.sides.iter().map(|s| (s.side.as_str(), s)).collect();
        assert_eq!(sides["starting"].wins, 1);
        assert_eq!(sides["second"].wins, 0);
        // ...and the rating key knows nothing about it.
        assert!(table.entities.keys().all(|(m, _)| m != "starting"));
    }

    #[test]
    fn the_uncertainty_note_carries_the_game_count() {
        let mut table = RatingTable::default();
        table.update(&record(["a", "b", "c", "d"], Team::A));
        let rows = leaderboard(&table, 2.0);
        let note = uncertainty_note(&rows).expect("a one-game run is uncertain");
        assert!(note.contains("seat-games scored"), "{note}");
    }
}
