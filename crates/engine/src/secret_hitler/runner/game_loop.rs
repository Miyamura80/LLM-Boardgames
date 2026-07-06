//! Drives one full game: decision collection with the parse → legal-check →
//! rethink loop, simultaneous-reveal discussion, private belief elicitation,
//! and forced legal defaults — producing a complete [`GameRecord`].

use super::record::*;
use crate::secret_hitler::actions::{Action, DecisionPoint};
use crate::secret_hitler::agents::SeatAgent;
use crate::secret_hitler::agents::{AgentError, BeliefReport};
use crate::secret_hitler::events::{GameEvent, Visibility};
use crate::secret_hitler::state::GameState;
use crate::secret_hitler::types::{Power, Seat, PLAYER_COUNT};
use futures::future::join_all;
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct GameConfig {
    pub game_id: String,
    pub seed: u64,
    /// Simultaneous discussion rounds before each government's vote.
    pub discussion_rounds: u8,
    /// Attempts per decision before the forced legal default applies
    /// (each malformed parse, transport failure, or illegal move consumes one).
    pub retry_budget: u32,
    /// Elicit private beliefs after each enacted policy + at game end.
    pub belief_checkpoints: bool,
    pub schedule_label: String,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            game_id: "game".into(),
            seed: 0,
            discussion_rounds: 3,
            retry_budget: 3,
            belief_checkpoints: true,
            schedule_label: "adhoc".into(),
        }
    }
}

/// Per-seat mutable bookkeeping the loop accumulates.
#[derive(Default)]
struct SeatTracker {
    reliability: Reliability,
    policy_choices: Vec<PolicyChoice>,
    executions: Vec<ExecutionChoice>,
    thoughts: Vec<ThoughtRecord>,
}

/// Persist a reply's private reasoning (observability only — never enters any
/// observation).
fn record_thought(
    trackers: &mut [SeatTracker],
    state: &GameState,
    seat: Seat,
    decision: &DecisionPoint,
    thought: Option<String>,
) {
    if let Some(text) = thought.filter(|t| !t.trim().is_empty()) {
        trackers[seat as usize].thoughts.push(ThoughtRecord {
            round: state.round,
            at_event: state.events.len() as u32,
            decision: decision.kind().to_string(),
            text,
        });
    }
}

/// Run one game to completion. `agents[seat]` plays that seat;
/// `is_anchor[seat]` marks non-candidate seats for the rating layer.
pub async fn run_game(
    cfg: &GameConfig,
    agents: &mut [Box<dyn SeatAgent>],
    is_anchor: &[bool],
) -> GameRecord {
    run_game_from(cfg, GameState::new(cfg.seed), agents, is_anchor).await
}

/// Run a pre-built state (forced role arrangements from the controlled
/// scheduler) to completion.
pub async fn run_game_from(
    cfg: &GameConfig,
    state: GameState,
    agents: &mut [Box<dyn SeatAgent>],
    is_anchor: &[bool],
) -> GameRecord {
    assert_eq!(agents.len(), PLAYER_COUNT as usize);
    let started = Instant::now();
    let mut state = state;
    let mut trackers: Vec<SeatTracker> =
        (0..PLAYER_COUNT).map(|_| SeatTracker::default()).collect();
    let mut beliefs: Vec<BeliefSnapshot> = Vec::new();
    let mut enacted_at_last_checkpoint = 0u8;
    let mut safety = 0u32;

    while !state.is_over() {
        safety += 1;
        assert!(safety < 5_000, "game loop failed to terminate");

        let decisions = state.pending_decisions();
        // Only the Election phase yields simultaneous decisions; match on the
        // decision kind so this rule is stated, not assumed.
        if matches!(decisions[0], DecisionPoint::Vote { .. }) {
            // Simultaneous ballots: discussion first, then parallel votes.
            run_discussion(cfg, &state, agents, &mut trackers)
                .await
                .into_iter()
                .for_each(|(seat, round, text)| {
                    let pass = text.is_none();
                    state.add_utterance(seat, round, text.unwrap_or_default(), pass);
                });
            collect_votes(cfg, &mut state, agents, &mut trackers, &decisions).await;
        } else {
            let decision = decisions[0].clone();
            resolve_decision(cfg, &mut state, agents, &mut trackers, &decision).await;
        }

        // Belief checkpoint after each newly enacted policy.
        let enacted = state.liberal_policies + state.fascist_policies;
        if cfg.belief_checkpoints && enacted > enacted_at_last_checkpoint && !state.is_over() {
            enacted_at_last_checkpoint = enacted;
            beliefs.extend(elicit_beliefs(&state, agents, &mut trackers, enacted).await);
        }
    }

    // Final snapshot at game end (the terminal role reveal is hidden by
    // elicit_beliefs, or the snapshot would just read the answer key).
    if cfg.belief_checkpoints {
        beliefs.extend(elicit_beliefs(&state, agents, &mut trackers, u8::MAX).await);
    }

    let win = state.winner.expect("loop exits only on game over");
    let winner = win.winner();
    GameRecord {
        game_id: cfg.game_id.clone(),
        seed: cfg.seed,
        player_count: PLAYER_COUNT,
        rules_version: RULES_VERSION.into(),
        schedule_label: cfg.schedule_label.clone(),
        winner,
        win_condition: win,
        rounds: state.round,
        roles: state.players.iter().map(|p| p.role).collect(),
        seats: (0..PLAYER_COUNT as usize)
            .map(|i| {
                let t = std::mem::take(&mut trackers[i]);
                SeatRecord {
                    seat: i as Seat,
                    model_id: agents[i].model_id(),
                    agent_kind: agents[i].kind().to_string(),
                    scaffold_version: agents[i].scaffold_version(),
                    temperature: None,
                    role: state.players[i].role,
                    is_anchor: is_anchor.get(i).copied().unwrap_or(false),
                    survived: state.players[i].alive,
                    won: state.players[i].role.party() == winner,
                    reliability: t.reliability,
                    policy_choices: t.policy_choices,
                    executions: t.executions,
                    thoughts: t.thoughts,
                    usage: agents[i].usage(),
                }
            })
            .collect(),
        beliefs,
        events: state.events.clone(),
        duration_ms: started.elapsed().as_millis() as u64,
        discussion_rounds: cfg.discussion_rounds,
    }
}

/// One decision through the rethink loop; falls back to the forced legal
/// default when the budget is exhausted.
async fn resolve_decision(
    cfg: &GameConfig,
    state: &mut GameState,
    agents: &mut [Box<dyn SeatAgent>],
    trackers: &mut [SeatTracker],
    decision: &DecisionPoint,
) {
    let seat = decision.seat();
    let mut feedback: Option<String> = None;

    for _ in 0..cfg.retry_budget {
        let obs = state.observe(seat);
        let reply = agents[seat as usize]
            .decide(&obs, decision, feedback.as_deref())
            .await;
        match reply {
            Err(AgentError::Malformed(m)) => {
                trackers[seat as usize].reliability.malformed_outputs += 1;
                feedback = Some(m);
            }
            Err(AgentError::Transport(e)) => {
                trackers[seat as usize].reliability.transport_failures += 1;
                tracing::warn!(seat, error = %e, "agent transport failure");
            }
            Ok(reply) => match state.apply(seat, reply.action) {
                Ok(()) => {
                    record_thought(trackers, state, seat, decision, reply.thought);
                    record_choice(state, trackers, seat, decision, reply.action, false);
                    return;
                }
                Err(illegal) => {
                    trackers[seat as usize].reliability.illegal_moves += 1;
                    feedback = Some(illegal.0);
                }
            },
        }
    }

    // Budget exhausted: deterministic forced legal default, metric-exempt.
    let action = state.forced_default(decision);
    trackers[seat as usize].reliability.forced_defaults += 1;
    state.push_forced_default(seat, decision.kind());
    state
        .apply(seat, action)
        .expect("forced default must be legal");
    record_choice(state, trackers, seat, decision, action, true);
}

/// Capture metric inputs for policy and execution decisions.
fn record_choice(
    state: &GameState,
    trackers: &mut [SeatTracker],
    seat: Seat,
    decision: &DecisionPoint,
    action: Action,
    forced: bool,
) {
    let round = state.round;
    match (decision, action) {
        (DecisionPoint::Discard { tiles, .. }, Action::Discard { policy }) => {
            trackers[seat as usize].policy_choices.push(PolicyChoice {
                round,
                as_president: true,
                tiles_held: tiles.clone(),
                chosen: policy,
                forced,
            });
        }
        (DecisionPoint::Enact { tiles, .. }, Action::Enact { policy }) => {
            trackers[seat as usize].policy_choices.push(PolicyChoice {
                round,
                as_president: false,
                tiles_held: tiles.clone(),
                chosen: policy,
                forced,
            });
        }
        (
            DecisionPoint::UsePower {
                power: Power::Execution,
                ..
            },
            Action::UsePower { target },
        ) => {
            let role = state.players[target as usize].role;
            trackers[seat as usize].executions.push(ExecutionChoice {
                round,
                target,
                target_party: role.party(),
                target_was_hitler: role == crate::secret_hitler::types::Role::Hitler,
                forced,
            });
        }
        _ => {}
    }
}

/// Simultaneous ballots: gather every voter's decision in parallel against the
/// same pre-vote observation, then apply in canonical seat order. Stragglers
/// (parse/transport failures) go through the sequential rethink loop.
async fn collect_votes(
    cfg: &GameConfig,
    state: &mut GameState,
    agents: &mut [Box<dyn SeatAgent>],
    trackers: &mut [SeatTracker],
    decisions: &[DecisionPoint],
) {
    let by_seat: BTreeMap<Seat, &DecisionPoint> = decisions.iter().map(|d| (d.seat(), d)).collect();
    let observations: BTreeMap<Seat, _> = by_seat.keys().map(|&s| (s, state.observe(s))).collect();

    let futures = agents
        .iter_mut()
        .enumerate()
        .filter(|(i, _)| by_seat.contains_key(&(*i as Seat)))
        .map(|(i, agent)| {
            let seat = i as Seat;
            let obs = &observations[&seat];
            let decision = by_seat[&seat];
            async move { (seat, agent.decide(obs, decision, None).await) }
        });
    let replies: BTreeMap<Seat, _> = join_all(futures).await.into_iter().collect();

    for (&seat, decision) in &by_seat {
        match replies.get(&seat) {
            Some(Ok(reply)) if state.apply(seat, reply.action).is_ok() => {
                record_thought(trackers, state, seat, decision, reply.thought.clone());
            }
            _ => {
                // Count the failed parallel attempt, then rethink sequentially.
                match replies.get(&seat) {
                    Some(Err(AgentError::Malformed(_))) => {
                        trackers[seat as usize].reliability.malformed_outputs += 1
                    }
                    Some(Err(AgentError::Transport(_))) => {
                        trackers[seat as usize].reliability.transport_failures += 1
                    }
                    Some(Ok(_)) => trackers[seat as usize].reliability.illegal_moves += 1,
                    None => {}
                }
                resolve_decision(cfg, state, agents, trackers, decision).await;
            }
        }
    }
}

/// Orderless discussion: within a round every living speaker writes in
/// parallel, conditioned only on state through the previous round; the round's
/// utterances are then revealed together (appended in seat order).
async fn run_discussion(
    cfg: &GameConfig,
    state: &GameState,
    agents: &mut [Box<dyn SeatAgent>],
    trackers: &mut [SeatTracker],
) -> Vec<(Seat, u8, Option<String>)> {
    let speakers: Vec<Seat> = state
        .alive_seats()
        .into_iter()
        .filter(|&s| agents[s as usize].can_speak())
        .collect();
    if speakers.is_empty() || cfg.discussion_rounds == 0 {
        return Vec::new();
    }

    let mut all: Vec<(Seat, u8, Option<String>)> = Vec::new();
    let mut revealed: Vec<(Seat, u8, Option<String>)> = Vec::new();
    for round in 0..cfg.discussion_rounds {
        // Same-round isolation: every speaker sees the identical observation,
        // extended only by fully revealed previous rounds.
        let observations: BTreeMap<Seat, _> = speakers
            .iter()
            .map(|&s| (s, observe_with_utterances(state, s, &revealed)))
            .collect();
        let futures = agents
            .iter_mut()
            .enumerate()
            .filter(|(i, _)| speakers.contains(&(*i as Seat)))
            .map(|(i, agent)| {
                let seat = i as Seat;
                let obs = &observations[&seat];
                async move { (seat, agent.speak(obs, round).await) }
            });
        let mut replies: Vec<(Seat, Result<_, _>)> = join_all(futures).await;
        replies.sort_by_key(|(s, _)| *s); // canonical order, no seat priority
        for (seat, reply) in replies {
            let text = match reply {
                Ok(r) => r.text,
                Err(AgentError::Malformed(_)) => {
                    trackers[seat as usize].reliability.malformed_outputs += 1;
                    None
                }
                Err(AgentError::Transport(_)) => {
                    trackers[seat as usize].reliability.transport_failures += 1;
                    None
                }
            };
            revealed.push((seat, round, text.clone()));
            all.push((seat, round, text));
        }
    }
    all
}

/// A seat's observation extended with already-revealed discussion utterances
/// that are not yet in the engine transcript.
fn observe_with_utterances(
    state: &GameState,
    seat: Seat,
    revealed: &[(Seat, u8, Option<String>)],
) -> crate::secret_hitler::observation::Observation {
    let mut obs = state.observe(seat);
    for (s, round, text) in revealed {
        obs.history.push(crate::secret_hitler::events::EventRecord {
            idx: obs.history.len() as u32,
            round: state.round,
            visibility: Visibility::Public,
            event: GameEvent::Utterance {
                seat: *s,
                discussion_round: *round,
                text: text.clone().unwrap_or_default(),
                pass: text.is_none(),
            },
        });
    }
    obs
}

/// Private belief elicitation for every living seat, in parallel. Failures
/// are logged as transport noise, never as play-quality signal. The terminal
/// role reveal is stripped from the observation so the game-end snapshot
/// still measures inference, not the revealed answer key.
async fn elicit_beliefs(
    state: &GameState,
    agents: &mut [Box<dyn SeatAgent>],
    trackers: &mut [SeatTracker],
    checkpoint: u8,
) -> Vec<BeliefSnapshot> {
    let alive = state.alive_seats();
    let observations: BTreeMap<Seat, _> = alive
        .iter()
        .map(|&s| {
            let mut obs = state.observe(s);
            obs.history
                .retain(|r| !matches!(r.event, GameEvent::GameEnded { .. }));
            (s, obs)
        })
        .collect();
    let futures = agents
        .iter_mut()
        .enumerate()
        .filter(|(i, _)| alive.contains(&(*i as Seat)))
        .map(|(i, agent)| {
            let seat = i as Seat;
            let obs = &observations[&seat];
            async move { (seat, agent.beliefs(obs).await) }
        });
    let results: Vec<(Seat, Result<Option<BeliefReport>, AgentError>)> = join_all(futures).await;

    let mut out = Vec::new();
    for (seat, res) in results {
        match res {
            Ok(Some(report)) => out.push(BeliefSnapshot {
                checkpoint,
                seat,
                report,
            }),
            Ok(None) => {}
            Err(e) => {
                trackers[seat as usize].reliability.transport_failures += 1;
                tracing::warn!(seat, error = %e, "belief elicitation failed");
            }
        }
    }
    out
}
