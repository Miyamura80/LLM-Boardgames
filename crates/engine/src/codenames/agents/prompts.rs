//! Frozen prompt templates for Codenames LLM seats: rules digest, per-role
//! brief, output contract, the compact observation render, and the per-decision
//! ask + strict JSON schema.
//!
//! Codenames observations are tiny (25 words, a short clue log), so everything
//! is rendered in full every turn — no delta machinery, unlike Catan.
//!
//! The rated unit is *model + scaffold*: [`super::golden::scaffold_version`]
//! hashes every constant and a golden render of every function here, so a
//! wording edit anywhere moves the scaffold id.

use crate::codenames::actions::DecisionPoint;
use crate::codenames::observation::{CardView, ClueView, Observation};
use crate::codenames::types::{CardIdentity, Role};

/// Manual override lever. The golden render covers every function and constant
/// below automatically, so this only needs bumping to *intentionally*
/// invalidate scaffolds for a reason the rendered text cannot see (e.g. a
/// decoding or sampling change).
pub const PROMPT_REVISION: &str = "codenames-prompts-v1";

pub const RULES_SUMMARY: &str = "\
You are playing Codenames (standard 2-team game, 4 seats: one spymaster and one operative per team).
Board: 25 word cards face up in a 5x5 grid. A hidden key card assigns every card an identity: 9 agents to team a (which moves first), 8 agents to team b, 7 innocent bystanders, and 1 assassin. Only the two spymasters see the key.
Turn: the active team's spymaster gives a clue — ONE word plus a number. The number says how many face-down cards on the board the clue points at.
Then that team's operative guesses one board word at a time. The card is turned face up:
  - own agent  -> correct: the turn continues (up to number + 1 guesses in total for this clue);
  - bystander  -> the turn ends immediately;
  - opposing agent -> it counts for the OTHER team, and the turn ends immediately;
  - assassin   -> the guessing team loses the game on the spot.
The first guess under a clue is mandatory. After it, the operative may pass instead of guessing, ending the turn.
Winning: a team wins when all of its agents are revealed (even if the opponent flips the last one by mistake), or when the opponent reveals the assassin.";

pub const OUTPUT_CONTRACT: &str = "\
Respond with a SINGLE strict JSON object and nothing else. No markdown fences, no prose outside the JSON.
The object must match the requested schema exactly — unknown fields are rejected.
You may include a \"thought_process\" string field with your private reasoning; it is recorded for review but is never shown to any other seat, so it cannot be used to signal your partner.
The engine is the sole authority on legality: an illegal clue or guess is rejected verbatim and you are asked again, which counts against you.";

pub const SPYMASTER_BRIEF: &str = "\
You are the SPYMASTER. You alone see the key card printed below. Your clue is the only channel you have: your operative sees the same grid but no identities, and you may say nothing else — no gestures, no extra words, no hints about position.
A good clue links several of your own face-down agents at once while staying far away from the assassin, from the opposing team's agents, and from bystanders. A greedy number that drags your operative onto the assassin loses the game outright; a safe 1 that is unmistakable is often worth more than a risky 3.";

pub const OPERATIVE_BRIEF: &str = "\
You are the OPERATIVE. You do NOT see the key card: you only know the words, which cards are already revealed, and the clues your spymaster has given.
Guess the face-down word your spymaster's clue most plausibly points at, and use the clue history — including the opposing team's clues and mistakes — as evidence. Every correct guess buys another; a bystander or an opposing agent ends your turn, and the assassin ends the game. Once you have made the mandatory first guess, passing to keep a wrong reveal off the board is a legitimate move.";

/// Seat identity + role brief for the system prompt.
pub fn role_brief(obs: &Observation) -> String {
    let mut s = format!(
        "You are seat {} playing for team {}, the {} of your team. Your team wins or loses together; \
play to win.\n\n",
        obs.seat,
        obs.team.as_str(),
        obs.role.as_str(),
    );
    s.push_str(match obs.role {
        Role::Spymaster => SPYMASTER_BRIEF,
        Role::Operative => OPERATIVE_BRIEF,
    });
    s
}

/// The clue legality rules, with the game's effective knobs filled in. Spelled
/// out for the model because the engine rejects violations verbatim.
pub fn clue_rules(obs: &Observation) -> String {
    format!(
        "A legal clue word is:\n\
         - ONE word: letters only, hyphens allowed inside the word, no spaces, digits, or punctuation;\n\
         - at most {} characters long;\n\
         - case-insensitive, and NOT equal to, contained in, or containing any FACE-DOWN board word \
         (\"{}\" would be illegal while \"{}\" is face down). Words already revealed no longer \
         constrain your clues.\n\
         The number must be between 1 and the count of your own agents still face down.",
        obs.clue_word_max_len,
        sample_illegal_clue(obs),
        sample_face_down_word(obs),
    )
}

/// A face-down word to name in the legality example (grid order, so the
/// example is deterministic).
fn sample_face_down_word(obs: &Observation) -> &str {
    obs.grid
        .iter()
        .find(|c| !c.revealed)
        .map(|c| c.word.as_str())
        .unwrap_or("ocean")
}

/// A concrete illegal clue for this board: the face-down word plus a letter,
/// which collides by superstring.
fn sample_illegal_clue(obs: &Observation) -> String {
    format!("{}s", sample_face_down_word(obs))
}

/// The full observation render: grid, key card (spymasters only), clue log,
/// score, and whose turn it is.
pub fn render_observation(obs: &Observation) -> String {
    let mut s = String::with_capacity(2048);
    s.push_str("== BOARD (5 rows x 5 columns, row-major) ==\n");
    for card in &obs.grid {
        s.push_str(&format!(
            "  r{}c{} {:<14} {}\n",
            card.row,
            card.col,
            card.word,
            card_status(card)
        ));
    }
    s.push_str(&format!(
        "Face-down words still guessable: {}\n",
        join_words(obs.grid.iter().filter(|c| !c.revealed))
    ));

    if let Some(key) = &obs.key {
        s.push_str("\n== KEY CARD (SPYMASTER ONLY — never name these words aloud) ==\n");
        let face_down = |want: CardIdentity| {
            join_words(
                obs.grid
                    .iter()
                    .zip(&key.identities)
                    .filter(|(c, id)| !c.revealed && **id == want)
                    .map(|(c, _)| c),
            )
        };
        s.push_str(&format!(
            "  YOUR agents still face down (team {}): {}\n",
            obs.team.as_str(),
            face_down(CardIdentity::Agent { team: obs.team })
        ));
        s.push_str(&format!(
            "  OPPOSING agents still face down (team {}): {}\n",
            obs.team.other().as_str(),
            face_down(CardIdentity::Agent {
                team: obs.team.other()
            })
        ));
        s.push_str(&format!(
            "  Bystanders still face down: {}\n",
            face_down(CardIdentity::Bystander)
        ));
        s.push_str(&format!(
            "  ASSASSIN: {} — if your operative guesses it, your team loses instantly.\n",
            face_down(CardIdentity::Assassin)
        ));
    }

    s.push_str("\n== CLUE HISTORY ==\n");
    if obs.clues.is_empty() {
        s.push_str("  (no clues yet)\n");
    }
    for clue in &obs.clues {
        s.push_str(&format!("  {}\n", render_clue(clue)));
    }

    s.push_str("\n== SCORE ==\n");
    for score in &obs.scores {
        s.push_str(&format!(
            "  team {}: {}/{} agents found, {} still face down{}\n",
            score.team.as_str(),
            score.agents_revealed,
            score.agents_total,
            score.agents_remaining,
            if score.team == obs.team {
                " (yours)"
            } else {
                ""
            },
        ));
    }
    s.push_str(&format!(
        "Turn {}: team {} is acting ({} phase).\n",
        obs.turn,
        obs.active_team.as_str(),
        obs.phase,
    ));
    s
}

fn card_status(card: &CardView) -> String {
    match card.identity {
        Some(identity) => format!("REVEALED: {}", identity.as_str()),
        None => "face down".into(),
    }
}

fn join_words<'a>(cards: impl Iterator<Item = &'a CardView>) -> String {
    let words: Vec<&str> = cards.map(|c| c.word.as_str()).collect();
    if words.is_empty() {
        "none".into()
    } else {
        words.join(", ")
    }
}

/// One clue plus what it produced, oldest first.
fn render_clue(clue: &ClueView) -> String {
    let mut s = format!(
        "turn {} team {}: \"{}\" {}",
        clue.turn,
        clue.team.as_str(),
        clue.word,
        clue.number
    );
    if clue.outcomes.is_empty() {
        s.push_str(" -> no guesses yet");
    } else {
        let outcomes: Vec<String> = clue
            .outcomes
            .iter()
            .map(|o| format!("{} ({})", o.word, o.identity.as_str()))
            .collect();
        s.push_str(&format!(" -> {}", outcomes.join(", ")));
    }
    if clue.passed {
        s.push_str("; the operative passed");
    }
    if let Some(remaining) = clue.guesses_remaining {
        s.push_str(&format!("; LIVE, {remaining} guess(es) still allowed"));
    }
    s
}

/// The natural-language "what to decide now" line. Model-visible: wording
/// changes must move the scaffold id (the golden render hashes this).
pub fn decision_ask(obs: &Observation, decision: &DecisionPoint) -> String {
    match decision {
        DecisionPoint::GiveClue { max_number, .. } => format!(
            "Give your operative a clue: one word plus a number from 1 to {max_number} (you have \
             {max_number} agent(s) still face down). Your operative may make up to number + 1 \
             guesses under it.\n{}",
            clue_rules(obs)
        ),
        DecisionPoint::GuessOrPass {
            clue,
            guesses_used,
            guesses_remaining,
            ..
        } => {
            let mut s = format!(
                "Your spymaster's clue is \"{}\" {}. You have made {guesses_used} guess(es) under \
                 it and may make up to {guesses_remaining} more.",
                clue.word, clue.number
            );
            if *guesses_used == 0 {
                s.push_str(" The first guess under a clue is mandatory — you may not pass yet.");
            } else {
                s.push_str(" You may guess again, or pass to end your team's turn now.");
            }
            s.push_str(&format!(
                "\nGuess exactly one of the face-down words: {}.",
                join_words(obs.grid.iter().filter(|c| !c.revealed))
            ));
            s
        }
    }
}

/// The strict JSON shape for each decision, shown verbatim to the model.
pub fn decision_schema(decision: &DecisionPoint) -> &'static str {
    match decision {
        DecisionPoint::GiveClue { .. } => {
            r#"{"action": "give_clue", "word": "<single clue word>", "number": <integer >= 1>, "thought_process": "<private reasoning>"}"#
        }
        DecisionPoint::GuessOrPass { .. } => {
            r#"One of:
{"action": "guess", "word": "<one face-down board word, exactly as printed>", "thought_process": "<private reasoning>"}
{"action": "pass", "thought_process": "<private reasoning>"}
("pass" is legal only after the mandatory first guess of the current clue.)"#
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::testkit;
    use crate::codenames::types::{SEAT_A_OPERATIVE, SEAT_A_SPYMASTER};

    #[test]
    fn spymaster_render_carries_the_key_and_the_operative_one_does_not() {
        let mut state = testkit::scripted_game(5);
        testkit::give_clue(&mut state, 2);

        let spymaster = render_observation(&state.observe(SEAT_A_SPYMASTER));
        assert!(spymaster.contains("== KEY CARD (SPYMASTER ONLY"));
        assert!(spymaster.contains("ASSASSIN:"));
        assert!(spymaster.contains("== BOARD (5 rows x 5 columns, row-major) =="));
        assert!(spymaster.contains("== SCORE =="));

        let operative = render_observation(&state.observe(SEAT_A_OPERATIVE));
        assert!(!operative.contains("KEY CARD"));
        assert!(!operative.contains("ASSASSIN"));
        assert!(operative.contains("LIVE, 3 guess(es) still allowed"));
    }

    /// The configured cap — not the default — must reach the model.
    #[test]
    fn clue_rules_quote_the_effective_length_cap() {
        let mut state = testkit::scripted_game(5);
        state.config.clue_word_max_len = 7;
        let obs = state.observe(SEAT_A_SPYMASTER);
        let rules = clue_rules(&obs);
        assert!(rules.contains("at most 7 characters"), "{rules}");
        // The worked example names a real face-down word from this board.
        let face_down = sample_face_down_word(&obs).to_string();
        assert!(rules.contains(&format!("\"{face_down}s\"")), "{rules}");
    }

    #[test]
    fn the_guess_ask_marks_the_mandatory_first_guess() {
        let mut state = testkit::scripted_game(5);
        testkit::give_clue(&mut state, 2);
        let obs = state.observe(SEAT_A_OPERATIVE);
        let decision = state.pending_decisions().remove(0);
        let ask = decision_ask(&obs, &decision);
        assert!(ask.contains("mandatory"), "{ask}");
        assert!(ask.contains("up to 3 more"), "{ask}");
        assert!(decision_schema(&decision).contains("\"action\": \"guess\""));
    }

    #[test]
    fn the_clue_ask_bounds_the_number_by_remaining_agents() {
        let state = testkit::scripted_game(5);
        let obs = state.observe(SEAT_A_SPYMASTER);
        let decision = state.pending_decisions().remove(0);
        let ask = decision_ask(&obs, &decision);
        assert!(ask.contains("a number from 1 to 9"), "{ask}");
        assert!(decision_schema(&decision).contains("give_clue"));
    }
}
