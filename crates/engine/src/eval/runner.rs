//! The single-game runner: drives a [`GameState`] to a terminal state using a
//! set of [`Agent`]s, enforcing the parse → legal-check → rethink →
//! forced-default loop (FR-4), the simultaneous-reveal discussion (US-007), and
//! private belief elicitation (US-010). Produces a [`GameRecord`].

use crate::eval::agent::{Agent, AgentError};
use crate::eval::record::{BeliefSnapshot, GameRecord, Reliability, SeatAssignment, Usage};
use crate::game::{Action, Event, GameState};
use std::collections::BTreeMap;

pub struct RunnerConfig {
    /// Attempts per decision before the forced legal default is applied.
    pub max_attempts: usize,
    /// Simultaneous discussion rounds before each vote.
    pub discussion_rounds: u8,
    /// Whether to elicit private beliefs (skip for zero-token baseline runs).
    pub elicit_beliefs: bool,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            discussion_rounds: 3,
            elicit_beliefs: true,
        }
    }
}

/// Play one game to completion. `agents[seat]` occupies that seat.
pub async fn run_game(
    mut game: GameState,
    agents: &[Box<dyn Agent>],
    cfg: &RunnerConfig,
) -> GameRecord {
    assert_eq!(agents.len(), game.players().len(), "one agent per seat");

    let seats: Vec<SeatAssignment> = game
        .players()
        .iter()
        .map(|p| SeatAssignment {
            seat: p.seat,
            agent: agents[p.seat].name(),
            role: p.role,
        })
        .collect();

    let mut reliability: BTreeMap<usize, Reliability> = BTreeMap::new();
    let mut beliefs: Vec<BeliefSnapshot> = Vec::new();
    let mut checkpoint: u32 = 0;

    while !game.is_over() {
        let decision = game.current_decision().expect("decision while not over");
        let events = if decision.is_vote() {
            run_discussion(&mut game, agents, cfg.discussion_rounds).await;
            let ballot = collect_votes(&game, agents, &decision, cfg, &mut reliability).await;
            game.apply(ballot).expect("aggregated ballot is legal")
        } else {
            let seat = decision.actor().expect("non-vote decision has an actor");
            let action =
                resolve_action(&game, agents, seat, &decision, cfg, &mut reliability).await;
            game.apply(action).expect("resolved action is legal")
        };

        if cfg.elicit_beliefs && enacted_a_policy(&events) {
            elicit_beliefs(&game, agents, checkpoint, &mut beliefs).await;
            checkpoint += 1;
        }
    }

    // Final game-end belief snapshot.
    if cfg.elicit_beliefs {
        elicit_beliefs(&game, agents, checkpoint, &mut beliefs).await;
    }

    // Sum per-agent token usage.
    let mut usage = Usage::default();
    for a in agents {
        usage.add(&a.take_usage());
    }

    GameRecord {
        seed: game.config().seed,
        first_president: first_president_from_log(&game),
        seats,
        winner: game.winner().expect("game over has a winner"),
        win_reason: game.win_reason().expect("game over has a reason"),
        log: game.log().clone(),
        reliability,
        beliefs,
        usage,
    }
}

/// Resolve a single-seat decision through the rethink/forced-default loop.
async fn resolve_action(
    game: &GameState,
    agents: &[Box<dyn Agent>],
    seat: usize,
    decision: &crate::game::Decision,
    cfg: &RunnerConfig,
    reliability: &mut BTreeMap<usize, Reliability>,
) -> Action {
    let obs = game.observation(seat);
    let mut feedback: Option<String> = None;
    let counters = reliability.entry(seat).or_default();

    for _ in 0..cfg.max_attempts {
        match agents[seat].act(&obs, decision, feedback.as_deref()).await {
            Ok(action) => {
                // Legal-check by probing a clone; on illegal, rethink with reason.
                let mut probe = game.clone();
                match probe.apply(action.clone()) {
                    Ok(_) => return action,
                    Err(reason) => {
                        counters.illegal_moves += 1;
                        feedback = Some(format!("illegal move: {reason}. Choose a legal action."));
                    }
                }
            }
            Err(AgentError::Malformed(m)) => {
                counters.malformed_outputs += 1;
                feedback = Some(format!("malformed output: {m}. Return schema-valid JSON."));
            }
            Err(AgentError::Api(m)) => {
                counters.malformed_outputs += 1;
                feedback = Some(format!("provider error: {m}. Try again."));
            }
        }
    }

    counters.forced_defaults += 1;
    // Forced legal default is computed on a clone to keep `game` immutable here.
    let mut g = game.clone();
    g.forced_default()
        .expect("a pending decision has a default")
}

/// Collect a simultaneous ballot: each living voter reports only its own vote,
/// defaulting to Nein on exhaustion.
async fn collect_votes(
    game: &GameState,
    agents: &[Box<dyn Agent>],
    decision: &crate::game::Decision,
    cfg: &RunnerConfig,
    reliability: &mut BTreeMap<usize, Reliability>,
) -> Action {
    let voters = game.living_seats();
    let mut ballot: BTreeMap<usize, bool> = BTreeMap::new();

    // Sequential is fine for correctness and keeps reliability bookkeeping
    // simple; parallelism is a wall-clock optimization applied in discussion.
    for seat in voters {
        let obs = game.observation(seat);
        let mut vote = None;
        for _ in 0..cfg.max_attempts {
            match agents[seat].act(&obs, decision, None).await {
                Ok(Action::CastVotes(m)) => {
                    if let Some(&v) = m.get(&seat) {
                        vote = Some(v);
                        break;
                    }
                    reliability.entry(seat).or_default().malformed_outputs += 1;
                }
                Ok(_) => {
                    reliability.entry(seat).or_default().malformed_outputs += 1;
                }
                Err(_) => {
                    reliability.entry(seat).or_default().malformed_outputs += 1;
                }
            }
        }
        match vote {
            Some(v) => {
                ballot.insert(seat, v);
            }
            None => {
                reliability.entry(seat).or_default().forced_defaults += 1;
                ballot.insert(seat, false); // forced default: Nein
            }
        }
    }
    Action::CastVotes(ballot)
}

/// Run the simultaneous-reveal discussion: within a round, every living player
/// speaks conditioned only on state through the *previous* round; all utterances
/// are appended together after the round, so no seat sees another's same-round
/// message. No within-round ordering exists.
async fn run_discussion(game: &mut GameState, agents: &[Box<dyn Agent>], rounds: u8) {
    for round in 0..rounds {
        let living = game.living_seats();
        // Snapshot every seat's observation BEFORE any await → within-round
        // inputs are all conditioned on state through the previous round only.
        let futures = living.iter().map(|&seat| {
            let obs = game.observation(seat);
            async move { (seat, agents[seat].discuss(&obs, round).await) }
        });
        let utterances = futures_join_all(futures).await;
        // Reveal together: append after the whole round is collected.
        for (seat, maybe_text) in utterances {
            if let Some(text) = maybe_text {
                if !text.trim().is_empty() {
                    game.record_utterance(seat, round, text);
                }
            }
        }
    }
}

/// Elicit private beliefs from every living seat at a checkpoint.
async fn elicit_beliefs(
    game: &GameState,
    agents: &[Box<dyn Agent>],
    checkpoint: u32,
    out: &mut Vec<BeliefSnapshot>,
) {
    for seat in game.living_seats() {
        let obs = game.observation(seat);
        if let Some(beliefs) = agents[seat].beliefs(&obs).await {
            out.push(BeliefSnapshot {
                checkpoint,
                seat,
                beliefs,
            });
        }
    }
}

fn first_president_from_log(game: &GameState) -> usize {
    game.log()
        .public_events()
        .find_map(|e| match e {
            Event::PresidencyBegan { president, .. } => Some(*president),
            _ => None,
        })
        .unwrap_or(0)
}

fn enacted_a_policy(events: &[Event]) -> bool {
    events.iter().any(|e| {
        matches!(
            e,
            Event::PolicyEnacted { .. } | Event::ChaosPolicyEnacted { .. }
        )
    })
}

/// Minimal `join_all` without pulling in the `futures` crate: awaits each future
/// in turn. Discussion has no cross-future dependency, so ordering is irrelevant
/// and the within-round inputs were all snapshotted before any await.
async fn futures_join_all<F, T>(futures: impl Iterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T>,
{
    let mut out = Vec::new();
    for f in futures {
        out.push(f.await);
    }
    out
}
