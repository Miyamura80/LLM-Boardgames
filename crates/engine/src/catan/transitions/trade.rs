//! Trading: bank/port exchanges and the player-trade window — free-form talk
//! in `message` fields, typed engine-validated commitment (PRD-catan-evals
//! US-C06). Only resource moves change hands; words never do.

use super::super::actions::{Action, IllegalMove};
use super::super::events::{CatanEvent, TradeOffer, TradeResponse};
use super::super::state::{GameState, Phase};
use super::super::types::{Resource, ResourceSet, Seat, PLAYER_COUNT, RESOURCES};
use crate::game_core::Visibility;

pub(super) fn bank_trade(
    state: &mut GameState,
    seat: Seat,
    give: Resource,
    receive: Resource,
) -> Result<(), IllegalMove> {
    if give == receive {
        return Err(IllegalMove("cannot trade a resource for itself".into()));
    }
    let rate = state.bank_rate(seat, give);
    if state.players[seat as usize].resources.get(give) < rate {
        return Err(IllegalMove(format!(
            "your best {} rate is {rate}:1 and you hold only {}",
            give.as_str(),
            state.players[seat as usize].resources.get(give)
        )));
    }
    if state.bank.get(receive) == 0 {
        return Err(IllegalMove(format!("the bank has no {}", receive.as_str())));
    }
    state.players[seat as usize].resources.remove(give, rate);
    state.bank.add(give, rate);
    state.bank.remove(receive, 1);
    state.players[seat as usize].resources.add(receive, 1);
    state.push_event(
        Visibility::Public,
        CatanEvent::BankTraded {
            seat,
            gave: ResourceSet::of(give, rate),
            got: ResourceSet::of(receive, 1),
            rate,
        },
    );
    Ok(())
}

/// Both sides non-empty and no resource on both sides.
fn check_offer_shape(give: &ResourceSet, receive: &ResourceSet) -> Result<(), IllegalMove> {
    if give.is_empty() || receive.is_empty() {
        return Err(IllegalMove(
            "a trade must move cards both ways (no gifts)".into(),
        ));
    }
    if RESOURCES
        .iter()
        .any(|&r| give.get(r) > 0 && receive.get(r) > 0)
    {
        return Err(IllegalMove(
            "a trade cannot give and receive the same resource".into(),
        ));
    }
    Ok(())
}

pub(super) fn propose(
    state: &mut GameState,
    seat: Seat,
    give: ResourceSet,
    receive: ResourceSet,
    to: Option<Seat>,
    message: Option<String>,
) -> Result<(), IllegalMove> {
    if state.trade_windows_this_turn >= state.caps.trade_windows {
        return Err(IllegalMove(format!(
            "trade-window limit reached ({} per turn)",
            state.caps.trade_windows
        )));
    }
    check_offer_shape(&give, &receive)?;
    if !state.players[seat as usize].resources.contains(&give) {
        return Err(IllegalMove(format!(
            "you offered {} but hold {}",
            give.describe(),
            state.players[seat as usize].resources.describe()
        )));
    }
    match to {
        Some(t) if t == seat => return Err(IllegalMove("cannot trade with yourself".into())),
        Some(t) if t >= PLAYER_COUNT => {
            return Err(IllegalMove(format!("seat {t} does not exist")))
        }
        _ => {}
    }

    // Responders in seat order starting left of the proposer.
    let to_respond: Vec<Seat> = (1..PLAYER_COUNT)
        .map(|i| (seat + i) % PLAYER_COUNT)
        .filter(|&s| to.is_none() || to == Some(s))
        .collect();

    state.trade_windows_this_turn += 1;
    let offer = TradeOffer { give, receive };
    state.push_event(
        Visibility::Public,
        CatanEvent::TradeProposed {
            seat,
            to,
            offer,
            message,
        },
    );
    state.phase = Phase::TradeResponse {
        offer,
        open: to.is_none(),
        to_respond,
        responses: Vec::new(),
    };
    Ok(())
}

pub(super) fn respond(
    state: &mut GameState,
    seat: Seat,
    action: Action,
) -> Result<(), IllegalMove> {
    let Phase::TradeResponse {
        offer,
        open,
        to_respond,
        responses,
    } = state.phase.clone()
    else {
        unreachable!("dispatch checked the phase");
    };

    let (response, message) = match action {
        Action::AcceptTrade { message } => {
            // The responder pays `offer.receive` and gains `offer.give`.
            if !state.players[seat as usize]
                .resources
                .contains(&offer.receive)
            {
                return Err(IllegalMove(format!(
                    "accepting means paying {}; you hold {}",
                    offer.receive.describe(),
                    state.players[seat as usize].resources.describe()
                )));
            }
            (TradeResponse::Accept, message)
        }
        Action::RejectTrade { message } => (TradeResponse::Reject, message),
        Action::CounterTrade {
            give,
            receive,
            message,
        } => {
            check_offer_shape(&give, &receive)?;
            if !state.players[seat as usize].resources.contains(&give) {
                return Err(IllegalMove(format!(
                    "your counter offers {} but you hold {}",
                    give.describe(),
                    state.players[seat as usize].resources.describe()
                )));
            }
            // Store in the proposer's perspective.
            let counter = TradeOffer {
                give: receive,
                receive: give,
            };
            (TradeResponse::Counter { offer: counter }, message)
        }
        a => {
            return Err(IllegalMove(format!(
                "only accept_trade / reject_trade / counter_trade answer an offer (got {})",
                super::action_name(&a)
            )))
        }
    };

    state.push_event(
        Visibility::Public,
        CatanEvent::TradeResponded {
            seat,
            response,
            message,
        },
    );

    let mut responses = responses;
    responses.push((seat, response));
    let to_respond: Vec<Seat> = to_respond.into_iter().filter(|&s| s != seat).collect();
    if !to_respond.is_empty() {
        state.phase = Phase::TradeResponse {
            offer,
            open,
            to_respond,
            responses,
        };
        return Ok(());
    }
    settle_window(state, offer, responses);
    Ok(())
}

/// All responses are in: auto-execute a unique clean accept, hand a choice to
/// the proposer when there is one, or close the window.
fn settle_window(state: &mut GameState, offer: TradeOffer, responses: Vec<(Seat, TradeResponse)>) {
    let accepters: Vec<Seat> = responses
        .iter()
        .filter(|(_, r)| matches!(r, TradeResponse::Accept))
        .map(|(s, _)| *s)
        .collect();
    let counters: Vec<(Seat, TradeOffer)> = responses
        .iter()
        .filter_map(|(s, r)| match r {
            TradeResponse::Counter { offer } => Some((*s, *offer)),
            _ => None,
        })
        .collect();

    if accepters.len() == 1 && counters.is_empty() {
        execute(state, accepters[0], offer);
        state.phase = Phase::Turn;
    } else if accepters.is_empty() && counters.is_empty() {
        state.push_event(
            Visibility::Public,
            CatanEvent::TradeWindowClosed { seat: state.active },
        );
        state.phase = Phase::Turn;
    } else {
        state.phase = Phase::ResolveTrade {
            offer,
            accepters,
            counters,
        };
    }
}

pub(super) fn resolve(
    state: &mut GameState,
    seat: Seat,
    partner: Option<Seat>,
) -> Result<(), IllegalMove> {
    let Phase::ResolveTrade {
        offer,
        accepters,
        counters,
    } = state.phase.clone()
    else {
        unreachable!("dispatch checked the phase");
    };

    match partner {
        None => {
            state.push_event(Visibility::Public, CatanEvent::TradeWindowClosed { seat });
            state.phase = Phase::Turn;
            Ok(())
        }
        Some(p) if accepters.contains(&p) => {
            execute(state, p, offer);
            state.phase = Phase::Turn;
            Ok(())
        }
        Some(p) => match counters.iter().find(|(s, _)| *s == p) {
            Some(&(_, counter)) => {
                // Taking a counter means paying its terms — validate now.
                if !state.players[seat as usize]
                    .resources
                    .contains(&counter.give)
                {
                    return Err(IllegalMove(format!(
                        "seat {p}'s counter needs you to pay {}; you hold {}",
                        counter.give.describe(),
                        state.players[seat as usize].resources.describe()
                    )));
                }
                execute(state, p, counter);
                state.phase = Phase::Turn;
                Ok(())
            }
            None => Err(IllegalMove(format!(
                "seat {p} neither accepted nor countered (accepted: {accepters:?})"
            ))),
        },
    }
}

/// Move the cards. Callers validated both hands; the debug asserts keep the
/// conservation invariant loud in tests.
fn execute(state: &mut GameState, partner: Seat, offer: TradeOffer) {
    let proposer = state.active;
    debug_assert!(state.players[proposer as usize]
        .resources
        .contains(&offer.give));
    debug_assert!(state.players[partner as usize]
        .resources
        .contains(&offer.receive));
    state.players[proposer as usize]
        .resources
        .remove_set(&offer.give);
    state.players[partner as usize]
        .resources
        .add_set(&offer.give);
    state.players[partner as usize]
        .resources
        .remove_set(&offer.receive);
    state.players[proposer as usize]
        .resources
        .add_set(&offer.receive);
    state.push_event(
        Visibility::Public,
        CatanEvent::TradeExecuted {
            proposer,
            with: partner,
            offer,
        },
    );
}
