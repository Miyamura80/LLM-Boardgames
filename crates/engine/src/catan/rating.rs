//! Seat-conditioned free-for-all Weng-Lin (OpenSkill) ratings.
//!
//! Rating entity = `(model_id, seat_position)` — turn order is Catan's
//! structural asymmetry, as role is Secret Hitler's. Every game is a 4-team
//! FFA update ranked by final placement (ties share rank). A model occupying
//! two seats in one game updates two distinct entities, so no delta-averaging
//! is needed within a game.

use super::runner::GameRecord;
use super::types::{Seat, PLAYER_COUNT};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skillratings::weng_lin::{weng_lin_multi_team, WengLinConfig, WengLinRating};
use skillratings::MultiTeamOutcome;
use std::collections::BTreeMap;

/// Mutable rating state for one `(model_id, seat)` entity.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RatingState {
    pub mu: f64,
    pub sigma: f64,
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
    pub entities: BTreeMap<(String, Seat), RatingState>,
}

impl RatingTable {
    /// Apply one finished game as a 4-team FFA ranked by placement.
    pub fn update(&mut self, record: &GameRecord) {
        let keys: Vec<(String, Seat)> = record
            .seats
            .iter()
            .map(|s| (s.model_id.clone(), s.seat))
            .collect();
        let ratings: Vec<[WengLinRating; 1]> = keys
            .iter()
            .map(|k| {
                let st = self.entities.get(k).cloned().unwrap_or_default();
                [WengLinRating {
                    rating: st.mu,
                    uncertainty: st.sigma,
                }]
            })
            .collect();
        let teams_and_ranks: Vec<(&[WengLinRating], MultiTeamOutcome)> = ratings
            .iter()
            .zip(&record.seats)
            .map(|(r, s)| (&r[..], MultiTeamOutcome::new(s.placement as usize)))
            .collect();
        let updated = weng_lin_multi_team(&teams_and_ranks, &WengLinConfig::new());

        for ((key, seat), new) in keys.into_iter().zip(&record.seats).zip(updated) {
            let entry = self.entities.entry(key).or_default();
            entry.mu = new[0].rating;
            entry.sigma = new[0].uncertainty;
            entry.games += 1;
            if seat.won {
                entry.wins += 1;
            }
            entry.is_anchor |= seat.is_anchor;
        }
    }
}

/// One per-seat rating line.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SeatLine {
    pub seat: Seat,
    pub mu: f64,
    pub sigma: f64,
    pub games: u32,
    pub wins: u32,
}

/// One leaderboard row: a model with its seat-conditioned lines and the
/// seat-averaged conservative headline score.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LeaderboardRow {
    pub model_id: String,
    pub is_anchor: bool,
    /// Seat-averaged conservative score: mean over seats of (μ − kσ).
    pub overall: f64,
    pub games: u32,
    pub wins: u32,
    pub win_rate: f64,
    pub seats: Vec<SeatLine>,
}

/// Build the leaderboard, sorted by conservative overall score.
pub fn leaderboard(table: &RatingTable, k: f64) -> Vec<LeaderboardRow> {
    let mut by_model: BTreeMap<String, (bool, Vec<SeatLine>)> = BTreeMap::new();
    for ((model_id, seat), st) in &table.entities {
        let entry = by_model
            .entry(model_id.clone())
            .or_insert_with(|| (st.is_anchor, Vec::new()));
        entry.0 |= st.is_anchor;
        entry.1.push(SeatLine {
            seat: *seat,
            mu: st.mu,
            sigma: st.sigma,
            games: st.games,
            wins: st.wins,
        });
    }
    let mut rows: Vec<LeaderboardRow> = by_model
        .into_iter()
        .map(|(model_id, (is_anchor, seats))| {
            let conservative: f64 =
                seats.iter().map(|s| s.mu - k * s.sigma).sum::<f64>() / seats.len().max(1) as f64;
            let games = seats.iter().map(|s| s.games).sum();
            let wins = seats.iter().map(|s| s.wins).sum();
            LeaderboardRow {
                model_id,
                is_anchor,
                overall: conservative,
                games,
                wins,
                win_rate: if games > 0 {
                    wins as f64 / games as f64
                } else {
                    0.0
                },
                seats,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.overall
            .partial_cmp(&a.overall)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

/// Human-facing uncertainty caveat, mirrored from the SH harness philosophy.
pub fn uncertainty_note(rows: &[LeaderboardRow]) -> Option<String> {
    let underplayed = rows
        .iter()
        .filter(|r| r.games < PLAYER_COUNT as u32 * 4)
        .count();
    (underplayed > 0).then(|| {
        format!(
            "{underplayed} model(s) have few games; FFA ratings converge slowly — treat close rows as tied"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catan::runner::{placements, SeatRecord};
    use crate::game_core::Reliability;
    use crate::llm::TokenUsage;

    fn record(models: [&str; 4], vps: [u8; 4]) -> GameRecord {
        let ranks = placements(&vps);
        let winner = ranks.iter().position(|&r| r == 1).unwrap() as Seat;
        GameRecord {
            game_id: "g".into(),
            seed: 0,
            player_count: 4,
            rules_version: "catan-4p-v1".into(),
            schedule_label: "test".into(),
            winner,
            final_vps: vps.to_vec(),
            turns: 10,
            seats: (0..4)
                .map(|i| SeatRecord {
                    seat: i as Seat,
                    model_id: models[i].into(),
                    agent_kind: "bot".into(),
                    scaffold_version: "s".into(),
                    temperature: None,
                    is_anchor: false,
                    final_vp: vps[i],
                    public_vp: vps[i],
                    placement: ranks[i],
                    won: winner == i as Seat,
                    knights_played: 0,
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
    fn winners_gain_and_last_place_loses() {
        let mut table = RatingTable::default();
        for _ in 0..10 {
            table.update(&record(["a", "b", "c", "d"], [10, 8, 6, 4]));
        }
        let base = RatingState::default().mu;
        let a = &table.entities[&("a".to_string(), 0)];
        let d = &table.entities[&("d".to_string(), 3)];
        assert!(a.mu > base, "winner must climb");
        assert!(d.mu < base, "last place must fall");
        assert_eq!(a.wins, 10);

        let rows = leaderboard(&table, 2.0);
        assert_eq!(rows[0].model_id, "a");
        assert_eq!(rows[3].model_id, "d");
    }

    #[test]
    fn ties_move_less_than_decisive_results() {
        let mut tied = RatingTable::default();
        tied.update(&record(["a", "b", "c", "d"], [5, 5, 5, 5]));
        let mut decisive = RatingTable::default();
        decisive.update(&record(["a", "b", "c", "d"], [10, 7, 5, 3]));
        let base = RatingState::default().mu;
        let tied_shift = (tied.entities[&("a".to_string(), 0)].mu - base).abs();
        let decisive_shift = (decisive.entities[&("a".to_string(), 0)].mu - base).abs();
        assert!(tied_shift < decisive_shift);
    }
}
