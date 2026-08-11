//! Human/prompt-facing rendering of Catan events. One neutral wording serves
//! prompts, replays, and reports: private events only ever appear in entitled
//! observations, so the words themselves need no per-seat variants.

use super::events::{CatanEvent, TradeResponse};
use super::types::Seat;

fn p(seat: Seat) -> String {
    format!("Player {seat}")
}

impl CatanEvent {
    pub fn render(&self) -> String {
        use CatanEvent::*;
        match self {
            GameStarted { players } => format!("A {players}-player game of Catan begins."),
            BoardLaid { desert, .. } => {
                format!("The board is laid out; the desert (hex {desert}) hosts the robber.")
            }
            TurnStarted { seat, turn } => format!("— Turn {turn}: {} —", p(*seat)),
            SetupSettlementPlaced {
                seat,
                vertex,
                round,
            } => format!(
                "{} places setup settlement {} at vertex {vertex}.",
                p(*seat),
                round + 1
            ),
            SetupRoadPlaced { seat, edge } => {
                format!("{} places a setup road on edge {edge}.", p(*seat))
            }
            SetupResourcesGranted { seat, gained } => {
                format!(
                    "{} collects starting resources: {}.",
                    p(*seat),
                    gained.describe()
                )
            }
            DiceRolled { seat, d1, d2 } => {
                format!("{} rolls {d1}+{d2} = {}.", p(*seat), d1 + d2)
            }
            ResourcesProduced { gains } => {
                let parts: Vec<String> = gains
                    .iter()
                    .map(|(s, set)| format!("{} gains {}", p(*s), set.describe()))
                    .collect();
                format!("Production: {}.", parts.join("; "))
            }
            ResourceShortage { resource } => format!(
                "The bank cannot cover everyone's {}; nobody receives it this roll.",
                resource.as_str()
            ),
            MustDiscard { seats } => {
                let parts: Vec<String> = seats
                    .iter()
                    .map(|(s, n)| format!("{} ({n} cards)", p(*s)))
                    .collect();
                format!(
                    "A 7! Over-limit hands must discard half: {}.",
                    parts.join(", ")
                )
            }
            Discarded { seat, count } => format!("{} discards {count} cards.", p(*seat)),
            DiscardedContents { seat, resources } => {
                format!("[private] {} discarded {}.", p(*seat), resources.describe())
            }
            RobberMoved { seat, hex } => format!("{} moves the robber to hex {hex}.", p(*seat)),
            CardStolen { from, to } => {
                format!("{} steals a card from {}.", p(*to), p(*from))
            }
            CardStolenContents { from, to, resource } => format!(
                "[private] The stolen card was {} ({} → {}).",
                resource.as_str(),
                p(*from),
                p(*to)
            ),
            RoadBuilt { seat, edge, free } => {
                if *free {
                    format!(
                        "{} places a free road on edge {edge} (Road Building).",
                        p(*seat)
                    )
                } else {
                    format!("{} builds a road on edge {edge}.", p(*seat))
                }
            }
            SettlementBuilt { seat, vertex } => {
                format!("{} builds a settlement at vertex {vertex}.", p(*seat))
            }
            CityBuilt { seat, vertex } => {
                format!("{} upgrades vertex {vertex} to a city.", p(*seat))
            }
            DevCardBought { seat } => format!("{} buys a development card.", p(*seat)),
            DevCardDrawn { seat, card } => {
                format!("[private] {} drew {}.", p(*seat), card.as_str())
            }
            DevCardPlayed { seat, card } => format!("{} plays {}.", p(*seat), card.as_str()),
            YearOfPlentyTaken {
                seat,
                first,
                second,
            } => format!(
                "{} takes {} and {} from the bank.",
                p(*seat),
                first.as_str(),
                second.as_str()
            ),
            MonopolyResolved {
                seat,
                resource,
                taken,
            } => {
                let total: u8 = taken.iter().map(|(_, n)| n).sum();
                format!(
                    "{} monopolizes {}: collects {total} card(s) from the table.",
                    p(*seat),
                    resource.as_str()
                )
            }
            BankTraded {
                seat,
                gave,
                got,
                rate,
            } => format!(
                "{} trades {} for {} with the bank ({rate}:1).",
                p(*seat),
                gave.describe(),
                got.describe()
            ),
            TradeProposed {
                seat,
                to,
                offer,
                message,
            } => {
                let target = match to {
                    Some(t) => p(*t),
                    None => "everyone".into(),
                };
                let mut s = format!(
                    "{} offers {}: {} for {}.",
                    p(*seat),
                    target,
                    offer.give.describe(),
                    offer.receive.describe()
                );
                if let Some(m) = message {
                    s.push_str(&format!(" \"{m}\""));
                }
                s
            }
            TradeResponded {
                seat,
                response,
                message,
            } => {
                let verdict = match response {
                    TradeResponse::Accept => "accepts the offer".to_string(),
                    TradeResponse::Reject => "declines".to_string(),
                    TradeResponse::Counter { offer } => format!(
                        "counters: they would give {} for {}",
                        offer.receive.describe(),
                        offer.give.describe()
                    ),
                };
                let mut s = format!("{} {verdict}.", p(*seat));
                if let Some(m) = message {
                    s.push_str(&format!(" \"{m}\""));
                }
                s
            }
            TradeExecuted {
                proposer,
                with,
                offer,
            } => format!(
                "Trade executed: {} gives {} to {} for {}.",
                p(*proposer),
                offer.give.describe(),
                p(*with),
                offer.receive.describe()
            ),
            TradeWindowClosed { seat } => format!("{} closes the trade window.", p(*seat)),
            Said { seat, text } => format!("{}: \"{text}\"", p(*seat)),
            LongestRoadClaimed {
                seat,
                length,
                previous,
            } => match (seat, previous) {
                (Some(s), Some(prev)) => format!(
                    "{} takes Longest Road ({length} segments) from {}.",
                    p(*s),
                    p(*prev)
                ),
                (Some(s), None) => format!("{} claims Longest Road ({length} segments).", p(*s)),
                (None, Some(prev)) => {
                    format!("Longest Road is severed and set aside (was {}).", p(*prev))
                }
                (None, None) => "Longest Road remains unclaimed.".into(),
            },
            LargestArmyClaimed {
                seat,
                knights,
                previous,
            } => match previous {
                Some(prev) => format!(
                    "{} takes Largest Army ({knights} knights) from {}.",
                    p(*seat),
                    p(*prev)
                ),
                None => format!("{} claims Largest Army ({knights} knights).", p(*seat)),
            },
            ForcedDefault { seat, decision } => format!(
                "{} failed to answer ({decision}); the engine applied the forced legal default.",
                p(*seat)
            ),
            GameEnded { winner, vps, turns } => {
                let scores: Vec<String> = vps
                    .iter()
                    .enumerate()
                    .map(|(s, v)| format!("{}: {v} VP", p(s as Seat)))
                    .collect();
                format!(
                    "Game over after {turns} turns — {} wins! Final scores: {}.",
                    p(*winner),
                    scores.join(", ")
                )
            }
        }
    }
}
