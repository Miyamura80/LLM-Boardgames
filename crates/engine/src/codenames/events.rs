//! The Codenames transcript. Every event is `Public`: the only hidden
//! information in the game is the key card, and that is *state* derived into
//! spymaster observations (see [`super::observation`]), never an event — so
//! `Visibility` needs no team variant (PRD-codenames-evals §9 decision 8).

use super::types::{CardIdentity, Clue, EndReason, Seat, Team};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::game_core::Visibility;

/// One transcript entry (the shared envelope carrying a Codenames payload).
pub type EventRecord = crate::game_core::EventRecord<CodenamesEvent>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum CodenamesEvent {
    GameStarted {
        /// Always team A — the nine-agent side.
        starting_team: Team,
        /// SHA-256 of the pool the grid was drawn from, for record
        /// comparability.
        wordlist_hash: String,
        rules_version: String,
    },
    /// The 25 words in grid order, emitted once at setup. Public: the words
    /// are face-up from the start; only their identities are secret.
    BoardLaid {
        words: Vec<String>,
    },
    TurnStarted {
        team: Team,
        turn: u32,
    },
    ClueGiven {
        seat: Seat,
        team: Team,
        clue: Clue,
    },
    /// One card flipped. Carries the revealed identity (now public) and
    /// whether the reveal ended the guessing team's turn.
    GuessRevealed {
        seat: Seat,
        team: Team,
        word: String,
        identity: CardIdentity,
        ends_turn: bool,
    },
    /// The operative declined a further guess (legal only after the mandatory
    /// first guess).
    TurnPassed {
        seat: Seat,
        team: Team,
    },
    /// An agent exhausted its retry budget; the engine applied the
    /// deterministic legal default (metric-exempt, public).
    ForcedDefault {
        seat: Seat,
        decision: String,
    },
    GameEnded {
        winner: Team,
        reason: EndReason,
        turns: u32,
    },
}

impl CodenamesEvent {
    /// One neutral line per event, for replays and reports. Every Codenames
    /// event is public, so a single wording serves every reader.
    pub fn render(&self) -> String {
        use CodenamesEvent::*;
        let team = |t: &Team| format!("Team {}", t.as_str().to_uppercase());
        match self {
            GameStarted { starting_team, .. } => {
                format!(
                    "A game of Codenames begins; {} starts.",
                    team(starting_team)
                )
            }
            BoardLaid { words } => format!("The grid is laid out: {}.", words.join(", ")),
            TurnStarted { team: t, turn } => format!("— Turn {turn}: {} —", team(t)),
            ClueGiven { team: t, clue, .. } => format!(
                "{}'s spymaster gives the clue \"{}\" {}.",
                team(t),
                clue.word,
                clue.number
            ),
            GuessRevealed {
                team: t,
                word,
                identity,
                ends_turn,
                ..
            } => format!(
                "{}'s operative guesses {word} — {}{}",
                team(t),
                identity.as_str(),
                if *ends_turn { "; the turn ends." } else { "." }
            ),
            TurnPassed { team: t, .. } => format!("{}'s operative passes.", team(t)),
            ForcedDefault { seat, decision } => {
                format!("Seat {seat} ran out of attempts; the engine applied the forced legal default for {decision}.")
            }
            GameEnded {
                winner,
                reason,
                turns,
            } => format!(
                "{} wins after {turns} turn(s) ({}).",
                team(winner),
                match reason {
                    EndReason::AgentsFound => "all agents found",
                    EndReason::Assassin => "the opponent revealed the assassin",
                }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::types::Clue;

    #[test]
    fn every_event_renders_a_readable_line() {
        let lines = [
            CodenamesEvent::GameStarted {
                starting_team: Team::A,
                wordlist_hash: "0".repeat(64),
                rules_version: "codenames-v1".into(),
            },
            CodenamesEvent::BoardLaid {
                words: vec!["alfa".into(), "bravo".into()],
            },
            CodenamesEvent::ClueGiven {
                seat: 0,
                team: Team::A,
                clue: Clue {
                    word: "signal".into(),
                    number: 2,
                },
            },
            CodenamesEvent::GuessRevealed {
                seat: 1,
                team: Team::A,
                word: "alfa".into(),
                identity: CardIdentity::Assassin,
                ends_turn: true,
            },
            CodenamesEvent::TurnPassed {
                seat: 1,
                team: Team::A,
            },
            CodenamesEvent::GameEnded {
                winner: Team::B,
                reason: EndReason::Assassin,
                turns: 4,
            },
        ]
        .map(|e| e.render());
        assert!(lines.iter().all(|l| !l.is_empty() && l.ends_with('.')));
        assert!(lines[2].contains("\"signal\" 2"), "{}", lines[2]);
        assert!(lines[3].contains("assassin"), "{}", lines[3]);
    }
}
