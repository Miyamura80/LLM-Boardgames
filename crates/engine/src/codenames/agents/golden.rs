//! The scaffold hash and the frozen fixtures it renders.
//!
//! [`scaffold_version`] hashes every model-visible prompt constant plus a
//! golden render of every prompt-shaping function, so any wording edit — in a
//! constant, a brief, an ask, a schema, or the observation renderer — changes
//! the scaffold id and rating attribution cannot silently drift.
//!
//! The golden state is built on a pool frozen *here* rather than on the
//! vendored wordlist: curating `assets/codenames_words.txt` changes the board
//! draw, and a wordlist edit must not invalidate every recorded scaffold (the
//! pool is already pinned per game by `wordlist_hash`).

use super::prompts;
use crate::codenames::actions::{Action, DecisionPoint};
use crate::codenames::state::GameState;
use crate::codenames::types::{
    CardIdentity, Clue, GameConfig, Team, SEAT_A_OPERATIVE, SEAT_A_SPYMASTER, SEAT_B_SPYMASTER,
};
use crate::codenames::wordlist::Wordlist;
use sha2::{Digest, Sha256};

/// 30 words, none a substring of another, frozen for the golden render.
const GOLDEN_WORDS: &str = "\
anchor\nbanjo\ncactus\ndragon\nengine\nfossil\nglacier\nharbor\niceberg\njungle\n\
kettle\nlantern\nmammoth\nnebula\noyster\npyramid\nquarry\nrocket\nsaddle\ntunnel\n\
umbrella\nvolcano\nwalnut\nxylophone\nyogurt\nzebra\nbeacon\ncompass\ndomino\nfeather\n";

/// A fixed mid-game position: one resolved clue with a correct guess behind it
/// and a live clue in front, so every branch of the renderer has something to
/// show (revealed cards, outcomes, a live guess count).
fn golden_state() -> GameState {
    let wordlist = Wordlist::parse(GOLDEN_WORDS).expect("golden pool is valid");
    let mut state = GameState::with_wordlist(0xC0DE, wordlist, GameConfig::default());
    // Neither clue word is in the golden pool, nor a sub/superstring of any
    // word in it, so both are legal on every draw from it.
    let first_agent = state
        .board
        .cards
        .iter()
        .find(|c| c.identity == CardIdentity::Agent { team: Team::A })
        .map(|c| c.word.clone())
        .expect("team a always holds nine agents");
    let scripted = [
        Action::GiveClue {
            word: "signal".into(),
            number: 2,
        },
        // A correct guess: the turn survives it, so the pass below is legal.
        Action::Guess { word: first_agent },
        Action::Pass,
        Action::GiveClue {
            word: "midnight".into(),
            number: 1,
        },
    ];
    for action in scripted {
        let decision = state.pending_decisions().remove(0);
        state
            .apply(decision.seat(), action)
            .expect("the golden script is legal by construction");
    }
    state
}

/// Every decision variant, with concrete values, so both asks and both schemas
/// are hashed.
fn golden_decisions() -> Vec<DecisionPoint> {
    vec![
        DecisionPoint::GiveClue {
            seat: SEAT_A_SPYMASTER,
            team: Team::A,
            max_number: 9,
        },
        DecisionPoint::GuessOrPass {
            seat: SEAT_A_OPERATIVE,
            team: Team::A,
            clue: Clue {
                word: "signal".into(),
                number: 2,
            },
            guesses_used: 0,
            guesses_remaining: 3,
        },
        DecisionPoint::GuessOrPass {
            seat: SEAT_A_OPERATIVE,
            team: Team::A,
            clue: Clue {
                word: "signal".into(),
                number: 2,
            },
            guesses_used: 1,
            guesses_remaining: 2,
        },
    ]
}

/// A fixed, deterministic render of every prompt-shaping function — any
/// wording edit anywhere changes this string, hence the scaffold id.
fn golden_prompt_render() -> String {
    let state = golden_state();
    let mut s = String::new();
    // Both roles: the spymaster render carries the key-card section, the
    // operative render does not.
    for seat in [SEAT_A_SPYMASTER, SEAT_B_SPYMASTER, SEAT_A_OPERATIVE] {
        let obs = state.observe(seat);
        s.push_str(&prompts::role_brief(&obs));
        s.push_str(&prompts::render_observation(&obs));
        s.push_str(&prompts::clue_rules(&obs));
        for decision in &golden_decisions() {
            s.push_str(&prompts::decision_ask(&obs, decision));
            s.push_str(prompts::decision_schema(decision));
        }
    }
    s
}

/// Version hash over every model-visible prompt surface + persona +
/// temperature. Any wording edit anywhere invalidates the scaffold id so
/// rating attribution can't silently drift.
pub fn scaffold_version(persona: Option<&str>, temperature: f32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompts::PROMPT_REVISION);
    hasher.update(prompts::RULES_SUMMARY);
    hasher.update(prompts::OUTPUT_CONTRACT);
    hasher.update(prompts::SPYMASTER_BRIEF);
    hasher.update(prompts::OPERATIVE_BRIEF);
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
        assert_ne!(a, scaffold_version(Some("cautious"), 0.5));
        assert_ne!(a, scaffold_version(None, 0.7));
        assert!(a.starts_with("sc-"));
        assert_eq!(a.len(), 11);
    }

    /// The golden position must actually exercise every render branch, or a
    /// wording edit in an unrendered branch would slip past the hash.
    #[test]
    fn the_golden_render_covers_every_prompt_surface() {
        let render = golden_prompt_render();
        for needle in [
            "== KEY CARD (SPYMASTER ONLY",
            "ASSASSIN:",
            "REVEALED:",
            "== CLUE HISTORY ==",
            "the operative passed",
            "LIVE,",
            "SPYMASTER",
            "OPERATIVE",
            "give_clue",
            "\"action\": \"pass\"",
            "mandatory",
        ] {
            assert!(
                render.contains(needle),
                "golden render is missing {needle:?}"
            );
        }
    }
}
