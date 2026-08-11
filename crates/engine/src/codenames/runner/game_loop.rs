//! Drives one full Codenames game through the shared `game_core` rethink loop,
//! producing a complete [`GameRecord`].
//!
//! Codenames is fully sequential — `pending_decisions()` always holds exactly
//! one entry — so this is the Catan single-decision loop with no simultaneous
//! phase to reconcile (PRD-codenames-evals §6).

use super::record::{GameRecord, SeatRecord, RULES_VERSION};
use crate::codenames::agents::SeatAgent;
use crate::codenames::state::GameState;
use crate::codenames::types::{seat_role, seat_team, GameConfig as RulesConfig, Seat, SEAT_COUNT};
use crate::codenames::wordlist::Wordlist;
use crate::game_core::{DecisionOps, Reliability, ThoughtRecord};
use std::time::Instant;

/// Everything one game needs beyond its four agents.
#[derive(Debug, Clone)]
pub struct GameConfig {
    pub game_id: String,
    pub seed: u64,
    /// Attempts per decision before the forced legal default applies.
    pub retry_budget: u32,
    pub schedule_label: String,
    /// Rules knobs (clue-word cap) — recorded per game via the observation.
    pub rules: RulesConfig,
    /// The pool the grid is drawn from; its hash lands in the record.
    pub wordlist: Wordlist,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            game_id: "game".into(),
            seed: 0,
            retry_budget: 3,
            schedule_label: "adhoc".into(),
            rules: RulesConfig::default(),
            wordlist: Wordlist::default_embedded(),
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
    assert_eq!(agents.len(), SEAT_COUNT as usize);
    let started = Instant::now();
    let mut state = GameState::with_wordlist(cfg.seed, cfg.wordlist.clone(), cfg.rules.clone());
    let mut trackers: Vec<SeatTracker> = (0..SEAT_COUNT).map(|_| SeatTracker::default()).collect();
    let mut safety = 0u32;

    while !state.is_over() {
        safety += 1;
        // Every turn reveals at least one card (the mandatory first guess), so
        // a live game cannot outlast the grid by more than its retries.
        assert!(safety < 10_000, "game loop failed to terminate");

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
    GameRecord {
        game_id: cfg.game_id.clone(),
        seed: cfg.seed,
        rules_version: RULES_VERSION.into(),
        wordlist_hash: state.wordlist_hash.clone(),
        schedule_label: cfg.schedule_label.clone(),
        winner,
        end_reason: state.end_reason.expect("a finished game carries a reason"),
        turns: state.turn,
        seats: (0..SEAT_COUNT as usize)
            .map(|i| {
                let t = std::mem::take(&mut trackers[i]);
                let seat = i as Seat;
                SeatRecord {
                    seat,
                    role: seat_role(seat),
                    team: seat_team(seat),
                    model_id: agents[i].model_id(),
                    agent_kind: agents[i].kind().to_string(),
                    scaffold_version: agents[i].scaffold_version(),
                    temperature: None,
                    is_anchor: is_anchor.get(i).copied().unwrap_or(false),
                    won: seat_team(seat) == winner,
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
