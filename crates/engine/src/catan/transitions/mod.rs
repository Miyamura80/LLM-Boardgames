//! `GameState::apply` — the single mutation entry point and sole legality
//! authority. Dispatches on (phase, action); every arm either mutates and
//! returns `Ok`, or returns an [`IllegalMove`] whose message is fed back to
//! the agent on the rethink re-prompt.

pub(crate) mod build;
mod dev;
mod roll;
mod setup;
mod trade;

use super::actions::{Action, DecisionPoint, IllegalMove};
use super::events::CatanEvent;
use super::state::{GameState, Phase};
use super::types::{Seat, PLAYER_COUNT};
use crate::game_core::Visibility;

impl GameState {
    pub fn apply(&mut self, seat: Seat, action: Action) -> Result<(), IllegalMove> {
        if self.is_over() {
            return Err(IllegalMove("the game is over".into()));
        }
        let phase_name = self.phase.name();
        if !self
            .pending_decisions()
            .iter()
            .any(|d: &DecisionPoint| d.seat() == seat)
        {
            return Err(IllegalMove(format!(
                "seat {seat} has no pending decision in phase {phase_name}"
            )));
        }

        let counts_toward_cap =
            matches!(self.phase, Phase::Turn) && !matches!(action, Action::EndTurn);

        match (self.phase.clone(), action) {
            (
                Phase::Setup {
                    placements,
                    road_for: None,
                },
                Action::PlaceSetupSettlement { vertex },
            ) => setup::place_settlement(self, seat, placements, vertex)?,
            (
                Phase::Setup {
                    placements,
                    road_for: Some(at),
                },
                Action::PlaceSetupRoad { edge },
            ) => setup::place_road(self, seat, placements, at, edge)?,
            (Phase::PreRoll, Action::Roll) => roll::roll_dice(self)?,
            (Phase::PreRoll, Action::PlayKnight) => dev::play_knight(self, seat)?,
            (Phase::Discard { pending }, Action::Discard { resources }) => {
                roll::discard(self, seat, &pending, resources)?
            }
            (Phase::MoveRobber, Action::MoveRobber { hex }) => roll::move_robber(self, seat, hex)?,
            (Phase::ChooseVictim { victims }, Action::StealFrom { seat: victim }) => {
                roll::choose_victim(self, seat, &victims, victim)?
            }
            (Phase::FreeRoad, Action::BuildRoad { edge }) => build::free_road(self, seat, edge)?,
            (Phase::Turn, a) => self.apply_turn_action(seat, a)?,
            (Phase::TradeResponse { .. }, a) => trade::respond(self, seat, a)?,
            (Phase::ResolveTrade { .. }, Action::ResolveTrade { partner }) => {
                trade::resolve(self, seat, partner)?
            }
            (_, a) => {
                return Err(IllegalMove(format!(
                    "{} is not a valid action in phase {phase_name}",
                    action_name(&a)
                )))
            }
        }

        // Per-turn action budget: past the cap the engine ends the turn itself
        // (logged as a forced default, metric-exempt).
        if counts_toward_cap {
            self.actions_this_turn += 1;
            if matches!(self.phase, Phase::Turn) && self.actions_this_turn >= self.caps.actions {
                self.push_forced_default(self.active, "turn-cap");
                end_turn(self);
            }
        }
        Ok(())
    }

    fn apply_turn_action(&mut self, seat: Seat, action: Action) -> Result<(), IllegalMove> {
        match action {
            Action::BuildRoad { edge } => build::build_road(self, seat, edge),
            Action::BuildSettlement { vertex } => build::build_settlement(self, seat, vertex),
            Action::BuildCity { vertex } => build::build_city(self, seat, vertex),
            Action::BuyDev => build::buy_dev(self, seat),
            Action::PlayKnight => dev::play_knight(self, seat),
            Action::PlayRoadBuilding => dev::play_road_building(self, seat),
            Action::PlayYearOfPlenty { first, second } => {
                dev::play_year_of_plenty(self, seat, first, second)
            }
            Action::PlayMonopoly { resource } => dev::play_monopoly(self, seat, resource),
            Action::BankTrade { give, receive } => trade::bank_trade(self, seat, give, receive),
            Action::ProposeTrade {
                give,
                receive,
                to,
                message,
            } => trade::propose(self, seat, give, receive, to, message),
            Action::Say { message } => {
                if self.says_this_turn >= self.caps.says {
                    return Err(IllegalMove(format!(
                        "say limit reached ({} per turn)",
                        self.caps.says
                    )));
                }
                self.says_this_turn += 1;
                self.push_event(
                    Visibility::Public,
                    CatanEvent::Said {
                        seat,
                        text: message,
                    },
                );
                Ok(())
            }
            Action::EndTurn => {
                end_turn(self);
                Ok(())
            }
            a => Err(IllegalMove(format!(
                "{} is not a valid action in the turn phase",
                action_name(&a)
            ))),
        }
    }
}

/// The action's JSON tag, for error messages that match what the agent wrote.
fn action_name(a: &Action) -> &'static str {
    match a {
        Action::PlaceSetupSettlement { .. } => "place_setup_settlement",
        Action::PlaceSetupRoad { .. } => "place_setup_road",
        Action::Roll => "roll",
        Action::PlayKnight => "play_knight",
        Action::Discard { .. } => "discard",
        Action::MoveRobber { .. } => "move_robber",
        Action::StealFrom { .. } => "steal_from",
        Action::BuildRoad { .. } => "build_road",
        Action::BuildSettlement { .. } => "build_settlement",
        Action::BuildCity { .. } => "build_city",
        Action::BuyDev => "buy_dev",
        Action::PlayRoadBuilding => "play_road_building",
        Action::PlayYearOfPlenty { .. } => "play_year_of_plenty",
        Action::PlayMonopoly { .. } => "play_monopoly",
        Action::BankTrade { .. } => "bank_trade",
        Action::ProposeTrade { .. } => "propose_trade",
        Action::AcceptTrade { .. } => "accept_trade",
        Action::RejectTrade { .. } => "reject_trade",
        Action::CounterTrade { .. } => "counter_trade",
        Action::ResolveTrade { .. } => "resolve_trade",
        Action::Say { .. } => "say",
        Action::EndTurn => "end_turn",
    }
}

/// Start `seat`'s turn: reset per-turn accounting and enforce the game-length
/// backstop (highest total VP wins a capped game; ties to the earlier seat).
pub(super) fn begin_turn(state: &mut GameState, seat: Seat) {
    state.turn += 1;
    if state.turn > state.caps.max_turns {
        let winner = (0..PLAYER_COUNT)
            .max_by_key(|&s| (state.victory_points(s, true), std::cmp::Reverse(s)))
            .expect("four seats");
        let vps: Vec<u8> = (0..PLAYER_COUNT)
            .map(|s| state.victory_points(s, true))
            .collect();
        state.winner = Some(winner);
        state.phase = Phase::GameOver;
        let turns = state.turn - 1;
        state.push_event(
            Visibility::Public,
            CatanEvent::GameEnded { winner, vps, turns },
        );
        return;
    }
    state.active = seat;
    state.actions_this_turn = 0;
    state.trade_windows_this_turn = 0;
    state.says_this_turn = 0;
    state.free_roads = 0;
    state.dev_played_this_turn = false;
    state.rolled = false;
    state.phase = Phase::PreRoll;
    state.push_event(
        Visibility::Public,
        CatanEvent::TurnStarted {
            seat,
            turn: state.turn,
        },
    );
}

/// End the active player's turn: their newly bought dev cards mature, play
/// passes left.
pub(super) fn end_turn(state: &mut GameState) {
    let active = state.active as usize;
    let mut matured = std::mem::take(&mut state.players[active].devs_new);
    state.players[active].devs_playable.append(&mut matured);
    begin_turn(state, (state.active + 1) % PLAYER_COUNT);
}

/// Resume after a robber interruption: back to the roll if it hasn't happened
/// yet (pre-roll Knight), else back to the action loop.
pub(super) fn resume_after_robber(state: &mut GameState) {
    state.phase = if state.rolled {
        Phase::Turn
    } else {
        Phase::PreRoll
    };
}
