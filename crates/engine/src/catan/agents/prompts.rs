//! Prompt construction for LLM seats: rules digest, compact board
//! serialization with stable ids, per-decision asks/schemas, and the scaffold
//! hash that pins rating attribution to exact prompt wording.

use crate::catan::actions::DecisionPoint;
use crate::catan::board::BoardLayout;
use crate::catan::events::TradeOffer;
use crate::catan::observation::Observation;
use crate::catan::state::GameState;
use crate::catan::types::{Seat, RESOURCES};
use sha2::{Digest, Sha256};

const PROMPT_REVISION: &str = "catan-prompts-v1";

pub const RULES_SUMMARY: &str = "\
You are playing The Settlers of Catan (4 players, base game). First to 10 victory points (VP) on YOUR OWN turn wins.
VP: settlement 1, city 2, Longest Road 2 (first continuous route of 5+, stealable), Largest Army 2 (first 3+ knights played, stealable), each Victory Point dev card 1 (hidden until you win).
Resources: brick, lumber, wool, grain, ore. Hexes produce to adjacent settlements (1) and cities (2) when their number is rolled, unless the robber sits there.
Costs — road: 1 brick + 1 lumber. Settlement: 1 brick + 1 lumber + 1 wool + 1 grain. City (upgrades a settlement): 2 grain + 3 ore. Dev card: 1 wool + 1 grain + 1 ore.
Placement — settlements must sit on a vacant vertex with ALL neighboring vertices vacant (distance rule) and (after setup) touch one of your roads. Roads must connect to your network; an opponent's building blocks connection through its vertex.
Turn — roll first (you may play one Knight before rolling), then any number of actions: build, buy/play one dev card per turn (not one bought this turn), trade with the bank (4:1, or 3:1/2:1 with a port you settled), trade with players, then end your turn.
A rolled 7: every player holding more than 7 cards discards half (rounded down); you move the robber and steal 1 random card from a player at its hex.
Dev cards — Knight: move the robber and steal (counts toward Largest Army). Road Building: place 2 free roads. Year of Plenty: take any 2 cards from the bank. Monopoly: name a resource, every opponent hands you all of theirs. Victory Point: secret, counts at win.
Player trades — you may propose trades on your turn (talk is allowed in the message field, but only typed offers move cards; promises about future turns are not enforced). Responders accept, reject, or counter; you pick who to close with. The engine validates every exchange.";

pub const OUTPUT_CONTRACT: &str = "\
Respond with a SINGLE strict JSON object and nothing else. No markdown fences, no prose outside the JSON.
The object must match the requested schema exactly — unknown fields are rejected.
You may include a \"thought_process\" string field with your private reasoning; it is never shown to other players.
Message fields are public table talk seen by everyone.";

/// Who you are this game (seat identity + private holdings live in the
/// observation body).
pub fn seat_brief(obs: &Observation) -> String {
    format!(
        "You are Player {} in a 4-player game. Play to win: maximize your own final position. \
Turn order is fixed: Player 0, 1, 2, 3, repeating.",
        obs.seat
    )
}

/// The full compact board + status rendering.
pub fn render_observation(obs: &Observation) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str("== BOARD ==\n");
    s.push_str("Hexes (id: terrain number [pips], R = robber):\n");
    for (h, t) in obs.layout.terrains.iter().enumerate() {
        let num = match obs.layout.numbers[h] {
            Some(n) => format!("{n} [{}]", BoardLayout::pips(n)),
            None => "desert".into(),
        };
        let robber = if obs.robber == h as u8 { " R" } else { "" };
        s.push_str(&format!("  h{h}: {} {num}{robber}\n", t.as_str()));
    }
    s.push_str("Hex corners (vertex ids clockwise from north — shared corners = shared ids):\n");
    let g = crate::catan::board::graph();
    for (h, vs) in g.hex_vertices.iter().enumerate() {
        s.push_str(&format!(
            "  h{h}: v{} v{} v{} v{} v{} v{}\n",
            vs[0], vs[1], vs[2], vs[3], vs[4], vs[5]
        ));
    }
    s.push_str("Ports (settle an endpoint vertex to use the rate):\n");
    for &(e, port) in &obs.layout.ports {
        let (a, b) = g.edge_ends(e);
        let kind = match port {
            crate::catan::board::Port::Generic => "3:1 any".into(),
            crate::catan::board::Port::Resource { resource } => {
                format!("2:1 {}", resource.as_str())
            }
        };
        s.push_str(&format!("  e{e} (v{a}-v{b}): {kind}\n"));
    }
    s.push_str("Buildings: ");
    if obs.occupancy.buildings.is_empty() {
        s.push_str("none");
    }
    for &(v, owner, is_city) in &obs.occupancy.buildings {
        let kind = if is_city { "city" } else { "settlement" };
        s.push_str(&format!("P{owner} {kind} v{v}; "));
    }
    s.push_str("\nRoads: ");
    if obs.occupancy.roads.is_empty() {
        s.push_str("none");
    }
    for &(e, owner) in &obs.occupancy.roads {
        let (a, b) = g.edge_ends(e);
        s.push_str(&format!("P{owner} e{e}(v{a}-v{b}); "));
    }
    s.push('\n');

    s.push_str("\n== STATUS ==\n");
    s.push_str(&format!(
        "Turn {} — Player {} is active ({} phase).\n",
        obs.turn, obs.active, obs.phase
    ));
    s.push_str(&format!(
        "Your hand: {}. Playable dev cards: {}. Bought this turn: {}.\n",
        obs.hand.describe(),
        list_devs(&obs.devs_playable),
        list_devs(&obs.devs_new),
    ));
    s.push_str(&format!("Your total VP (incl. hidden): {}.\n", obs.my_vp));
    for p in &obs.players {
        if p.seat == obs.seat {
            continue;
        }
        s.push_str(&format!(
            "P{}: {} VP shown, {} resource cards, {} dev cards, {} knights played.\n",
            p.seat, p.public_vp, p.hand_count, p.dev_count, p.knights_played
        ));
    }
    s.push_str(&format!(
        "Bank: {}. Dev deck: {} left. Longest Road: {}. Largest Army: {}.\n",
        obs.bank.describe(),
        obs.dev_deck_remaining,
        holder(obs.longest_road),
        holder(obs.largest_army),
    ));

    // Delta rendering: the last three turns of events carry the live context
    // (talk, trades, rolls); the board above already encodes older history.
    let cutoff = obs.turn.saturating_sub(2);
    s.push_str("\n== RECENT EVENTS ==\n");
    for record in obs.history.iter().filter(|r| r.round >= cutoff) {
        let tag = match record.visibility {
            crate::game_core::Visibility::Public => "",
            crate::game_core::Visibility::Private(_) => "[private] ",
        };
        let line = record.event.render();
        if line.starts_with("[private]") {
            s.push_str(&format!("{line}\n"));
        } else {
            s.push_str(&format!("{tag}{line}\n"));
        }
    }
    s
}

fn list_devs(devs: &[crate::catan::types::DevCard]) -> String {
    if devs.is_empty() {
        "none".into()
    } else {
        devs.iter()
            .map(|d| d.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn holder(entry: Option<(Seat, u8)>) -> String {
    match entry {
        Some((s, n)) => format!("P{s} ({n})"),
        None => "unclaimed".into(),
    }
}

fn ids<T: std::fmt::Display>(prefix: &str, xs: &[T]) -> String {
    xs.iter()
        .map(|x| format!("{prefix}{x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn offer_line(offer: &TradeOffer) -> String {
    format!(
        "the proposer gives {} and wants {} back",
        offer.give.describe(),
        offer.receive.describe()
    )
}

/// The natural-language "what to decide now" line. Model-visible: wording
/// changes must move the scaffold id (the golden render hashes this).
pub fn decision_ask(obs: &Observation, decision: &DecisionPoint) -> String {
    match decision {
        DecisionPoint::SetupSettlement { round, .. } => format!(
            "Setup round {}: place a settlement on any open vertex. Legal vertices: {}.",
            round + 1,
            ids("v", &obs.setup_settlement_sites())
        ),
        DecisionPoint::SetupRoad { at, .. } => format!(
            "Place the free road attached to your new settlement at v{at}. Legal edges: {}.",
            ids("e", &obs.setup_road_sites(*at))
        ),
        DecisionPoint::PreRoll { can_play_knight, .. } => {
            if *can_play_knight {
                "Start your turn: roll the dice, or play your Knight first.".into()
            } else {
                "Start your turn: roll the dice.".into()
            }
        }
        DecisionPoint::DiscardHalf { count, .. } => format!(
            "A 7 was rolled and you hold too many cards: discard exactly {count} cards from your hand."
        ),
        DecisionPoint::MoveRobber { .. } => format!(
            "Move the robber to any other hex (currently h{}). Blocking a hex stops its production; you steal from a player settled there.",
            obs.robber
        ),
        DecisionPoint::ChooseVictim { victims, .. } => format!(
            "Choose whom to steal one random card from: {}.",
            ids("P", victims)
        ),
        DecisionPoint::TurnAction { .. } => {
            let mut s = String::from(
                "Take one action: build_road / build_settlement / build_city / buy_dev / play a dev card / bank_trade / propose_trade / say / end_turn.",
            );
            let roads = obs.road_sites();
            let setts = obs.settlement_sites();
            if !setts.is_empty() {
                s.push_str(&format!(" Legal settlement sites: {}.", ids("v", &setts)));
            }
            if !roads.is_empty() {
                s.push_str(&format!(" Legal road edges: {}.", ids("e", &roads)));
            }
            s
        }
        DecisionPoint::FreeRoad { remaining, .. } => format!(
            "Road Building: place a free road ({remaining} remaining). Legal edges: {}.",
            ids("e", &obs.road_sites())
        ),
        DecisionPoint::RespondTrade { proposer, offer, .. } => format!(
            "Player {proposer} proposes a trade: {}. Accept it (you pay their asking side), reject it, or counter with your own terms.",
            offer_line(offer)
        ),
        DecisionPoint::ResolveTrade { accepters, counters, .. } => {
            let mut s = String::from("Close your trade window: ");
            if !accepters.is_empty() {
                s.push_str(&format!(
                    "accepted by {}; ",
                    ids("P", accepters)
                ));
            }
            for (seat, counter) in counters {
                s.push_str(&format!(
                    "P{seat} countered ({}); ",
                    offer_line(counter)
                ));
            }
            s.push_str(
                "pick a partner to execute with, or cancel with partner = null.",
            );
            s
        }
    }
}

/// The strict JSON shape for each decision, shown verbatim to the model.
pub fn decision_schema(decision: &DecisionPoint) -> &'static str {
    match decision {
        DecisionPoint::SetupSettlement { .. } => {
            r#"{"action": "place_setup_settlement", "vertex": <number>, "thought_process": "..."}"#
        }
        DecisionPoint::SetupRoad { .. } => {
            r#"{"action": "place_setup_road", "edge": <number>, "thought_process": "..."}"#
        }
        DecisionPoint::PreRoll { .. } => {
            r#"{"action": "roll", "thought_process": "..."}  OR  {"action": "play_knight", "thought_process": "..."}"#
        }
        DecisionPoint::DiscardHalf { .. } => {
            r#"{"action": "discard", "resources": {"brick": 0, "lumber": 0, "wool": 0, "grain": 0, "ore": 0}, "thought_process": "..."}"#
        }
        DecisionPoint::MoveRobber { .. } => {
            r#"{"action": "move_robber", "hex": <number>, "thought_process": "..."}"#
        }
        DecisionPoint::ChooseVictim { .. } => {
            r#"{"action": "steal_from", "seat": <number>, "thought_process": "..."}"#
        }
        DecisionPoint::TurnAction { .. } => {
            r#"One of:
{"action": "build_road", "edge": <number>}
{"action": "build_settlement", "vertex": <number>}
{"action": "build_city", "vertex": <number>}
{"action": "buy_dev"}
{"action": "play_knight"}
{"action": "play_road_building"}
{"action": "play_year_of_plenty", "first": "<resource>", "second": "<resource>"}
{"action": "play_monopoly", "resource": "<resource>"}
{"action": "bank_trade", "give": "<resource>", "receive": "<resource>"}
{"action": "propose_trade", "give": {"brick": 0, ...}, "receive": {"brick": 0, ...}, "to": <seat or null for everyone>, "message": "optional public table talk"}
{"action": "say", "message": "public table talk"}
{"action": "end_turn"}
Each may carry "thought_process". Resources are one of: brick, lumber, wool, grain, ore."#
        }
        DecisionPoint::FreeRoad { .. } => {
            r#"{"action": "build_road", "edge": <number>, "thought_process": "..."}"#
        }
        DecisionPoint::RespondTrade { .. } => {
            r#"One of:
{"action": "accept_trade", "message": "optional public reply"}
{"action": "reject_trade", "message": "optional public reply"}
{"action": "counter_trade", "give": {"brick": 0, ...}, "receive": {"brick": 0, ...}, "message": "optional public reply"}
("give"/"receive" are from YOUR perspective: what you would hand over and what you want back.) Each may carry "thought_process"."#
        }
        DecisionPoint::ResolveTrade { .. } => {
            r#"{"action": "resolve_trade", "partner": <seat number or null to cancel>, "thought_process": "..."}"#
        }
    }
}

/// A fixed, deterministic render of every prompt-shaping function — any
/// wording edit anywhere changes this string, hence the scaffold id.
fn golden_prompt_render() -> String {
    let state = GameState::new(0xC0FFEE);
    let obs = state.observe(0);
    let sample_offer = TradeOffer {
        give: crate::catan::types::ResourceSet::of(RESOURCES[0], 1),
        receive: crate::catan::types::ResourceSet::of(RESOURCES[3], 2),
    };
    let decisions = vec![
        DecisionPoint::SetupSettlement { seat: 0, round: 0 },
        DecisionPoint::SetupRoad { seat: 0, at: 0 },
        DecisionPoint::PreRoll {
            seat: 0,
            can_play_knight: true,
        },
        DecisionPoint::DiscardHalf { seat: 0, count: 4 },
        DecisionPoint::MoveRobber { seat: 0 },
        DecisionPoint::ChooseVictim {
            seat: 0,
            victims: vec![1, 2],
        },
        DecisionPoint::TurnAction { seat: 0 },
        DecisionPoint::FreeRoad {
            seat: 0,
            remaining: 2,
        },
        DecisionPoint::RespondTrade {
            seat: 1,
            proposer: 0,
            offer: sample_offer,
        },
        DecisionPoint::ResolveTrade {
            seat: 0,
            accepters: vec![1],
            counters: vec![(2, sample_offer)],
        },
    ];
    let mut s = String::new();
    s.push_str(&seat_brief(&obs));
    s.push_str(&render_observation(&obs));
    for d in &decisions {
        s.push_str(&decision_ask(&obs, d));
        s.push_str(decision_schema(d));
    }
    // Every event render arm, via the fixed golden event set.
    for record in &super::golden::sample_events() {
        s.push_str(&record.render());
    }
    s
}

/// Version hash over every model-visible prompt surface + persona +
/// temperature. Any wording edit anywhere invalidates the scaffold id so
/// rating attribution can't silently drift.
pub fn scaffold_version(persona: Option<&str>, temperature: f32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(PROMPT_REVISION);
    hasher.update(RULES_SUMMARY);
    hasher.update(OUTPUT_CONTRACT);
    hasher.update(golden_prompt_render());
    hasher.update(persona.unwrap_or(""));
    hasher.update(temperature.to_le_bytes());
    let digest = hasher.finalize();
    format!("sc-{digest:x}")[..11].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_render_is_deterministic_and_scaffold_stable() {
        assert_eq!(golden_prompt_render(), golden_prompt_render());
        let a = scaffold_version(None, 0.5);
        assert_eq!(a, scaffold_version(None, 0.5));
        assert_ne!(a, scaffold_version(Some("aggressive"), 0.5));
        assert_ne!(a, scaffold_version(None, 0.7));
        assert!(a.starts_with("sc-"));
    }

    #[test]
    fn observation_render_lists_board_and_status() {
        let state = GameState::new(7);
        let obs = state.observe(2);
        let r = render_observation(&obs);
        assert!(r.contains("== BOARD =="));
        assert!(r.contains("== STATUS =="));
        assert!(r.contains("h18:"));
        assert!(r.contains("Your hand:"));
    }
}
