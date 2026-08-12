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

/// Label prefix marking a plan as part of the arena rotation. It is the only
/// thing that distinguishes the two schedules once they are flattened into
/// plans, so [`rotation_imbalance_note`] keys off it.
pub const ARENA_LABEL_PREFIX: &str = "arena/";

/// Hard ceiling on the games one match may schedule. A schedule is fully
/// materialized in memory before the run row exists, so `boards × 4 × k` (or
/// `games`) near `u32::MAX` would overflow the capacity arithmetic and exhaust
/// memory before anything is persisted. Ten thousand games is orders of
/// magnitude above any real eval batch.
pub const MAX_SCHEDULED_GAMES: u32 = 10_000;

/// The total games a schedule would materialize, rejected when it overflows or
/// exceeds [`MAX_SCHEDULED_GAMES`].
fn checked_total(total: Option<u32>, what: &str) -> Result<u32, String> {
    match total {
        Some(n) if n <= MAX_SCHEDULED_GAMES => Ok(n),
        Some(n) => Err(format!(
            "{what} schedules {n} games; the limit is {MAX_SCHEDULED_GAMES}"
        )),
        None => Err(format!(
            "{what} schedules more than {} games; the limit is {MAX_SCHEDULED_GAMES}",
            u32::MAX
        )),
    }
}

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
    let total = checked_total(
        boards
            .checked_mul(SEAT_COUNT as u32)
            .and_then(|n| n.checked_mul(k)),
        "a controlled codenames match",
    )?;

    let mut plans = Vec::with_capacity(total as usize);
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
///
/// The rotation only balances if there is exactly one model per seat, and an
/// empty schedule must not be able to finalize as a complete run, so both are
/// rejected here rather than silently producing a confounded (or vacuous)
/// rating. A `games` count that is not a whole number of rotations is allowed —
/// it is a real, if lopsided, batch — and the imbalance it leaves is reported
/// by [`rotation_imbalance_note`] beside the realized distribution.
pub fn arena_schedule(
    match_seed: u64,
    models: &[AgentSpec],
    games: u32,
) -> Result<Vec<GamePlan>, String> {
    if models.len() != SEAT_COUNT as usize {
        return Err(format!(
            "an arena codenames model set must hold exactly {SEAT_COUNT} models (one per seat), \
             got {}",
            models.len()
        ));
    }
    if games == 0 {
        return Err("an arena codenames match must schedule at least 1 game".into());
    }
    checked_total(Some(games), "an arena codenames match")?;
    Ok((0..games)
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
                label: format!("{ARENA_LABEL_PREFIX}g{g}"),
                seed,
                seats,
            }
        })
        .collect())
}

/// The coverage a partial rotation leaves behind, as a human-facing note beside
/// the realized distribution: with `games` not a multiple of the four-seat
/// rotation, some models hold a seat (and therefore a role and a side) more
/// often than others, so their ratings are not equally confounded. `None` when
/// every model held every seat the same number of times.
///
/// **Arena plans only.** Controlled seating is nonuniform *by design* — the
/// candidate rotates through all four seats while `pool[0]` holds the two
/// teammate seats and `pool[1..]` the opposing pair — so a per-model seat count
/// comparison there always reports an imbalance that is the experiment, not a
/// defect. A non-arena (or empty) schedule therefore gets `None`.
pub fn rotation_imbalance_note(plans: &[GamePlan]) -> Option<String> {
    if plans.is_empty()
        || !plans
            .iter()
            .all(|p| p.label.starts_with(ARENA_LABEL_PREFIX))
    {
        return None;
    }
    let dist = planned_distribution(plans);
    let counts = |key: &str| -> (u32, u32) {
        dist.values()
            .map(|by| by.get(key).copied().unwrap_or(0))
            .fold((u32::MAX, 0), |(lo, hi), n| (lo.min(n), hi.max(n)))
    };
    let uneven: Vec<String> = (0..SEAT_COUNT)
        .map(|s| format!("seat{s}"))
        .chain(
            [
                "role:spymaster",
                "role:operative",
                "side:starting",
                "side:second",
            ]
            .map(String::from),
        )
        .filter(|key| {
            let (lo, hi) = counts(key);
            lo != hi
        })
        .collect();
    if uneven.is_empty() {
        return None;
    }
    Some(format!(
        "{} games do not divide evenly into the {SEAT_COUNT}-seat rotation: coverage is uneven \
         across {} — read side- and role-conditioned rows with that in mind",
        plans.len(),
        uneven.join(", ")
    ))
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
        let plans = arena_schedule(3, &models, 8).expect("four models, eight games");
        assert_eq!(plans.len(), 8);
        assert!(
            rotation_imbalance_note(&plans).is_none(),
            "two whole rotations are balanced"
        );
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

    /// A schedule is materialized in full before the run row exists, so an
    /// absurd size is rejected by arithmetic rather than by the allocator.
    #[test]
    fn oversized_schedules_are_rejected_before_they_are_materialized() {
        let cand = AgentSpec::llm("m");
        for (boards, k) in [(u32::MAX, 1), (1, u32::MAX), (100_000, 3), (2501, 1)] {
            let err = controlled_schedule(1, &cand, &pool(), boards, k)
                .expect_err("boards × 4 × k exceeds the cap");
            assert!(err.contains("the limit is"), "{err}");
        }
        // Exactly at the cap still builds.
        assert_eq!(
            controlled_schedule(1, &cand, &pool(), MAX_SCHEDULED_GAMES / 4, 1)
                .expect("the cap itself is allowed")
                .len(),
            MAX_SCHEDULED_GAMES as usize
        );

        let models: Vec<AgentSpec> = ["m0", "m1", "m2", "m3"].map(AgentSpec::llm).into();
        let err = arena_schedule(1, &models, u32::MAX).expect_err("over the cap");
        assert!(err.contains("the limit is"), "{err}");
    }

    /// An arena run must have one model per seat and at least one game: an
    /// empty schedule would otherwise finalize as a "complete" run with no
    /// games, and a set of any other size confounds model with seat.
    #[test]
    fn arena_needs_four_models_and_a_non_empty_schedule() {
        let four: Vec<AgentSpec> = ["m0", "m1", "m2", "m3"].map(AgentSpec::llm).into();
        assert!(arena_schedule(1, &four[..3], 4)
            .expect_err("three models")
            .contains("exactly 4 models"));
        assert!(arena_schedule(1, &[], 4)
            .expect_err("no models")
            .contains("exactly 4 models"));
        assert!(arena_schedule(1, &four, 0)
            .expect_err("zero games")
            .contains("at least 1 game"));
    }

    /// A partial rotation is allowed, but the coverage gap it leaves is stated
    /// rather than implied.
    #[test]
    fn a_partial_rotation_is_allowed_but_reported() {
        let models: Vec<AgentSpec> = ["m0", "m1", "m2", "m3"].map(AgentSpec::llm).into();
        let plans = arena_schedule(3, &models, 6).expect("six games");
        let note = rotation_imbalance_note(&plans).expect("6 is not a multiple of 4");
        assert!(note.contains("6 games"), "{note}");
        assert!(note.contains("seat0"), "{note}");
    }

    /// Controlled seating is nonuniform on purpose (the candidate rotates, the
    /// anchors sit still), so the rotation note — which only means anything for
    /// the arena — must stay silent about it whatever the shape of the run.
    #[test]
    fn a_controlled_schedule_never_reports_rotation_imbalance() {
        for (boards, k) in [(1, 1), (3, 2), (5, 1), (2, 3)] {
            let plans =
                controlled_schedule(7, &AgentSpec::llm("cand"), &pool(), boards, k).unwrap();
            // The counts really are lopsided — this is what used to warn.
            let dist = planned_distribution(&plans);
            let seat0: Vec<u32> = dist
                .values()
                .map(|by| by.get("seat0").copied().unwrap_or(0))
                .collect();
            assert_ne!(
                seat0.iter().min(),
                seat0.iter().max(),
                "controlled seating is deliberately nonuniform: {dist:?}"
            );
            assert!(
                rotation_imbalance_note(&plans).is_none(),
                "controlled/b{boards}…/r{k} must not report an arena rotation gap"
            );
        }
        assert!(rotation_imbalance_note(&[]).is_none(), "empty schedule");
    }
}
