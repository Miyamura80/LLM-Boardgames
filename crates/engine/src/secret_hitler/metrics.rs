//! Objective, engine-derived per-seat metrics. No LLM judge anywhere.
//!
//! Definitions (documented limitations in `docs/rating-design.md` §4):
//! - **Suspicion Brier**: mean Brier score of the seat's private belief
//!   distributions against ground-truth roles, averaged over checkpoints.
//!   Lower is better. Most meaningful for Liberals (Fascists know the roles).
//! - **Goal-aligned enactment**: of the seat's own non-forced policy choices
//!   *where the tiles left a real choice*, the fraction that advanced the
//!   seat's faction. Luck-controlled: forced hands (all-same tiles) are
//!   excluded from the denominator.
//! - **Faction throughput**: own-faction policies enacted in governments where
//!   the seat personally made a non-forced policy choice. Outcome-correlated;
//!   informs, doesn't adjudicate.
//! - **Execution accuracy**: shots that advanced the shooter's faction
//!   (Liberal hitting a Fascist/Hitler; Fascist hitting a Liberal).
//! - **Hitler survival**: rounds survived by the Hitler seat.

use super::runner::record::GameRecord;
use super::types::{Party, Role, Seat};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SeatMetrics {
    pub seat: Seat,
    /// Mean Brier score across checkpoints (lower = better calibrated).
    pub suspicion_brier: Option<f64>,
    pub goal_aligned_num: u32,
    pub goal_aligned_den: u32,
    pub throughput_num: u32,
    pub throughput_den: u32,
    pub exec_hits: u32,
    pub exec_shots: u32,
    /// Rounds survived as Hitler (None for other roles).
    pub hitler_survived_rounds: Option<u32>,
}

/// Score every seat of a finished game.
pub fn score_game(record: &GameRecord) -> Vec<SeatMetrics> {
    (0..record.seats.len())
        .map(|i| score_seat(record, i as Seat))
        .collect()
}

fn score_seat(record: &GameRecord, seat: Seat) -> SeatMetrics {
    let sr = &record.seats[seat as usize];
    let my_party = sr.role.party();
    let mut m = SeatMetrics {
        seat,
        ..Default::default()
    };

    // -- suspicion calibration ------------------------------------------------
    let mut briers: Vec<f64> = Vec::new();
    for snap in record.beliefs.iter().filter(|b| b.seat == seat) {
        let mut per_opponent = Vec::new();
        for (&subject, probs) in &snap.report.assessments {
            let Some(actual) = record.roles.get(subject as usize) else {
                continue;
            };
            let t = |r: Role| if *actual == r { 1.0 } else { 0.0 };
            let brier = (probs.liberal - t(Role::Liberal)).powi(2)
                + (probs.fascist - t(Role::Fascist)).powi(2)
                + (probs.hitler - t(Role::Hitler)).powi(2);
            per_opponent.push(brier);
        }
        if !per_opponent.is_empty() {
            briers.push(per_opponent.iter().sum::<f64>() / per_opponent.len() as f64);
        }
    }
    if !briers.is_empty() {
        m.suspicion_brier = Some(briers.iter().sum::<f64>() / briers.len() as f64);
    }

    // -- goal-aligned enactment (per decision, luck-controlled) ---------------
    for choice in sr.policy_choices.iter().filter(|c| !c.forced) {
        let held_both = choice.tiles_held.contains(&Party::Liberal)
            && choice.tiles_held.contains(&Party::Fascist);
        if !held_both {
            continue; // no real choice existed
        }
        m.goal_aligned_den += 1;
        // Presidents advance their faction by discarding the enemy tile;
        // Chancellors by enacting their own tile.
        let aligned = if choice.as_president {
            choice.chosen != my_party
        } else {
            choice.chosen == my_party
        };
        if aligned {
            m.goal_aligned_num += 1;
        }
    }

    // -- faction policy throughput (outcome stat, strict attribution) ---------
    use super::events::GameEvent;
    let mut open_gov: Option<(Seat, Seat)> = None;
    for rec in &record.events {
        match &rec.event {
            GameEvent::GovernmentFormed {
                president,
                chancellor,
            } => {
                open_gov = Some((*president, *chancellor));
            }
            GameEvent::PolicyEnacted { policy } => {
                if let Some((p, c)) = open_gov.take() {
                    if (p == seat || c == seat)
                        && sr
                            .policy_choices
                            .iter()
                            .any(|ch| ch.round == rec.round && !ch.forced)
                    {
                        m.throughput_den += 1;
                        if *policy == my_party {
                            m.throughput_num += 1;
                        }
                    }
                }
            }
            GameEvent::TopDeckEnacted { .. } => {
                open_gov = None; // no personal choice → no attribution
            }
            _ => {}
        }
    }

    // -- execution accuracy ----------------------------------------------------
    for shot in sr.executions.iter().filter(|e| !e.forced) {
        m.exec_shots += 1;
        let hit = match my_party {
            Party::Liberal => shot.target_party == Party::Fascist,
            Party::Fascist => shot.target_party == Party::Liberal,
        };
        if hit {
            m.exec_hits += 1;
        }
    }

    // -- Hitler survival --------------------------------------------------------
    if sr.role == Role::Hitler {
        m.hitler_survived_rounds = Some(if sr.survived {
            record.rounds
        } else {
            record
                .events
                .iter()
                .find_map(|r| match &r.event {
                    GameEvent::Executed { target, .. } if *target == seat => Some(r.round),
                    _ => None,
                })
                .unwrap_or(record.rounds)
        });
    }

    m
}
