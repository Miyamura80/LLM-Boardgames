//! Codenames match schedules.
//!
//! **Controlled** (primary): one candidate + three frozen anchors, enumerating
//! `board_bucket (B) × side (2) × role (2) × rep (K)` cells
//! (PRD-codenames-evals US-CN08). Because the four seats are fixed —
//! `0 = A spymaster, 1 = A operative, 2 = B spymaster, 3 = B operative` — the
//! `(side, role)` pair *is* the candidate's seat:
//!
//! ```text
//!            role →   spymaster   operative
//!   side ↓
//!   starting (team A)   seat 0      seat 1
//!   second   (team B)   seat 2      seat 3
//! ```
//!
//! Every cell's game seed is `cell_seed(match_seed, "codenames-controlled",
//! board, side_role, rep)` — a pure function of the cell, **independent of the
//! candidate**. The grid and key card derive from that seed alone
//! ([`Board::generate`](crate::codenames::board::Board::generate)), so two
//! candidates run through the same cell face the identical 25 words and the
//! identical 9/8/7/1 layout, from the same role on the same side. That is the
//! variance control the rating rests on: a candidate cannot draw an easier
//! board than its rivals, only play the same board better.
//!
//! The anchor arrangement is **fixed, not shuffled** — the opposite of Catan's
//! rotation, and deliberately so. US-CN08 wants the anchor teammate to hold the
//! partner confound constant: a candidate spymaster is always read by the same
//! operative, so a role's measured skill is attributable to the candidate
//! rather than to which anchor it happened to be paired with. `pool[0]` is
//! always the teammate; `pool[1]`/`pool[2]` always take the opposing
//! spymaster/operative seats.
//!
//! **Arena** (validation): 4-model sets rotated one seat per game, so each
//! model visits each seat evenly; the realized distribution is logged against
//! [`planned_distribution`] at finalize time.

use super::pools::AgentSpec;
use crate::codenames::types::{seat_role, seat_team, Seat, SEAT_COUNT};
use crate::game_core::cell_seed;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A fully determined single game to play.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GamePlan {
    pub game_id: String,
    /// Provenance: `controlled/b<board>/<side>/<role>/r<rep>` or `arena/g<idx>`.
    pub label: String,
    pub seed: u64,
    /// Seat agents, index = seat (0 = A spymaster … 3 = B operative).
    pub seats: Vec<AgentSpec>,
}

/// The four candidate cells, in `side_role` order: the index *is* the seat the
/// candidate occupies, and `side_role / 2` is the side (0 = starting team).
pub const CANDIDATE_CELLS: [Seat; SEAT_COUNT as usize] = [0, 1, 2, 3];

/// `starting` (the nine-agent team that moves first) or `second`.
pub fn side_label(seat: Seat) -> &'static str {
    match seat_team(seat) {
        crate::codenames::types::Team::A => "starting",
        crate::codenames::types::Team::B => "second",
    }
}

/// Controlled schedule: `boards × 2 sides × 2 roles × k` games for one
/// candidate against a frozen 3-anchor pool.
pub fn controlled_schedule(
    match_seed: u64,
    candidate: &AgentSpec,
    pool: &[AgentSpec],
    boards: u32,
    k: u32,
) -> Result<Vec<GamePlan>, String> {
    if pool.len() != (SEAT_COUNT - 1) as usize {
        return Err(format!(
            "a controlled codenames pool must hold exactly {} anchors, got {}",
            SEAT_COUNT - 1,
            pool.len()
        ));
    }
    if boards == 0 || k == 0 {
        return Err("boards and k must both be at least 1".into());
    }

    let mut plans = Vec::with_capacity((boards * SEAT_COUNT as u32 * k) as usize);
    for board in 0..boards {
        for candidate_seat in CANDIDATE_CELLS {
            for rep in 0..k {
                // Candidate-independent: the seed depends on the cell only.
                let seed = cell_seed(
                    match_seed,
                    "codenames-controlled",
                    board as u64,
                    candidate_seat as u64,
                    rep as u64,
                );
                let seats = seat_candidate(candidate, pool, candidate_seat);
                plans.push(GamePlan {
                    game_id: format!("{seed:016x}"),
                    label: format!(
                        "controlled/b{board}/{}/{}/r{rep}",
                        side_label(candidate_seat),
                        seat_role(candidate_seat).as_str()
                    ),
                    seed,
                    seats,
                });
            }
        }
    }
    Ok(plans)
}

/// Place the candidate at `candidate_seat` and the three frozen anchors around
/// it: `pool[0]` is always the teammate (constant partner), `pool[1..]` fill
/// the opposing seats in seat order.
fn seat_candidate(
    candidate: &AgentSpec,
    pool: &[AgentSpec],
    candidate_seat: Seat,
) -> Vec<AgentSpec> {
    let team = seat_team(candidate_seat);
    let mut opposing = pool[1..].iter();
    (0..SEAT_COUNT)
        .map(|s| {
            if s == candidate_seat {
                let mut c = candidate.clone();
                c.is_anchor = false;
                c
            } else {
                let mut a = if seat_team(s) == team {
                    pool[0].clone()
                } else {
                    opposing.next().expect("two opposing anchors").clone()
                };
                a.is_anchor = true;
                a
            }
        })
        .collect()
}

/// Arena schedule: every game rotates the model list one seat, so each model
/// visits each seat — and therefore each role and each side — evenly over
/// `games`.
pub fn arena_schedule(match_seed: u64, models: &[AgentSpec], games: u32) -> Vec<GamePlan> {
    if models.is_empty() {
        return Vec::new();
    }
    (0..games)
        .map(|g| {
            let seed = cell_seed(match_seed, "codenames-arena", g as u64, 0, 0);
            let seats: Vec<AgentSpec> = (0..SEAT_COUNT as usize)
                .map(|i| {
                    let mut spec = models[(g as usize + i) % models.len()].clone();
                    spec.is_anchor = false;
                    spec
                })
                .collect();
            GamePlan {
                game_id: format!("{seed:016x}"),
                label: format!("arena/g{g}"),
                seed,
                seats,
            }
        })
        .collect()
}

/// **Planned** assignment distribution from the schedule (intent, before any
/// game is played): model → seat / role / side occurrence counts. Compared
/// against the store's realized distribution at finalize time, so a partial or
/// interrupted run cannot over-report its coverage.
pub fn planned_distribution(plans: &[GamePlan]) -> BTreeMap<String, BTreeMap<String, u32>> {
    let mut dist: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for plan in plans {
        for (seat, spec) in plan.seats.iter().enumerate() {
            let seat = seat as Seat;
            let by = dist.entry(spec.model_id()).or_default();
            *by.entry(format!("seat{seat}")).or_default() += 1;
            *by.entry(format!("role:{}", seat_role(seat).as_str()))
                .or_default() += 1;
            *by.entry(format!("side:{}", side_label(seat))).or_default() += 1;
        }
    }
    dist
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::agents::CodenamesAgentKind;
    use crate::codenames::types::Role;

    fn pool() -> Vec<AgentSpec> {
        vec![
            AgentSpec::bot(CodenamesAgentKind::Random),
            AgentSpec::llm("anchor-x"),
            AgentSpec::llm("anchor-y"),
        ]
    }

    #[test]
    fn controlled_cells_mirror_seeds_across_candidates() {
        let a = controlled_schedule(9, &AgentSpec::llm("model-a"), &pool(), 2, 3).unwrap();
        let b = controlled_schedule(9, &AgentSpec::llm("model-b"), &pool(), 2, 3).unwrap();
        assert_eq!(a.len(), 2 * 4 * 3);
        for (pa, pb) in a.iter().zip(&b) {
            assert_eq!(pa.seed, pb.seed, "seeds must be candidate-independent");
            assert_eq!(pa.label, pb.label);
        }
        // Every cell is a distinct game with a distinct seed.
        let mut ids: Vec<String> = a.iter().map(|p| p.game_id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), a.len());
    }

    #[test]
    fn the_candidate_visits_every_side_and_role_equally() {
        let plans = controlled_schedule(4, &AgentSpec::llm("cand"), &pool(), 3, 2).unwrap();
        let mut cells: BTreeMap<(&str, &str), u32> = BTreeMap::new();
        for plan in &plans {
            let seat = plan
                .seats
                .iter()
                .position(|s| !s.is_anchor)
                .expect("one candidate seat") as Seat;
            *cells
                .entry((side_label(seat), seat_role(seat).as_str()))
                .or_default() += 1;
        }
        assert_eq!(cells.len(), 4, "2 sides × 2 roles");
        assert!(
            cells.values().all(|&c| c == 3 * 2),
            "every cell exactly boards × K times: {cells:?}"
        );
    }

    /// The partner confound is held constant: the same anchor always sits
    /// beside the candidate, whatever side and role the candidate plays.
    #[test]
    fn the_anchor_teammate_is_constant() {
        let plans = controlled_schedule(1, &AgentSpec::llm("cand"), &pool(), 1, 1).unwrap();
        for plan in &plans {
            let seat = plan.seats.iter().position(|s| !s.is_anchor).unwrap() as Seat;
            let mate = if seat_role(seat) == Role::Spymaster {
                seat_team(seat).operative()
            } else {
                seat_team(seat).spymaster()
            };
            assert_eq!(plan.seats[mate as usize].name, pool()[0].name);
            assert!(plan.seats[mate as usize].is_anchor);
        }
    }

    #[test]
    fn arena_rotates_models_through_seats() {
        let models: Vec<AgentSpec> = ["m0", "m1", "m2", "m3"].map(AgentSpec::llm).into();
        let plans = arena_schedule(3, &models, 8);
        assert_eq!(plans.len(), 8);
        assert_eq!(plans[0].seats[0].name, "m0");
        assert_eq!(plans[1].seats[0].name, "m1");
        assert_eq!(plans[1].seats[3].name, "m0");
        // Over a full rotation every model has held every seat exactly twice.
        let planned = planned_distribution(&plans);
        for model in ["m0", "m1", "m2", "m3"] {
            let by = &planned[model];
            for seat in 0..SEAT_COUNT {
                assert_eq!(by[&format!("seat{seat}")], 2);
            }
            assert_eq!(by["role:spymaster"], 4);
            assert_eq!(by["side:starting"], 4);
        }
    }

    #[test]
    fn wrong_pool_size_is_rejected() {
        let mut p = pool();
        p.pop();
        assert!(controlled_schedule(1, &AgentSpec::llm("m"), &p, 1, 1).is_err());
        assert!(controlled_schedule(1, &AgentSpec::llm("m"), &pool(), 0, 1).is_err());
    }
}
