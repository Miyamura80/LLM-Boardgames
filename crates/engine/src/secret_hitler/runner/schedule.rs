//! Match schedules.
//!
//! **Controlled** (primary): one candidate + six frozen anchors, enumerating
//! candidate × 3 roles × 7 seats × K repetitions. The seed of each game is a
//! pure function of `(match_seed, role, seat, rep)` — independent of the
//! candidate — so different candidates run through a cell see the *same* deck
//! order and the *same* anchor arrangement (mirrored seeds).
//!
//! **Arena** (validation): mixed candidates rotated through seats; roles are
//! dealt from the seed. Duplicates are allowed (rating uses delta-averaging).

use super::pools::AgentSpec;
use crate::secret_hitler::types::{Role, PLAYER_COUNT};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A fully determined single game to play.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GamePlan {
    pub game_id: String,
    /// Provenance: `controlled/<role>/s<seat>/r<rep>` or `arena/g<idx>`.
    pub label: String,
    pub seed: u64,
    /// Forced role arrangement (controlled mode); `None` deals from the seed.
    pub roles: Option<Vec<Role>>,
    /// Seat agents, index = seat.
    pub seats: Vec<AgentSpec>,
}

use crate::game_core::cell_seed;

/// The three role buckets a candidate is rotated through.
pub const ROLE_BUCKETS: [Role; 3] = [Role::Liberal, Role::Fascist, Role::Hitler];

/// Controlled schedule: 21·K games for one candidate against a 6-anchor pool.
pub fn controlled_schedule(
    match_seed: u64,
    candidate: &AgentSpec,
    pool: &[AgentSpec],
    k: u32,
) -> Result<Vec<GamePlan>, String> {
    if pool.len() != 6 {
        return Err(format!(
            "a controlled pool must hold exactly 6 anchors, got {}",
            pool.len()
        ));
    }
    let mut plans = Vec::with_capacity(21 * k as usize);
    for (role_idx, role) in ROLE_BUCKETS.iter().enumerate() {
        for seat in 0..PLAYER_COUNT {
            for rep in 0..k {
                let seed = cell_seed(
                    match_seed,
                    "controlled",
                    role_idx as u64,
                    seat as u64,
                    rep as u64,
                );
                // Candidate-independent arrangement RNG.
                let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5EED_A11A);

                // Remaining six roles after the candidate takes `role`.
                let mut rest: Vec<Role> = full_role_set().into_iter().collect();
                let pos = rest.iter().position(|r| r == role).expect("role in set");
                rest.remove(pos);
                rest.shuffle(&mut rng);

                // Anchor arrangement over the six non-candidate seats.
                let mut anchors: Vec<AgentSpec> = pool.to_vec();
                anchors.shuffle(&mut rng);

                let mut roles = vec![Role::Liberal; PLAYER_COUNT as usize];
                let mut seats =
                    vec![AgentSpec::bot(app_config::AgentKind::Heuristic); PLAYER_COUNT as usize];
                let mut rest_iter = rest.into_iter();
                let mut anchor_iter = anchors.into_iter();
                for s in 0..PLAYER_COUNT {
                    if s == seat {
                        roles[s as usize] = *role;
                        let mut c = candidate.clone();
                        c.is_anchor = false;
                        seats[s as usize] = c;
                    } else {
                        roles[s as usize] = rest_iter.next().expect("six remaining roles");
                        let mut a = anchor_iter.next().expect("six anchors");
                        a.is_anchor = true;
                        seats[s as usize] = a;
                    }
                }

                let label = format!("controlled/{}/s{seat}/r{rep}", role.as_str().to_lowercase());
                plans.push(GamePlan {
                    game_id: format!("{seed:016x}"),
                    label,
                    seed,
                    roles: Some(roles),
                    seats,
                });
            }
        }
    }
    Ok(plans)
}

/// Arena schedule: every game seats 7 entries from `models`, rotated so each
/// model visits every seat as evenly as the count allows. Roles are dealt from
/// the seed (faction balance emerges over volume and is logged).
pub fn arena_schedule(match_seed: u64, models: &[AgentSpec], games: u32) -> Vec<GamePlan> {
    if models.is_empty() {
        return Vec::new();
    }
    let n = models.len();
    (0..games)
        .map(|g| {
            let seed = cell_seed(match_seed, "arena", g as u64, 0, 0);
            let seats: Vec<AgentSpec> = (0..PLAYER_COUNT)
                .map(|i| {
                    let mut spec = models[(g as usize + i as usize) % n].clone();
                    spec.is_anchor = false;
                    spec
                })
                .collect();
            GamePlan {
                game_id: format!("{seed:016x}"),
                label: format!("arena/g{g}"),
                seed,
                roles: None,
                seats,
            }
        })
        .collect()
}

fn full_role_set() -> [Role; PLAYER_COUNT as usize] {
    [
        Role::Liberal,
        Role::Liberal,
        Role::Liberal,
        Role::Liberal,
        Role::Fascist,
        Role::Fascist,
        Role::Hitler,
    ]
}

/// **Planned** assignment distribution from the schedule (intent, before any
/// game is played): model → seat counts and, when roles are forced up front
/// (controlled mode), role counts. Compare against the store's realized
/// distribution to spot coverage gaps in a partial run. Arena roles are dealt
/// at play time, so they are absent here and only appear in the realized view.
pub fn planned_distribution(plans: &[GamePlan]) -> BTreeMap<String, BTreeMap<String, u32>> {
    let mut dist: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for plan in plans {
        for (seat, spec) in plan.seats.iter().enumerate() {
            let by = dist.entry(spec.model_id()).or_default();
            *by.entry(format!("seat{seat}")).or_default() += 1;
            if let Some(roles) = &plan.roles {
                *by.entry(format!("role:{}", roles[seat].as_str()))
                    .or_default() += 1;
            }
        }
    }
    dist
}
