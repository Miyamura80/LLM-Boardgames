//! Drives one full Catan game through the shared `game_core` rethink loop,
//! producing a complete [`GameRecord`].
//!
//! Decisions are resolved strictly sequentially — including the simultaneous
//! discards after a 7, which the engine accepts in any order and which reveal
//! only public counts to later discarders.

use super::record::{placements, GameRecord, SeatRecord, RULES_VERSION};
use crate::catan::agents::SeatAgent;
use crate::catan::state::{GameState, TurnCaps};
use crate::catan::types::{Seat, PLAYER_COUNT};
use crate::game_core::{DecisionOps, Reliability, ThoughtRecord};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct GameConfig {
    pub game_id: String,
    pub seed: u64,
    /// Attempts per decision before the forced legal default applies.
    pub retry_budget: u32,
    pub schedule_label: String,
    pub caps: TurnCaps,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            game_id: "game".into(),
            seed: 0,
            retry_budget: 3,
            schedule_label: "adhoc".into(),
            caps: TurnCaps::default(),
        }
    }
}

#[derive(Default)]
struct SeatTracker {
    reliability: Reliability,
    thoughts: Vec<ThoughtRecord>,
}

/// Run one game to completion. `agents[seat]` plays that seat;
/// `is_anchor[seat]` marks non-candidate seats for the rating layer.
pub async fn run_game(
    cfg: &GameConfig,
    agents: &mut [Box<dyn SeatAgent>],
    is_anchor: &[bool],
) -> GameRecord {
    assert_eq!(agents.len(), PLAYER_COUNT as usize);
    let started = Instant::now();
    let mut state = GameState::with_caps(cfg.seed, cfg.caps.clone());
    let mut trackers: Vec<SeatTracker> =
        (0..PLAYER_COUNT).map(|_| SeatTracker::default()).collect();
    let mut safety = 0u32;

    while !state.is_over() {
        safety += 1;
        assert!(safety < 500_000, "game loop failed to terminate");

        let decision = state.pending_decisions()[0].clone();
        let seat = DecisionOps::seat(&decision);
        let outcome = crate::game_core::resolve_decision(
            cfg.retry_budget,
            &mut state,
            &mut agents[seat as usize],
            &decision,
            &mut trackers[seat as usize].reliability,
        )
        .await;
        if let Some(text) = outcome.thought.filter(|t| !t.trim().is_empty()) {
            trackers[seat as usize].thoughts.push(ThoughtRecord {
                round: state.turn,
                at_event: state.events.len() as u32,
                decision: DecisionOps::kind(&decision).to_string(),
                text,
            });
        }
    }

    let winner = state.winner.expect("loop exits only on game over");
    let final_vps: Vec<u8> = (0..PLAYER_COUNT)
        .map(|s| state.victory_points(s, true))
        .collect();
    let ranks = placements(&final_vps);

    GameRecord {
        game_id: cfg.game_id.clone(),
        seed: cfg.seed,
        player_count: PLAYER_COUNT,
        rules_version: RULES_VERSION.into(),
        schedule_label: cfg.schedule_label.clone(),
        winner,
        final_vps: final_vps.clone(),
        turns: state.turn,
        seats: (0..PLAYER_COUNT as usize)
            .map(|i| {
                let t = std::mem::take(&mut trackers[i]);
                SeatRecord {
                    seat: i as Seat,
                    model_id: agents[i].model_id(),
                    agent_kind: agents[i].kind().to_string(),
                    scaffold_version: agents[i].scaffold_version(),
                    temperature: None,
                    is_anchor: is_anchor.get(i).copied().unwrap_or(false),
                    final_vp: final_vps[i],
                    public_vp: state.victory_points(i as Seat, false),
                    placement: ranks[i],
                    won: winner == i as Seat,
                    knights_played: state.players[i].knights_played,
                    reliability: t.reliability,
                    thoughts: t.thoughts,
                    usage: agents[i].usage(),
                }
            })
            .collect(),
        events: state.events.clone(),
        duration_ms: started.elapsed().as_millis() as u64,
    }
}
