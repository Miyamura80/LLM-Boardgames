//! `EmbeddingGreedyBot` — the non-LLM **anchor** seat (PRD-codenames-evals
//! US-CN06). Unlike [`RandomLegalBot`](super::RandomLegalBot), which only
//! proves legality, this bot gives *meaningful* clues, so the opposing-team
//! comparison a Codenames rating rests on has a real floor instead of noise.
//!
//! It is a deterministic function of the observation alone — no RNG, no
//! tokens. Same board, same clue; same clue, same guesses.
//!
//! ```text
//!   spymaster                                   operative
//!   ─────────                                   ─────────
//!   for each clue candidate c in the table      sim(clue, w) for every
//!   that is LEGAL on this board:                face-down w with a vector
//!     rank own face-down agents by sim(c, ·)             │
//!     grow a cluster while sim >= floor                  ▼
//!     score = min sim over the cluster            rank descending
//!           − danger  (max sim to enemy/                  │
//!                      bystander words)                   ▼
//!           − 4 × assassin sim                    guess the top word;
//!           + 0.05 × (cluster size − 1)           after the mandatory
//!     reject outright if assassin sim > 0.30      first guess, pass once
//!   take the best (score, then larger cluster,    the next word's margin
//!   then lexicographic); number = cluster size    drops below 0.30
//! ```
//!
//! **Vector source.** The table is injected, never embedded: the bot takes an
//! `Arc<VectorTable>` so one match run loads one table and shares it across
//! seats. It comes from the optional `codenames.vectors_path` config knob,
//! shaped exactly like `codenames.wordlist_path` and loaded once by
//! [`AgentFactory`](crate::codenames::runner::AgentFactory); with no path
//! configured the game boundary rejects this kind rather than substituting
//! another agent. No pre-trained asset is vendored yet — tests drive it from
//! `crates/engine/fixtures/codenames_test_vectors.txt`.

use super::vectors::VectorTable;
use super::{AgentError, AgentReply, SeatAgent};
use crate::codenames::actions::{Action, DecisionPoint};
use crate::codenames::observation::Observation;
use crate::codenames::types::{CardIdentity, Clue, Role};
use async_trait::async_trait;
use std::sync::Arc;

/// Score differences below this are ties, resolved by the explicit tie-break
/// rules rather than by float noise.
const EPS: f32 = 1e-6;

/// The margin-scoring knobs. `Default` is the anchor configuration; PRD §9
/// item 12 lists the pass margin as tune-later, so it lives in a struct rather
/// than as scattered literals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmbeddingTuning {
    /// Weight on the closest enemy-agent/bystander word (the "danger").
    pub danger_weight: f32,
    /// Weight on assassin similarity for candidates that survive the reject.
    pub assassin_weight: f32,
    /// Any candidate closer than this to the assassin is dropped outright.
    pub assassin_reject: f32,
    /// Per-extra-cluster-word bonus: breaks near-ties toward bigger clues.
    pub cluster_bonus: f32,
    /// An own word below this similarity never joins the cluster.
    pub min_cluster_sim: f32,
    /// The operative's pass threshold after the mandatory first guess.
    pub continue_min_sim: f32,
}

impl Default for EmbeddingTuning {
    fn default() -> Self {
        Self {
            danger_weight: 1.0,
            assassin_weight: 4.0,
            assassin_reject: 0.30,
            cluster_bonus: 0.05,
            min_cluster_sim: 0.15,
            continue_min_sim: 0.30,
        }
    }
}

/// The embedding anchor. Cheap to clone per seat: the table is shared.
pub struct EmbeddingGreedyBot {
    vectors: Arc<VectorTable>,
    tuning: EmbeddingTuning,
}

/// One scored (clue word, cluster size) pair.
#[derive(Debug, Clone)]
struct Scored {
    word: String,
    number: u8,
    score: f32,
}

/// The face-down board split by identity, restricted to words the table can
/// actually score. Words without vectors are invisible to the search — for own
/// agents that costs a cluster slot, for enemies it is a blind spot, and both
/// shrink to nothing once the real vector subset covers the wordlist.
#[derive(Default)]
struct BoardView<'a> {
    own: Vec<&'a str>,
    others: Vec<&'a str>,
    assassins: Vec<&'a str>,
}

impl EmbeddingGreedyBot {
    pub fn new(vectors: Arc<VectorTable>) -> Self {
        Self {
            vectors,
            tuning: EmbeddingTuning::default(),
        }
    }

    pub fn with_tuning(vectors: Arc<VectorTable>, tuning: EmbeddingTuning) -> Self {
        Self { vectors, tuning }
    }

    pub fn tuning(&self) -> EmbeddingTuning {
        self.tuning
    }

    /// Split the face-down grid by key-card identity. An observation without a
    /// key (an operative's) yields empty buckets, which degrades the search to
    /// "the lexicographically first legal candidate" rather than panicking.
    fn board_view<'a>(&self, obs: &'a Observation) -> BoardView<'a> {
        let mut view = BoardView::default();
        let Some(key) = obs.key.as_ref() else {
            return view;
        };
        for (card, identity) in obs.grid.iter().zip(&key.identities) {
            if card.revealed || !self.vectors.contains(&card.word) {
                continue;
            }
            match identity {
                CardIdentity::Agent { team } if *team == obs.team => view.own.push(&card.word),
                CardIdentity::Assassin => view.assassins.push(&card.word),
                _ => view.others.push(&card.word),
            }
        }
        view
    }

    /// The best (clue, number) this board affords. Two passes: the assassin
    /// reject is a hard filter first, and only if it empties the field does the
    /// bot fall back to the weighted penalty — a legal clue must always exist.
    fn give_clue(&self, obs: &Observation, max_number: u8) -> Action {
        let view = self.board_view(obs);
        let max_number = max_number.max(1) as usize;
        let best = self
            .search(obs, &view, max_number, true)
            .or_else(|| self.search(obs, &view, max_number, false));

        match best {
            Some(best) => Action::GiveClue {
                word: best.word,
                number: best.number,
            },
            // Unreachable with any real table: it would need every candidate to
            // collide with a face-down word. Mirrors the engine's own forced
            // default so the bot still cannot emit an illegal clue.
            None => Action::GiveClue {
                word: synthetic_clue(obs),
                number: 1,
            },
        }
    }

    /// Sweep the candidate vocabulary in lexicographic order, keeping the best
    /// scoring (candidate, cluster size).
    fn search(
        &self,
        obs: &Observation,
        view: &BoardView<'_>,
        max_number: usize,
        reject_assassin: bool,
    ) -> Option<Scored> {
        let mut best: Option<Scored> = None;
        for candidate in self.vectors.words() {
            if !clue_word_is_legal(obs, candidate) {
                continue;
            }
            let Some(scored) = self.score(candidate, view, max_number, reject_assassin) else {
                continue;
            };
            if best.as_ref().is_none_or(|old| is_better(&scored, old)) {
                best = Some(scored);
            }
        }
        best
    }

    /// Score one candidate against every cluster size it supports, returning
    /// its best. `None` means the assassin reject fired.
    fn score(
        &self,
        candidate: &str,
        view: &BoardView<'_>,
        max_number: usize,
        reject_assassin: bool,
    ) -> Option<Scored> {
        let assassin = self.closest(candidate, &view.assassins);
        if reject_assassin && assassin > self.tuning.assassin_reject {
            return None;
        }
        let danger = self.closest(candidate, &view.others);
        let penalty = self.tuning.danger_weight * danger.max(0.0)
            + self.tuning.assassin_weight * assassin.max(0.0);

        // Greedy cluster growth: own agents by descending similarity, ties
        // lexicographic, cut at the floor and at the legal clue-number ceiling.
        let mut sims: Vec<(f32, &str)> = view
            .own
            .iter()
            .map(|w| (self.similarity(candidate, w), *w))
            .collect();
        sims.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let usable = sims
            .iter()
            .take_while(|(sim, _)| *sim >= self.tuning.min_cluster_sim)
            .count()
            .min(max_number);

        let mut best: Option<Scored> = None;
        for k in 1..=usable.max(1) {
            // With `usable == 0` this is the top own word anyway (below the
            // floor) — a weak but legal clue for a board this table cannot read.
            let min_sim = sims.get(k - 1).map(|(sim, _)| *sim).unwrap_or(0.0);
            let scored = Scored {
                word: candidate.to_string(),
                number: k as u8,
                score: min_sim - penalty + self.tuning.cluster_bonus * (k as f32 - 1.0),
            };
            if best.as_ref().is_none_or(|old| is_better(&scored, old)) {
                best = Some(scored);
            }
        }
        best
    }

    /// One guess, or a pass. The first guess of a clue is mandatory, so it is
    /// made even when the table cannot read the clue at all.
    fn guess_or_pass(&self, obs: &Observation, clue: &Clue, guesses_used: u8) -> Action {
        let face_down = || {
            let mut words: Vec<&str> = obs
                .grid
                .iter()
                .filter(|c| !c.revealed)
                .map(|c| c.word.as_str())
                .collect();
            words.sort_unstable();
            words.first().map(|w| w.to_string())
        };
        let mandatory = guesses_used == 0;

        // The clue's own number is the contract; the free bonus guess is
        // declined, because taking it means guessing past the last word the
        // spymaster vouched for.
        if !mandatory && u32::from(guesses_used) >= u32::from(clue.number) {
            return Action::Pass;
        }
        let Some(clue_vec) = self.vectors.get(&clue.word) else {
            return match (mandatory, face_down()) {
                (true, Some(word)) => Action::Guess { word },
                _ => Action::Pass,
            };
        };

        let mut ranked: Vec<(f32, &str)> = obs
            .grid
            .iter()
            .filter(|c| !c.revealed)
            .filter_map(|c| {
                let vec = self.vectors.get(&c.word)?;
                Some((super::vectors::cosine(clue_vec, vec), c.word.as_str()))
            })
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));

        match ranked.first() {
            Some((sim, word)) if mandatory || *sim >= self.tuning.continue_min_sim => {
                Action::Guess {
                    word: (*word).to_string(),
                }
            }
            Some(_) => Action::Pass,
            None => match (mandatory, face_down()) {
                (true, Some(word)) => Action::Guess { word },
                _ => Action::Pass,
            },
        }
    }

    /// Cosine to `word`, or `0.0` ("unrelated") when either side is untabled.
    fn similarity(&self, candidate: &str, word: &str) -> f32 {
        self.vectors.similarity(candidate, word).unwrap_or(0.0)
    }

    /// The closest of `words` to the candidate; `0.0` for an empty bucket, so
    /// a board with nothing left to fear is penalized by nothing.
    fn closest(&self, candidate: &str, words: &[&str]) -> f32 {
        words
            .iter()
            .map(|w| self.similarity(candidate, w))
            .fold(0.0f32, f32::max)
    }
}

/// Total order over candidates: higher score, then the bigger clue number,
/// then lexicographic — no float NaN, no iteration-order dependence.
fn is_better(new: &Scored, old: &Scored) -> bool {
    if new.score > old.score + EPS {
        return true;
    }
    if old.score > new.score + EPS {
        return false;
    }
    if new.number != old.number {
        return new.number > old.number;
    }
    new.word < old.word
}

/// The clue rule as an agent can check it from its own observation — the same
/// filter `RandomLegalBot` applies, and a mirror of `transitions::clue`, which
/// remains the sole authority. The length cap is read off the observation, so
/// a game configured tighter than the default narrows the candidate set
/// instead of producing illegal clues.
fn clue_word_is_legal(obs: &Observation, word: &str) -> bool {
    let shape_ok = word.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
        && word.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && word.chars().last().is_some_and(|c| c.is_ascii_alphabetic())
        && word.chars().count() <= obs.clue_word_max_len;
    if !shape_ok {
        return false;
    }
    let clue = word.to_ascii_lowercase();
    !obs.grid
        .iter()
        .any(|c| !c.revealed && (c.word.contains(&clue) || clue.contains(c.word.as_str())))
}

/// The engine's own last-resort clue shape, for a table that offers nothing
/// legal.
fn synthetic_clue(obs: &Observation) -> String {
    let longest = obs.clue_word_max_len.max(2);
    (2..=longest)
        .map(|n| "z".repeat(n))
        .find(|w| clue_word_is_legal(obs, w))
        .unwrap_or_else(|| "z".repeat(longest))
}

#[async_trait]
impl SeatAgent for EmbeddingGreedyBot {
    fn kind(&self) -> &'static str {
        "bot:codenames-embedding"
    }
    fn model_id(&self) -> String {
        "bot:codenames-embedding".into()
    }
    fn scaffold_version(&self) -> String {
        "bot-embedding-v1".into()
    }

    async fn decide(
        &mut self,
        obs: &Observation,
        decision: &DecisionPoint,
        _feedback: Option<&str>,
    ) -> Result<AgentReply, AgentError> {
        let action = match decision {
            DecisionPoint::GiveClue { max_number, .. } => {
                debug_assert_eq!(obs.role, Role::Spymaster);
                self.give_clue(obs, *max_number)
            }
            DecisionPoint::GuessOrPass {
                clue, guesses_used, ..
            } => self.guess_or_pass(obs, clue, *guesses_used),
        };
        Ok(AgentReply {
            action,
            thought: None,
        })
    }
}
