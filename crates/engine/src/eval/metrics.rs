//! Objective, engine-derived per-player metrics (US-010). No LLM judge: every
//! number comes from engine state + declared private beliefs. Forced-default
//! actions are excluded from play-quality metrics (FR-4).
//!
//! - **suspicion_brier** — mean Brier score of a seat's private P(fascist)
//!   beliefs vs ground truth (lower is better).
//! - **goal_aligned_enactment** — of the policy choices a seat *personally*
//!   made (President discard, Chancellor enact), the fraction that advanced its
//!   own faction *given the tiles it held* (luck-controlled).
//! - **faction_throughput** — of governments where the seat made a policy
//!   choice, the fraction that enacted its own faction's policy (outcome stat).
//! - **execution_accuracy** — of a seat's executions, the fraction that hit the
//!   opposing faction.

use crate::eval::record::GameRecord;
use crate::game::{Event, Faction, Policy, Role};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Raw per-seat tallies for one game (ratios derived on aggregation).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerMetrics {
    pub seat: usize,
    pub model: String,
    pub role: Role,
    // Goal-aligned enactment (per-decision).
    pub enactment_choices: u32,
    pub enactment_aligned: u32,
    // Faction throughput (outcome).
    pub throughput_govs: u32,
    pub throughput_faction: u32,
    // Execution accuracy.
    pub executions: u32,
    pub executions_hit: u32,
    // Suspicion (Brier).
    pub belief_points: u32,
    pub brier_sum: f64,
}

impl PlayerMetrics {
    pub fn goal_aligned_enactment(&self) -> Option<f64> {
        ratio(self.enactment_aligned, self.enactment_choices)
    }
    pub fn faction_throughput(&self) -> Option<f64> {
        ratio(self.throughput_faction, self.throughput_govs)
    }
    pub fn execution_accuracy(&self) -> Option<f64> {
        ratio(self.executions_hit, self.executions)
    }
    pub fn suspicion_brier(&self) -> Option<f64> {
        (self.belief_points > 0).then(|| self.brier_sum / self.belief_points as f64)
    }
}

fn ratio(num: u32, den: u32) -> Option<f64> {
    (den > 0).then(|| num as f64 / den as f64)
}

/// Compute per-seat metrics for a single game.
pub fn game_metrics(rec: &GameRecord) -> Vec<PlayerMetrics> {
    let mut m: Vec<PlayerMetrics> = (0..rec.seats.len())
        .map(|seat| PlayerMetrics {
            seat,
            model: rec.agent_of(seat).to_string(),
            role: rec.role_of(seat),
            ..Default::default()
        })
        .collect();

    // Government-scoped trackers, reset each presidency.
    let mut drawn: Option<(usize, Vec<Policy>)> = None; // (president, 3 tiles)
    let mut hand: Option<(usize, Vec<Policy>)> = None; // (chancellor, 2 tiles)
    let mut forced: HashSet<(usize, String)> = HashSet::new();

    for entry in &rec.log.entries {
        match &entry.event {
            Event::PresidencyBegan { .. } => {
                drawn = None;
                hand = None;
                forced.clear();
            }
            Event::ForcedDefault { seat, decision } => {
                forced.insert((*seat, decision.clone()));
            }
            Event::DrewPolicies { policies } => {
                if let Some(owner) = entry.private_to {
                    drawn = Some((owner, policies.clone()));
                }
            }
            Event::ReceivedPolicies { policies } => {
                if let Some(owner) = entry.private_to {
                    hand = Some((owner, policies.clone()));
                }
            }
            Event::PolicyEnacted {
                policy,
                president,
                chancellor,
                ..
            } => {
                score_enacted_government(
                    &mut m,
                    rec,
                    *policy,
                    *president,
                    *chancellor,
                    &drawn,
                    &hand,
                    &forced,
                );
                drawn = None;
                hand = None;
            }
            Event::PlayerExecuted { president, target } => {
                if !forced.contains(&(*president, "execute".to_string())) {
                    let shooter = rec.role_of(*president).faction();
                    let victim = rec.role_of(*target).faction();
                    m[*president].executions += 1;
                    if victim != shooter {
                        m[*president].executions_hit += 1;
                    }
                }
            }
            _ => {}
        }
    }

    score_beliefs(&mut m, rec);
    m
}

#[allow(clippy::too_many_arguments)]
fn score_enacted_government(
    m: &mut [PlayerMetrics],
    rec: &GameRecord,
    policy: Policy,
    president: usize,
    chancellor: usize,
    drawn: &Option<(usize, Vec<Policy>)>,
    hand: &Option<(usize, Vec<Policy>)>,
    forced: &HashSet<(usize, String)>,
) {
    let enacted_faction = policy_faction(policy);

    // President discard quality (given the 3 tiles drawn).
    if let Some((pres, tiles)) = drawn {
        if *pres == president && !forced.contains(&(president, "discard".to_string())) {
            let own = rec.role_of(president).faction();
            // Reconstruct the discard = drawn minus the 2-tile hand passed on.
            if let Some((_, passed)) = hand {
                if let Some(discarded) = multiset_diff(tiles, passed) {
                    let opposing_present = tiles.iter().any(|t| policy_faction(*t) != own);
                    let aligned = policy_faction(discarded) != own || !opposing_present;
                    m[president].enactment_choices += 1;
                    if aligned {
                        m[president].enactment_aligned += 1;
                    }
                }
            }
        }
    }

    // Chancellor enact quality (given the 2 tiles held).
    if let Some((chan, tiles)) = hand {
        if *chan == chancellor && !forced.contains(&(chancellor, "enact".to_string())) {
            let own = rec.role_of(chancellor).faction();
            let own_present = tiles.iter().any(|t| policy_faction(*t) == own);
            let aligned = enacted_faction == own || !own_present;
            m[chancellor].enactment_choices += 1;
            if aligned {
                m[chancellor].enactment_aligned += 1;
            }
        }
    }

    // Faction throughput: both President and Chancellor made a choice here.
    for seat in [president, chancellor] {
        let own = rec.role_of(seat).faction();
        m[seat].throughput_govs += 1;
        if enacted_faction == own {
            m[seat].throughput_faction += 1;
        }
    }
}

fn score_beliefs(m: &mut [PlayerMetrics], rec: &GameRecord) {
    for snap in &rec.beliefs {
        for (&target, &prob) in &snap.beliefs.fascist_prob {
            if target >= rec.seats.len() {
                continue;
            }
            let truth = if rec.role_of(target).faction() == Faction::Fascist {
                1.0
            } else {
                0.0
            };
            let brier = (prob - truth).powi(2);
            m[snap.seat].brier_sum += brier;
            m[snap.seat].belief_points += 1;
        }
    }
}

fn policy_faction(p: Policy) -> Faction {
    match p {
        Policy::Liberal => Faction::Liberal,
        Policy::Fascist => Faction::Fascist,
    }
}

/// The single tile in `whole` (3) not accounted for by `part` (2), as multisets.
fn multiset_diff(whole: &[Policy], part: &[Policy]) -> Option<Policy> {
    let count = |tiles: &[Policy], p: Policy| tiles.iter().filter(|&&t| t == p).count();
    [Policy::Liberal, Policy::Fascist]
        .into_iter()
        .find(|&p| count(whole, p) > count(part, p))
}

// ---- aggregation across many games ---------------------------------------

/// Aggregated metrics for one (model, role) cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRoleMetrics {
    pub model: String,
    pub role: Role,
    pub games: u32,
    pub goal_aligned_enactment: Option<f64>,
    pub faction_throughput: Option<f64>,
    pub execution_accuracy: Option<f64>,
    pub suspicion_brier: Option<f64>,
}

/// Aggregate per-seat metrics from many games into (model, role) cells.
pub fn aggregate(per_game: &[Vec<PlayerMetrics>]) -> Vec<ModelRoleMetrics> {
    let mut acc: HashMap<(String, Role), PlayerMetrics> = HashMap::new();
    let mut counts: HashMap<(String, Role), u32> = HashMap::new();

    for game in per_game {
        for pm in game {
            let key = (pm.model.clone(), pm.role);
            let e = acc.entry(key.clone()).or_insert_with(|| PlayerMetrics {
                model: pm.model.clone(),
                role: pm.role,
                ..Default::default()
            });
            e.enactment_choices += pm.enactment_choices;
            e.enactment_aligned += pm.enactment_aligned;
            e.throughput_govs += pm.throughput_govs;
            e.throughput_faction += pm.throughput_faction;
            e.executions += pm.executions;
            e.executions_hit += pm.executions_hit;
            e.belief_points += pm.belief_points;
            e.brier_sum += pm.brier_sum;
            *counts.entry(key).or_default() += 1;
        }
    }

    let mut out: Vec<ModelRoleMetrics> = acc
        .into_iter()
        .map(|((model, role), pm)| ModelRoleMetrics {
            model,
            role,
            games: counts.get(&(pm.model.clone(), role)).copied().unwrap_or(0),
            goal_aligned_enactment: pm.goal_aligned_enactment(),
            faction_throughput: pm.faction_throughput(),
            execution_accuracy: pm.execution_accuracy(),
            suspicion_brier: pm.suspicion_brier(),
        })
        .collect();
    out.sort_by(|a, b| {
        (a.model.clone(), role_order(a.role)).cmp(&(b.model.clone(), role_order(b.role)))
    });
    out
}

fn role_order(role: Role) -> u8 {
    match role {
        Role::Liberal => 0,
        Role::Fascist => 1,
        Role::Hitler => 2,
    }
}
