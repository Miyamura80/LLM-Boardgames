//! Catan match schedules.
//!
//! **Controlled** (primary): one candidate + three frozen anchors, enumerating
//! `board × seat (4) × rep (K)` cells. All four seat-cells of a `(board, rep)`
//! pair share ONE game seed — identical board, dice stream, and dev deck — so
//! the candidate experiences the same luck from every seat, and the seed is a
//! pure function of `(match_seed, board, rep)`, independent of the candidate
//! (mirrored across candidates). The anchor arrangement rotates
//! candidate-independently per cell.
//!
//! **Arena** (validation): mixed candidates rotated through seats.

use super::pools::AgentSpec;
use crate::catan::types::PLAYER_COUNT;
use crate::game_core::cell_seed;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A fully determined single game to play.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GamePlan {
    pub game_id: String,
    /// Provenance: `controlled/b<board>/s<seat>/r<rep>` or `arena/g<idx>`.
    pub label: String,
    pub seed: u64,
    /// Seat agents, index = seat.
    pub seats: Vec<AgentSpec>,
}

/// Controlled schedule: `boards × 4 × k` games for one candidate against a
/// 3-anchor pool.
pub fn controlled_schedule(
    match_seed: u64,
    candidate: &AgentSpec,
    pool: &[AgentSpec],
    boards: u32,
    k: u32,
) -> Result<Vec<GamePlan>, String> {
    if pool.len() != (PLAYER_COUNT - 1) as usize {
        return Err(format!(
            "a controlled catan pool must hold exactly {} anchors, got {}",
            PLAYER_COUNT - 1,
            pool.len()
        ));
    }
    if boards == 0 || k == 0 {
        return Err("boards and k must both be at least 1".into());
    }
    let mut plans = Vec::with_capacity((boards * 4 * k) as usize);
    for board in 0..boards {
        for rep in 0..k {
            // One seed per (board, rep): same board/dice/deck for all four
            // candidate seats, and for every candidate (mirrored).
            let seed = cell_seed(match_seed, "catan-controlled", board as u64, rep as u64, 0);
            for seat in 0..PLAYER_COUNT {
                // Candidate-independent anchor arrangement for this cell.
                let arrange = cell_seed(
                    match_seed,
                    "catan-anchors",
                    board as u64,
                    rep as u64,
                    seat as u64,
                );
                let mut rng = ChaCha8Rng::seed_from_u64(arrange);
                let mut anchors: Vec<AgentSpec> = pool.to_vec();
                anchors.shuffle(&mut rng);

                let mut seats = Vec::with_capacity(PLAYER_COUNT as usize);
                let mut anchor_iter = anchors.into_iter();
                for s in 0..PLAYER_COUNT {
                    if s == seat {
                        let mut c = candidate.clone();
                        c.is_anchor = false;
                        seats.push(c);
                    } else {
                        let mut a = anchor_iter.next().expect("three anchors");
                        a.is_anchor = true;
                        seats.push(a);
                    }
                }

                plans.push(GamePlan {
                    game_id: format!("{seed:016x}-s{seat}"),
                    label: format!("controlled/b{board}/s{seat}/r{rep}"),
                    seed,
                    seats,
                });
            }
        }
    }
    Ok(plans)
}

/// Arena schedule: every game rotates the model list one seat, so each model
/// visits each seat evenly over `games`.
pub fn arena_schedule(match_seed: u64, models: &[AgentSpec], games: u32) -> Vec<GamePlan> {
    (0..games)
        .map(|g| {
            let seed = cell_seed(match_seed, "catan-arena", g as u64, 0, 0);
            let seats: Vec<AgentSpec> = (0..PLAYER_COUNT as usize)
                .map(|i| models[(g as usize + i) % models.len()].clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catan::runner::CatanAgentKind;

    fn pool() -> Vec<AgentSpec> {
        vec![
            AgentSpec::bot(CatanAgentKind::RandomLegal),
            AgentSpec::bot(CatanAgentKind::Greedy),
            AgentSpec::bot(CatanAgentKind::RandomLegal),
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
            // Anchor arrangement is also candidate-independent.
            let anchors = |p: &GamePlan| {
                p.seats
                    .iter()
                    .filter(|s| s.is_anchor)
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(anchors(pa), anchors(pb));
        }
        // All four seats of one (board, rep) share a seed; candidate rotates.
        assert_eq!(a[0].seed, a[1].seed);
        assert_eq!(a[0].seed, a[3].seed);
        assert!(a[0].seats[0].name.contains("model-a"));
        assert!(a[1].seats[1].name.contains("model-a"));
        // Distinct (board, rep) cells get distinct seeds and game ids.
        let mut ids: Vec<String> = a.iter().map(|p| p.game_id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), a.len());
    }

    #[test]
    fn arena_rotates_models_through_seats() {
        let models = vec![
            AgentSpec::llm("m0"),
            AgentSpec::llm("m1"),
            AgentSpec::llm("m2"),
            AgentSpec::llm("m3"),
        ];
        let plans = arena_schedule(3, &models, 8);
        assert_eq!(plans.len(), 8);
        assert_eq!(plans[0].seats[0].name, "m0");
        assert_eq!(plans[1].seats[0].name, "m1");
        assert_eq!(plans[1].seats[3].name, "m0");
    }

    #[test]
    fn wrong_pool_size_is_rejected() {
        let mut p = pool();
        p.pop();
        assert!(controlled_schedule(1, &AgentSpec::llm("m"), &p, 1, 1).is_err());
    }
}
