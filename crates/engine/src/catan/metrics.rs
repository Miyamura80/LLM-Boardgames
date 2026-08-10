//! Objective, engine-derived per-seat metrics, computed by replaying the
//! event log (no LLM judge). Forced-default decisions carry no play-quality
//! signal and surface only in the reliability counters.

use super::board::{graph, BoardLayout, HexId};
use super::events::CatanEvent;
use super::runner::GameRecord;
use super::sites;
use super::types::{Seat, PLAYER_COUNT};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The per-seat metric suite (documented definitions; all proxies, placement
/// adjudicates).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SeatMetrics {
    pub seat: Seat,
    pub final_vp: u8,
    pub placement: u8,
    /// Cards actually gained from production rolls.
    pub production_actual: u32,
    /// Pip-model expectation: Σ over each roll of (owned yields × p(number)).
    /// `actual/expected` ≈ 1 absent robber pressure and bank shortages.
    pub production_expected: f64,
    /// Mean pip-percentile of the two setup picks among the sites legal at
    /// pick time (1.0 = took the best available vertex).
    pub placement_pips_pct: Option<f64>,
    pub trades_proposed: u32,
    pub trades_executed: u32,
    /// Cards handed out / taken in across executed player trades.
    pub trade_cards_out: u32,
    pub trade_cards_in: u32,
    pub robber_moves: u32,
    /// Robber placements that blocked the current public-VP leader.
    pub robber_on_leader: u32,
}

/// Replayed public scoreboard: buildings + awards (hidden VP invisible here,
/// matching what a robber-placing player could actually see).
struct Tracker {
    layout: Option<BoardLayout>,
    buildings: Vec<Option<(Seat, bool)>>,
    public_vp: [i32; PLAYER_COUNT as usize],
    robber: HexId,
    metrics: Vec<SeatMetrics>,
    placement_pcts: Vec<Vec<f64>>,
}

pub fn score_game(record: &GameRecord) -> Vec<SeatMetrics> {
    let mut t = Tracker {
        layout: None,
        buildings: vec![None; super::board::VERTEX_COUNT],
        public_vp: [0; PLAYER_COUNT as usize],
        robber: 0,
        metrics: (0..PLAYER_COUNT)
            .map(|s| SeatMetrics {
                seat: s,
                ..Default::default()
            })
            .collect(),
        placement_pcts: vec![Vec::new(); PLAYER_COUNT as usize],
    };

    for event in record.events.iter().map(|r| &r.event) {
        replay(&mut t, event);
    }

    for (i, m) in t.metrics.iter_mut().enumerate() {
        m.final_vp = record.final_vps[i];
        m.placement = record.seats[i].placement;
        let pcts = &t.placement_pcts[i];
        m.placement_pips_pct =
            (!pcts.is_empty()).then(|| pcts.iter().sum::<f64>() / pcts.len() as f64);
    }
    t.metrics
}

fn replay(t: &mut Tracker, event: &CatanEvent) {
    match event {
        CatanEvent::BoardLaid { hexes, desert, .. } => {
            let mut terrains = Vec::with_capacity(hexes.len());
            let mut numbers = Vec::with_capacity(hexes.len());
            for h in hexes {
                terrains.push(h.terrain);
                numbers.push(h.number);
            }
            t.layout = Some(BoardLayout {
                terrains,
                numbers,
                ports: Vec::new(),
                desert: *desert,
            });
            t.robber = *desert;
        }
        CatanEvent::SetupSettlementPlaced { seat, vertex, .. } => {
            // Percentile of the pick among the sites legal at pick time.
            if let Some(layout) = &t.layout {
                let open = sites::setup_settlement_sites(&t.buildings);
                if !open.is_empty() {
                    let pips = |v: u8| -> u32 {
                        graph().vertex_hexes[v as usize]
                            .iter()
                            .filter_map(|&h| layout.numbers[h as usize])
                            .map(|n| BoardLayout::pips(n) as u32)
                            .sum()
                    };
                    let mine = pips(*vertex);
                    let not_better = open.iter().filter(|&&v| pips(v) <= mine).count() as f64;
                    t.placement_pcts[*seat as usize].push(not_better / open.len() as f64);
                }
            }
            t.buildings[*vertex as usize] = Some((*seat, false));
            t.public_vp[*seat as usize] += 1;
        }
        CatanEvent::SettlementBuilt { seat, vertex } => {
            t.buildings[*vertex as usize] = Some((*seat, false));
            t.public_vp[*seat as usize] += 1;
        }
        CatanEvent::CityBuilt { seat, vertex } => {
            t.buildings[*vertex as usize] = Some((*seat, true));
            t.public_vp[*seat as usize] += 1;
        }
        CatanEvent::LongestRoadClaimed { seat, previous, .. } => {
            if let Some(prev) = previous {
                t.public_vp[*prev as usize] -= 2;
            }
            if let Some(s) = seat {
                t.public_vp[*s as usize] += 2;
            }
        }
        CatanEvent::LargestArmyClaimed { seat, previous, .. } => {
            if let Some(prev) = previous {
                t.public_vp[*prev as usize] -= 2;
            }
            t.public_vp[*seat as usize] += 2;
        }
        CatanEvent::DiceRolled { d1, d2, .. } => {
            if d1 + d2 == 7 {
                return;
            }
            // Pip-model expectation accrues on every productive roll.
            let Some(layout) = &t.layout else { return };
            let g = graph();
            for (h, num) in layout.numbers.iter().enumerate() {
                let Some(n) = num else { continue };
                if t.robber == h as HexId || layout.terrains[h].resource().is_none() {
                    continue;
                }
                let p = BoardLayout::pips(*n) as f64 / 36.0;
                for &v in &g.hex_vertices[h] {
                    if let Some((owner, is_city)) = t.buildings[v as usize] {
                        let yield_ = if is_city { 2.0 } else { 1.0 };
                        t.metrics[owner as usize].production_expected += p * yield_;
                    }
                }
            }
        }
        CatanEvent::ResourcesProduced { gains } => {
            for (seat, set) in gains {
                t.metrics[*seat as usize].production_actual += set.total() as u32;
            }
        }
        CatanEvent::RobberMoved { seat, hex } => {
            t.robber = *hex;
            let m = &mut t.metrics[*seat as usize];
            m.robber_moves += 1;
            // Did the placement block the current public-VP leader?
            let leader = (0..PLAYER_COUNT)
                .filter(|s| s != seat)
                .max_by_key(|&s| t.public_vp[s as usize])
                .expect("three opponents");
            let g = graph();
            let leader_here = g.hex_vertices[*hex as usize]
                .iter()
                .any(|&v| matches!(t.buildings[v as usize], Some((o, _)) if o == leader));
            if leader_here {
                m.robber_on_leader += 1;
            }
        }
        CatanEvent::TradeProposed { seat, .. } => {
            t.metrics[*seat as usize].trades_proposed += 1;
        }
        CatanEvent::TradeExecuted {
            proposer,
            with,
            offer,
        } => {
            let give = offer.give.total() as u32;
            let receive = offer.receive.total() as u32;
            let p = &mut t.metrics[*proposer as usize];
            p.trades_executed += 1;
            p.trade_cards_out += give;
            p.trade_cards_in += receive;
            let w = &mut t.metrics[*with as usize];
            w.trades_executed += 1;
            w.trade_cards_out += receive;
            w.trade_cards_in += give;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catan::runner::{run_game, AgentFactory, AgentSpec, CatanAgentKind, GameConfig};
    use crate::llm::{ProviderKeys, RetryPolicy};

    #[tokio::test]
    async fn scoring_a_real_bot_game_yields_consistent_metrics() {
        let f = AgentFactory {
            keys: ProviderKeys::default(),
            retry: RetryPolicy::default(),
            temperature: 0.5,
            max_tokens: 64,
            message_char_cap: 240,
        };
        let mut agents = Vec::new();
        for seat in 0..4u64 {
            agents.push(
                f.build(&AgentSpec::bot(CatanAgentKind::Greedy), 7 ^ (seat << 8))
                    .expect("bot"),
            );
        }
        let cfg = GameConfig {
            seed: 7,
            ..GameConfig::default()
        };
        let record = run_game(&cfg, &mut agents, &[false; 4]).await;
        let metrics = score_game(&record);
        assert_eq!(metrics.len(), 4);
        for m in &metrics {
            assert_eq!(m.final_vp, record.final_vps[m.seat as usize]);
            // Both setup picks scored, and picks are legal-percentile bounded.
            let pct = m.placement_pips_pct.expect("two setup picks");
            assert!((0.0..=1.0).contains(&pct));
            // The greedy bot always takes the best available vertex.
            assert!(pct > 0.9, "greedy setup percentile was {pct}");
            assert!(m.production_expected > 0.0);
        }
    }
}
