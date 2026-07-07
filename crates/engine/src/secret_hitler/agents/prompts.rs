//! Frozen prompt templates for LLM seats. The rated unit is *model +
//! scaffold*: [`scaffold_version`] hashes every template plus the persona and
//! temperature, and is recorded on each seat of each game.

use crate::secret_hitler::actions::DecisionPoint;
use crate::secret_hitler::observation::Observation;
use crate::secret_hitler::types::Role;
use sha2::{Digest, Sha256};

pub const RULES_SUMMARY: &str = r#"You are playing Secret Hitler, a hidden-role social deduction game, 7-player ruleset.
- Roles: 4 Liberals, 2 Fascists, 1 Hitler (Hitler is on the Fascist team).
- The two regular Fascists know each other and know who Hitler is. Hitler does NOT know who the Fascists are. Liberals know nothing.
- Each round: the President nominates a Chancellor; everyone votes Ja/Nein. On a majority Ja, the President draws 3 policy tiles, discards 1 secretly, and passes 2 to the Chancellor, who enacts 1 secretly.
- Liberals win when 5 Liberal policies are enacted or Hitler is executed.
- Fascists win when 6 Fascist policies are enacted or Hitler is elected Chancellor after 3+ Fascist policies are on the board.
- Failed votes advance the election tracker; at 3 the top policy is auto-enacted with no power and term limits reset.
- The last elected President and Chancellor are term-limited as Chancellor candidates (only the last Chancellor once 5 or fewer players remain).
- Fascist board powers (7-player): 2nd Fascist policy -> the President investigates one player's party membership (Hitler's card reads Fascist); 3rd -> Special Election (President picks the next Presidential candidate); 4th and 5th -> Execution. After the 5th Fascist policy the Chancellor may propose a Veto: if the President agrees, both tiles are discarded and the tracker advances.
- The policy deck holds 6 Liberal and 11 Fascist tiles; claims about drawn tiles cannot be verified, so players may lie."#;

/// Role brief appended to the system prompt.
pub fn role_brief(obs: &Observation) -> String {
    let mut s = format!(
        "You are Player {} and your secret role is {}.",
        obs.seat,
        obs.role.as_str()
    );
    match obs.role {
        Role::Liberal => s.push_str(
            " Win by enacting Liberal policies and identifying the Fascists. Trust no claim blindly.",
        ),
        Role::Fascist => {
            s.push_str(" Win by enacting Fascist policies or getting Hitler elected Chancellor after 3 Fascist policies. Lie, deflect suspicion, protect Hitler.");
            for (seat, role) in &obs.known_teammates {
                s.push_str(&format!(" Player {seat} is {}.", role.as_str()));
            }
        }
        Role::Hitler => s.push_str(
            " You do NOT know who your Fascist teammates are. Appear as Liberal as possible; if 3+ Fascist policies are enacted and you are elected Chancellor, your team wins instantly.",
        ),
    }
    s
}

pub const OUTPUT_CONTRACT: &str = "Respond with a single strict JSON object and nothing else: no markdown fences, no commentary outside the JSON. Unknown fields are rejected.";

/// The strict JSON schema text shown to the model for each decision kind.
pub fn decision_schema(decision: &DecisionPoint) -> String {
    match decision {
        DecisionPoint::Nominate { eligible, .. } => format!(
            r#"{{"action":"nominate","target":<seat number, one of {eligible:?}>,"thought_process":"<private reasoning>"}}"#
        ),
        DecisionPoint::Vote { nominee, .. } => format!(
            r#"{{"action":"vote","ja":<true to elect Player {nominee} as Chancellor, false to reject>,"thought_process":"<private reasoning>"}}"#
        ),
        DecisionPoint::Discard { tiles, .. } => format!(
            r#"{{"action":"discard","policy":"<"Liberal" or "Fascist"; you hold {tiles:?}; the discarded tile is hidden from everyone>","thought_process":"<private reasoning>"}}"#
        ),
        DecisionPoint::Enact { tiles, can_veto, .. } => {
            let veto = if *can_veto {
                r#" You may instead propose a veto with {"action":"propose_veto","thought_process":"..."}."#
            } else {
                ""
            };
            format!(
                r#"{{"action":"enact","policy":"<"Liberal" or "Fascist"; you hold {tiles:?}>","thought_process":"<private reasoning>"}}{veto}"#
            )
        }
        DecisionPoint::VetoConsent { .. } => r#"{"action":"veto_consent","approve":<true or false>,"thought_process":"<private reasoning>"}"#.to_string(),
        DecisionPoint::UsePower { power, targets, .. } => format!(
            r#"{{"action":"use_power","target":<seat number, one of {targets:?}, for {power:?}>,"thought_process":"<private reasoning>"}}"#
        ),
    }
}

pub const SPEECH_SCHEMA: &str =
    r#"{"message":"<what you say aloud to the table>"} or {"pass":true} to stay silent"#;

pub const BELIEFS_INSTRUCTION: &str = "PRIVATE evaluation (never shown to other players, has no game effect): for every OTHER living player, estimate the probability they are Liberal, a regular Fascist, or Hitler. Respond with strict JSON: {\"beliefs\":{\"<seat>\":{\"liberal\":<0..1>,\"fascist\":<0..1>,\"hitler\":<0..1>},...},\"thought_process\":\"...\"} with each row summing to 1.";

/// Render the seat's full observation as prompt text.
pub fn render_observation(obs: &Observation) -> String {
    let p = &obs.public;
    let mut s = String::with_capacity(4096);
    s.push_str(&format!(
        "== BOARD ==\nRound {} | Liberal policies: {}/5 | Fascist policies: {}/6 | Election tracker: {}/3 | Deck: {} tiles (+{} discarded) | Veto unlocked: {}\nPresident: Player {} | Living players: {:?}\n\n== HISTORY (everything you have witnessed) ==\n",
        p.round,
        p.liberal_policies,
        p.fascist_policies,
        p.election_tracker,
        p.deck_count,
        p.discard_count,
        p.veto_unlocked,
        p.president,
        p.alive,
    ));
    for rec in &obs.history {
        let private = if matches!(
            rec.visibility,
            crate::secret_hitler::events::Visibility::Private(_)
        ) {
            "[private] "
        } else {
            ""
        };
        s.push_str(&format!(
            "r{} {}{}\n",
            rec.round,
            private,
            rec.event.render()
        ));
    }
    s
}

/// Manual override lever. The golden render below already covers
/// `role_brief`, `decision_schema`, and `render_observation` automatically, so
/// this only needs bumping to *intentionally* invalidate scaffolds for a
/// reason the rendered text cannot see (e.g. a decoding/sampling change).
const PROMPT_REVISION: &str = "prompts-v1";

/// A fixed, deterministic render of every model-visible prompt-shaping
/// function. Folding this into the scaffold hash means any edit to
/// `role_brief`, `render_observation`, `decision_schema`, or the event
/// rendering they call changes the scaffold id on its own — no manual
/// `PROMPT_REVISION` bump required, so rating attribution can't silently drift.
fn golden_prompt_render() -> String {
    use crate::secret_hitler::actions::DecisionPoint;
    use crate::secret_hitler::agents::llm_agent::decision_ask;
    use crate::secret_hitler::events::GameEvent;
    use crate::secret_hitler::state::GameState;
    use crate::secret_hitler::types::{Party, Power, WinCondition, PLAYER_COUNT};

    // Fixed arrangement: seats 0-3 Liberal, 4-5 Fascist, 6 Hitler. The seed is
    // constant so the fixture (and thus the digest) is fully deterministic.
    let roles: [Role; PLAYER_COUNT as usize] = [
        Role::Liberal,
        Role::Liberal,
        Role::Liberal,
        Role::Liberal,
        Role::Fascist,
        Role::Fascist,
        Role::Hitler,
    ];
    let state = GameState::with_roles(0xF00D, roles);

    let mut s = String::with_capacity(4096);
    // One seat of each role exercises role_brief (incl. the Fascist teammate
    // block) and render_observation (incl. private-event rendering).
    for seat in [0u8, 4, 6] {
        let obs = state.observe(seat);
        s.push_str(&role_brief(&obs));
        s.push('\n');
        s.push_str(&render_observation(&obs));
        s.push('\n');
    }
    // Every decision variant exercises decision_schema.
    let decisions = [
        DecisionPoint::Nominate {
            president: 0,
            eligible: vec![1, 2, 3],
        },
        DecisionPoint::Vote {
            seat: 1,
            nominee: 2,
        },
        DecisionPoint::Discard {
            president: 0,
            tiles: vec![Party::Liberal, Party::Fascist, Party::Fascist],
        },
        DecisionPoint::Enact {
            chancellor: 2,
            tiles: vec![Party::Liberal, Party::Fascist],
            can_veto: false,
        },
        DecisionPoint::Enact {
            chancellor: 2,
            tiles: vec![Party::Liberal, Party::Fascist],
            can_veto: true,
        },
        DecisionPoint::VetoConsent { president: 0 },
        DecisionPoint::UsePower {
            president: 0,
            power: Power::InvestigateLoyalty,
            targets: vec![1, 2],
        },
        DecisionPoint::UsePower {
            president: 0,
            power: Power::SpecialElection,
            targets: vec![1, 2],
        },
        DecisionPoint::UsePower {
            president: 0,
            power: Power::Execution,
            targets: vec![1, 2],
        },
    ];
    for d in &decisions {
        // Both the schema AND the natural-language ask are shown to the model.
        s.push_str(&decision_schema(d));
        s.push('\n');
        s.push_str(&decision_ask(d));
        s.push('\n');
    }

    // Exercise GameEvent::render() for every variant directly: with_roles seeds
    // only RolesDealt into history, so a fixture game would leave most render
    // arms (powers, veto, execution, game-end) uncovered. Hashing each variant's
    // render output means editing any transcript wording moves the scaffold id.
    let events = [
        GameEvent::GameStarted { players: 7 },
        GameEvent::RolesDealt {
            seat: 4,
            role: Role::Fascist,
            known_teammates: vec![(5, Role::Fascist), (6, Role::Hitler)],
        },
        GameEvent::ChancellorNominated {
            president: 0,
            nominee: 1,
        },
        GameEvent::VotesRevealed {
            nominee: 1,
            votes: vec![(0, true), (1, false), (2, true)],
            passed: true,
        },
        GameEvent::ElectionTrackerAdvanced { value: 2 },
        GameEvent::TopDeckEnacted {
            policy: Party::Fascist,
        },
        GameEvent::GovernmentFormed {
            president: 0,
            chancellor: 1,
        },
        GameEvent::PresidentDrew {
            tiles: vec![Party::Liberal, Party::Fascist, Party::Fascist],
        },
        GameEvent::PresidentDiscarded {
            policy: Party::Liberal,
        },
        GameEvent::ChancellorReceived {
            tiles: vec![Party::Fascist, Party::Fascist],
        },
        GameEvent::PolicyEnacted {
            policy: Party::Fascist,
        },
        GameEvent::DeckReshuffled { draw_count: 14 },
        GameEvent::PowerGranted {
            president: 0,
            power: Power::InvestigateLoyalty,
        },
        GameEvent::Investigated {
            president: 0,
            target: 3,
        },
        GameEvent::InvestigationResult {
            target: 3,
            party: Party::Liberal,
        },
        GameEvent::SpecialElectionCalled {
            president: 0,
            target: 4,
        },
        GameEvent::Executed {
            president: 0,
            target: 3,
            was_hitler: false,
        },
        GameEvent::VetoProposed { chancellor: 1 },
        GameEvent::VetoDecided {
            president: 0,
            approved: true,
        },
        GameEvent::Utterance {
            seat: 2,
            discussion_round: 0,
            text: "I trust Player 0.".into(),
            pass: false,
        },
        GameEvent::ForcedDefault {
            seat: 3,
            decision: "vote".into(),
        },
        GameEvent::GameEnded {
            winner: Party::Liberal,
            condition: WinCondition::HitlerExecuted,
            roles: roles.to_vec(),
        },
    ];
    for e in &events {
        s.push_str(&e.render());
        s.push('\n');
    }
    s
}

/// Hash of every template + rendered prompt code + persona + temperature: the
/// scaffold version recorded on each seat of each game.
pub fn scaffold_version(persona: Option<&str>, temperature: f32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(PROMPT_REVISION);
    hasher.update(RULES_SUMMARY);
    hasher.update(OUTPUT_CONTRACT);
    hasher.update(SPEECH_SCHEMA);
    hasher.update(BELIEFS_INSTRUCTION);
    hasher.update(golden_prompt_render());
    hasher.update(persona.unwrap_or(""));
    hasher.update(temperature.to_le_bytes());
    let digest = hasher.finalize();
    format!("sc-{:x}", digest)[..11].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_render_exercises_every_prompt_function() {
        let g = golden_prompt_render();
        // role_brief for each role...
        assert!(g.contains("your secret role is Liberal"));
        assert!(g.contains("your secret role is Fascist"));
        assert!(g.contains("your secret role is Hitler"));
        // ...the Fascist teammate block from role_brief...
        assert!(g.contains("is Hitler."));
        // ...render_observation board header...
        assert!(g.contains("== BOARD =="));
        // ...and every decision schema.
        for token in [
            "\"nominate\"",
            "\"vote\"",
            "\"discard\"",
            "\"enact\"",
            "veto_consent",
            "use_power",
        ] {
            assert!(g.contains(token), "golden render missing schema {token}");
        }
        // ...the natural-language decision_ask lines...
        assert!(g.contains("nominate a Chancellor"), "missing decision_ask");
        // ...and GameEvent::render() output (both public and private arms).
        for token in [
            "Game started with 7 players",
            "Government formed",
            "top policy auto-enacted",
            "policy tiles",
        ] {
            assert!(g.contains(token), "golden render missing event {token}");
        }
    }

    #[test]
    fn scaffold_version_is_deterministic_and_input_sensitive() {
        let base = scaffold_version(None, 0.7);
        assert_eq!(base, scaffold_version(None, 0.7), "same inputs → same id");
        assert_ne!(
            base,
            scaffold_version(Some("aggressive"), 0.7),
            "persona shifts id"
        );
        assert_ne!(base, scaffold_version(None, 0.9), "temperature shifts id");
        assert!(base.starts_with("sc-"));
    }
}
