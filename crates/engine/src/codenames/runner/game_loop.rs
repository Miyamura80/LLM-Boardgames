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
use sha2::{Digest, Sha256};
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

impl GameConfig {
    /// A fingerprint of everything about this config that makes two games
    /// incomparable: the pool the grids are drawn from, the clue-word cap, and
    /// the retry budget that decides when a forced default lands.
    ///
    /// A run stores this at init and refuses to resume under a different one —
    /// live config is re-read on every call, so without the check a resume
    /// after an edit would quietly mix games played under two rule sets into
    /// one rating. Per-game fields (`game_id`, `seed`, `schedule_label`) are
    /// deliberately excluded: they differ by design within a run.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        h.update(b"codenames-effective-config-v1\n");
        h.update(self.wordlist.content_hash().as_bytes());
        h.update(b"\n");
        h.update(self.rules.clue_word_max_len.to_le_bytes());
        h.update(self.retry_budget.to_le_bytes());
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
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
        // The round the decision belongs to, captured *before* it resolves: a
        // turn-ending guess runs `end_turn`, which increments `state.turn`, so
        // reading the counter afterwards would file the thought under the next
        // round and desynchronize it from its own transcript events.
        let round = state.turn;
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
                round,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::actions::{Action, DecisionPoint};
    use crate::codenames::agents::{AgentError, AgentReply, SeatAgent};
    use crate::codenames::observation::Observation;
    use crate::codenames::testkit;
    use async_trait::async_trait;

    /// Plays a legal move at every decision and says which turn it believed it
    /// was acting in, so the loop's own round stamp can be checked against the
    /// agent's view of the same moment.
    struct ChattyBot;

    #[async_trait]
    impl SeatAgent for ChattyBot {
        fn kind(&self) -> &'static str {
            "bot:test-chatty"
        }
        fn model_id(&self) -> String {
            "bot:test-chatty".into()
        }
        fn scaffold_version(&self) -> String {
            "test-chatty-v1".into()
        }
        async fn decide(
            &mut self,
            obs: &Observation,
            decision: &DecisionPoint,
            _feedback: Option<&str>,
        ) -> Result<AgentReply, AgentError> {
            let action = match decision {
                // `qqqqq` shares no substring with the scripted pool, so it is
                // a legal clue on every board this test can draw.
                DecisionPoint::GiveClue { .. } => Action::GiveClue {
                    word: "qqqqq".into(),
                    number: 1,
                },
                DecisionPoint::GuessOrPass { .. } => Action::Guess {
                    word: obs
                        .grid
                        .iter()
                        .find(|c| !c.revealed)
                        .expect("a live game leaves a face-down card")
                        .word
                        .clone(),
                },
            };
            Ok(AgentReply {
                action,
                thought: Some(format!("acting in turn {}", obs.turn)),
            })
        }
    }

    /// A thought belongs to the round its decision was made in. The bot never
    /// passes, so turns end on a guess — the case where the engine has already
    /// advanced `state.turn` by the time the loop records the thought.
    #[tokio::test]
    async fn thoughts_are_stamped_with_the_round_their_decision_belonged_to() {
        let cfg = GameConfig {
            wordlist: testkit::test_wordlist(),
            ..GameConfig::default()
        };
        let mut agents: Vec<Box<dyn SeatAgent>> = (0..SEAT_COUNT)
            .map(|_| Box::new(ChattyBot) as Box<dyn SeatAgent>)
            .collect();
        let record = run_game(&cfg, &mut agents, &[false; SEAT_COUNT as usize]).await;

        let (mut seen, mut turn_ending) = (0usize, 0usize);
        for seat in &record.seats {
            for thought in &seat.thoughts {
                assert_eq!(
                    thought.text,
                    format!("acting in turn {}", thought.round),
                    "seat {} filed a thought under round {}",
                    seat.seat,
                    thought.round
                );
                // A decision pushes at most two events (its own, plus the
                // `TurnStarted` of the next turn when it ends one), and its own
                // event carries the round the thought claims.
                let at = thought.at_event as usize;
                assert!(
                    record.events[..at]
                        .iter()
                        .rev()
                        .take(2)
                        .any(|e| e.round == thought.round),
                    "no event of this decision carries round {}",
                    thought.round
                );
                if record.events[at - 1].round != thought.round {
                    turn_ending += 1;
                }
                seen += 1;
            }
        }
        assert!(seen >= record.turns as usize, "every decision was recorded");
        assert!(
            turn_ending > 0,
            "the regression case — a guess that ended the turn before the thought was filed — \
             must actually occur in this game"
        );
    }
}
