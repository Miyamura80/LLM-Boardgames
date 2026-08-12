//! Objective, engine-derived per-seat metrics (PRD-codenames-evals US-CN10),
//! computed by replaying the public event log against the key card. No LLM
//! judge is involved anywhere: every number below is a count of things the
//! engine itself adjudicated.
//!
//! **Forced defaults are metric-exempt.** A [`ForcedDefault`] event is emitted
//! immediately before the action it forces, so the replay marks the next action
//! by that seat as forced and skips it. A forced *clue* also suppresses
//! spymaster attribution for every reveal made under it — the spymaster did not
//! choose that clue, so its yield is not evidence about the spymaster.
//!
//! **Proxy caveats**, in full:
//!
//! - *Clue yield* divides own-agents-revealed by the clue's stated number. A
//!   spymaster whose operative under-guesses is punished for the operative's
//!   caution; controlled mode holds that partner constant so the confound is
//!   shared by every candidate rather than removed.
//! - *Agents found per clue* and the hit counters are attributed to the
//!   spymaster who gave the clue, but they are executed by the operative. They
//!   measure the *pair*, read as a spymaster line only because the schedule
//!   fixes the partner.
//! - *Overreach* counts only the bonus (number + 1)th guess. Wrong guesses
//!   inside the stated number are ordinary misses, not overreach.
//! - *Pass discipline* is the honest weak point. The engine cannot know which
//!   word the operative *would* have guessed next, so the counterfactual is the
//!   board average: at each voluntary pass, the share of face-down cards that
//!   are not own agents — the miss probability of an uninformed next guess. A
//!   high mean means passes were taken on dangerous boards. It is a hazard
//!   measure, not a proof the pass was right.
//! - Reliability counters (malformed / illegal / forced / transport) live on
//!   [`SeatRecord`](super::runner::SeatRecord) and are reported separately;
//!   they are never folded into play quality.
//!
//! [`ForcedDefault`]: super::events::CodenamesEvent::ForcedDefault

use super::board::{Board, KeyCard};
use super::events::CodenamesEvent;
use super::runner::GameRecord;
use super::types::{seat_role, seat_team, CardIdentity, EndReason, Role, Seat, Team, SEAT_COUNT};
use super::wordlist::Wordlist;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The per-seat metric suite. Counters and their denominators are stored raw so
/// a run can be re-aggregated in SQL; the ratio accessors below are the
/// documented readings.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SeatMetrics {
    pub seat: Seat,
    pub role: Role,
    pub team: Team,
    // ---- outcome ----------------------------------------------------------
    pub won: bool,
    /// This seat's team lost because its operative revealed the assassin.
    pub assassin_loss: bool,
    /// Team turns played before the game ended (identical for all four seats;
    /// carried per seat so a seat row is self-contained).
    pub turns: u32,
    // ---- spymaster --------------------------------------------------------
    /// Clues this seat chose (forced clues excluded).
    pub clues_given: u32,
    /// Σ of the numbers on those clues → `mean_clue_number`.
    pub clue_number_sum: u32,
    /// Own agents revealed under those clues → `agents_per_clue`.
    pub agents_found: u32,
    /// Σ of intended numbers, the clue-yield denominator (= `clue_number_sum`,
    /// kept separate so the ratio survives future changes to either side).
    pub clue_yield_den: u32,
    /// Opposing agents revealed under this seat's own clues.
    pub enemy_hits_caused: u32,
    pub bystander_hits_caused: u32,
    /// The assassin revealed under this seat's own clue — the fatal one.
    pub assassin_hits_caused: u32,
    // ---- operative --------------------------------------------------------
    /// Guesses this seat chose (forced guesses excluded).
    pub guesses: u32,
    /// Guesses that revealed an own agent → `guess_precision`.
    pub guess_hits: u32,
    /// Mandatory first guesses under a clue → `first_guess_precision`.
    pub first_guesses: u32,
    pub first_guess_hits: u32,
    /// Times the optional (number + 1)th guess was taken → `overreach_rate`.
    pub bonus_guesses: u32,
    /// Of those, the ones that did not reveal an own agent.
    pub bonus_misses: u32,
    /// Voluntary passes with guesses still available.
    pub passes: u32,
    /// Σ over those passes of the share of face-down cards that were not own
    /// agents → `pass_discipline`.
    pub pass_hazard_sum: f64,
    /// Assassin reveals by this seat.
    pub assassin_hits: u32,
}

impl SeatMetrics {
    fn new(seat: Seat) -> Self {
        Self {
            seat,
            role: seat_role(seat),
            team: seat_team(seat),
            won: false,
            assassin_loss: false,
            turns: 0,
            clues_given: 0,
            clue_number_sum: 0,
            agents_found: 0,
            clue_yield_den: 0,
            enemy_hits_caused: 0,
            bystander_hits_caused: 0,
            assassin_hits_caused: 0,
            guesses: 0,
            guess_hits: 0,
            first_guesses: 0,
            first_guess_hits: 0,
            bonus_guesses: 0,
            bonus_misses: 0,
            passes: 0,
            pass_hazard_sum: 0.0,
            assassin_hits: 0,
        }
    }

    /// Mean number attached to this seat's clues (bigger = more ambitious).
    pub fn mean_clue_number(&self) -> Option<f64> {
        ratio(self.clue_number_sum as f64, self.clues_given as f64)
    }
    /// Own agents revealed per clue given.
    pub fn agents_per_clue(&self) -> Option<f64> {
        ratio(self.agents_found as f64, self.clues_given as f64)
    }
    /// Own agents revealed ÷ agents promised. 1.0 = every clue paid in full.
    pub fn clue_yield(&self) -> Option<f64> {
        ratio(self.agents_found as f64, self.clue_yield_den as f64)
    }
    /// Share of chosen guesses that hit an own agent.
    pub fn guess_precision(&self) -> Option<f64> {
        ratio(self.guess_hits as f64, self.guesses as f64)
    }
    /// Share of mandatory first guesses that hit an own agent.
    pub fn first_guess_precision(&self) -> Option<f64> {
        ratio(self.first_guess_hits as f64, self.first_guesses as f64)
    }
    /// Share of taken bonus guesses that missed (see the caveat above).
    pub fn overreach_rate(&self) -> Option<f64> {
        ratio(self.bonus_misses as f64, self.bonus_guesses as f64)
    }
    /// Mean board hazard at the moment of a voluntary pass.
    pub fn pass_discipline(&self) -> Option<f64> {
        ratio(self.pass_hazard_sum, self.passes as f64)
    }
}

fn ratio(num: f64, den: f64) -> Option<f64> {
    (den > 0.0).then(|| num / den)
}

/// Recover a finished game's key card by regenerating its board from the seed
/// and the pool it was drawn from — the key is engine state, never an event, so
/// it is reconstructed rather than stored twice.
///
/// Returns `None` when `wordlist` is not the pool the game used (hash or word
/// mismatch): scoring a record against a foreign key would silently invent
/// identities, so it is refused.
pub fn key_card_for(record: &GameRecord, wordlist: &Wordlist) -> Option<KeyCard> {
    if wordlist.content_hash() != record.wordlist_hash {
        return None;
    }
    let board = Board::generate(&mut ChaCha8Rng::seed_from_u64(record.seed), wordlist);
    let laid = record.events.iter().find_map(|r| match &r.event {
        CodenamesEvent::BoardLaid { words } => Some(words),
        _ => None,
    })?;
    (&board.words() == laid).then(|| board.key_card())
}

/// Replay state: which cards are face-down, and what the live clue was.
struct Tracker<'a> {
    key: &'a KeyCard,
    /// Board words in grid order (from `BoardLaid`).
    words: Vec<String>,
    revealed: Vec<bool>,
    clue: Option<LiveClue>,
    /// The seat whose next action the engine forced (metric-exempt).
    forced_seat: Option<Seat>,
    metrics: Vec<SeatMetrics>,
}

struct LiveClue {
    spymaster: Seat,
    number: u8,
    guesses: u8,
    /// The clue itself was a forced default — its outcomes say nothing about
    /// the spymaster.
    forced: bool,
}

/// Score one finished game against its key card.
pub fn score_game(record: &GameRecord, key: &KeyCard) -> Vec<SeatMetrics> {
    let mut t = Tracker {
        key,
        words: Vec::new(),
        revealed: vec![false; key.identities.len()],
        clue: None,
        forced_seat: None,
        metrics: (0..SEAT_COUNT).map(SeatMetrics::new).collect(),
    };

    for event in record.events.iter().map(|r| &r.event) {
        replay(&mut t, event);
    }

    for (i, m) in t.metrics.iter_mut().enumerate() {
        m.turns = record.turns;
        m.won = record.seats[i].won;
        m.assassin_loss = record.end_reason == EndReason::Assassin && !record.seats[i].won;
    }
    t.metrics
}

fn replay(t: &mut Tracker, event: &CodenamesEvent) {
    match event {
        CodenamesEvent::BoardLaid { words } => {
            t.words = words.clone();
        }
        CodenamesEvent::ForcedDefault { seat, .. } => {
            // The forced action is the very next event by this seat.
            t.forced_seat = Some(*seat);
            return;
        }
        CodenamesEvent::ClueGiven { seat, clue, .. } => {
            let forced = t.is_forced(*seat);
            if !forced {
                let m = &mut t.metrics[*seat as usize];
                m.clues_given += 1;
                m.clue_number_sum += clue.number as u32;
                m.clue_yield_den += clue.number as u32;
            }
            t.clue = Some(LiveClue {
                spymaster: *seat,
                number: clue.number,
                guesses: 0,
                forced,
            });
        }
        CodenamesEvent::GuessRevealed {
            seat,
            team,
            word,
            identity,
            ..
        } => {
            let forced = t.is_forced(*seat);
            let own = *identity == CardIdentity::Agent { team: *team };
            let (index, number, clue_forced, spymaster) = match &t.clue {
                Some(c) => (c.guesses, c.number, c.forced, c.spymaster),
                // A reveal outside a clue cannot happen in a legal transcript;
                // score it as a plain guess rather than panicking on a record
                // written by an older rules version.
                None => (0, u8::MAX, true, *seat),
            };

            if !forced {
                let m = &mut t.metrics[*seat as usize];
                m.guesses += 1;
                if own {
                    m.guess_hits += 1;
                }
                if index == 0 {
                    m.first_guesses += 1;
                    if own {
                        m.first_guess_hits += 1;
                    }
                }
                // The (number + 1)th guess is the optional one: 0-based index
                // equal to the stated number.
                if index >= number {
                    m.bonus_guesses += 1;
                    if !own {
                        m.bonus_misses += 1;
                    }
                }
                if *identity == CardIdentity::Assassin {
                    m.assassin_hits += 1;
                }
            }
            // Attribute to the spymaster only when neither the clue nor the
            // guess was forced.
            if !forced && !clue_forced {
                let m = &mut t.metrics[spymaster as usize];
                match identity {
                    CardIdentity::Agent { team: t2 } if t2 == team => m.agents_found += 1,
                    CardIdentity::Agent { .. } => m.enemy_hits_caused += 1,
                    CardIdentity::Bystander => m.bystander_hits_caused += 1,
                    CardIdentity::Assassin => m.assassin_hits_caused += 1,
                }
            }

            if let Some(c) = &mut t.clue {
                c.guesses += 1;
            }
            t.reveal(word);
        }
        CodenamesEvent::TurnPassed { seat, team } => {
            let forced = t.is_forced(*seat);
            let remaining = t
                .clue
                .as_ref()
                // guess cap is number + 1.
                .map(|c| c.guesses < c.number.saturating_add(1))
                .unwrap_or(false);
            if !forced && remaining {
                let hazard = t.hazard(*team);
                let m = &mut t.metrics[*seat as usize];
                m.passes += 1;
                m.pass_hazard_sum += hazard;
            }
            t.clue = None;
        }
        CodenamesEvent::TurnStarted { .. }
        | CodenamesEvent::GameStarted { .. }
        | CodenamesEvent::GameEnded { .. } => {}
    }
    // Any event other than ForcedDefault consumes the pending forced marker:
    // the forced action is always the immediately following event.
    t.forced_seat = None;
}

impl Tracker<'_> {
    /// Was this seat's current action a forced default? (The marker is cleared
    /// by `replay` once the forced action has been consumed.)
    fn is_forced(&self, seat: Seat) -> bool {
        self.forced_seat == Some(seat)
    }

    fn reveal(&mut self, word: &str) {
        if let Some(i) = self.words.iter().position(|w| w == word) {
            if i < self.revealed.len() {
                self.revealed[i] = true;
            }
        }
    }

    /// Share of face-down cards that are *not* `team`'s agents — the miss
    /// probability of an uninformed next guess.
    fn hazard(&self, team: Team) -> f64 {
        let mut down = 0u32;
        let mut safe = 0u32;
        for (i, identity) in self.key.identities.iter().enumerate() {
            if self.revealed.get(i).copied().unwrap_or(false) {
                continue;
            }
            down += 1;
            if *identity == (CardIdentity::Agent { team }) {
                safe += 1;
            }
        }
        if down == 0 {
            return 0.0;
        }
        1.0 - safe as f64 / down as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::runner::{run_game, AgentFactory, AgentSpec, CodenamesAgentKind};
    use crate::codenames::runner::{GameConfig, GameRecord};
    use crate::codenames::testkit;
    use crate::llm::{ProviderKeys, RetryPolicy};

    async fn bot_game(seed: u64) -> GameRecord {
        let f = AgentFactory {
            keys: ProviderKeys::default(),
            retry: RetryPolicy::default(),
            temperature: 0.5,
            max_tokens: 64,
            vectors: None,
        };
        let mut agents = Vec::new();
        for seat in 0..SEAT_COUNT as u64 {
            agents.push(
                f.build(
                    &AgentSpec::bot(CodenamesAgentKind::Random),
                    seed ^ seat << 8,
                )
                .expect("bot builds"),
            );
        }
        let cfg = GameConfig {
            seed,
            wordlist: testkit::test_wordlist(),
            ..GameConfig::default()
        };
        run_game(&cfg, &mut agents, &[false; SEAT_COUNT as usize]).await
    }

    #[tokio::test]
    async fn a_real_bot_game_scores_consistently() {
        let record = bot_game(5).await;
        let key = key_card_for(&record, &testkit::test_wordlist()).expect("key reconstructs");
        let metrics = score_game(&record, &key);
        assert_eq!(metrics.len(), SEAT_COUNT as usize);

        for m in &metrics {
            assert_eq!(m.turns, record.turns);
            assert_eq!(m.won, record.seats[m.seat as usize].won);
            // Denominators bound their numerators.
            assert!(m.guess_hits <= m.guesses);
            assert!(m.first_guess_hits <= m.first_guesses);
            assert!(m.bonus_misses <= m.bonus_guesses);
            assert!(m.first_guesses <= m.guesses);
            for r in [
                m.guess_precision(),
                m.first_guess_precision(),
                m.overreach_rate(),
                m.pass_discipline(),
            ] {
                assert!(r.is_none_or(|v| (0.0..=1.0).contains(&v)), "{r:?}");
            }
            match m.role {
                Role::Spymaster => assert_eq!(m.guesses, 0, "spymasters never guess"),
                Role::Operative => assert_eq!(m.clues_given, 0, "operatives never clue"),
            }
        }
        // Every own-agent reveal is somebody's `agents_found`.
        let found: u32 = metrics.iter().map(|m| m.agents_found).sum();
        let hits: u32 = metrics.iter().map(|m| m.guess_hits).sum();
        assert_eq!(found, hits, "attribution must not double count or drop");
    }

    /// The key is refused (rather than fabricated) when the pool differs.
    #[tokio::test]
    async fn a_foreign_wordlist_yields_no_key() {
        let record = bot_game(6).await;
        assert!(key_card_for(&record, &Wordlist::default_embedded()).is_none());
    }
}
