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

/// Bump on ANY change to prompt-shaping code that the constant hashes below
/// cannot see: `role_brief`, `decision_schema`/`decision_ask`, or
/// `render_observation`. Keeps rating attribution honest — a scaffold id must
/// change whenever model-visible prompts change.
const PROMPT_REVISION: &str = "prompts-v1";

/// Hash of every template + persona + temperature: the scaffold version.
pub fn scaffold_version(persona: Option<&str>, temperature: f32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(PROMPT_REVISION);
    hasher.update(RULES_SUMMARY);
    hasher.update(OUTPUT_CONTRACT);
    hasher.update(SPEECH_SCHEMA);
    hasher.update(BELIEFS_INSTRUCTION);
    hasher.update(persona.unwrap_or(""));
    hasher.update(temperature.to_le_bytes());
    let digest = hasher.finalize();
    format!("sc-{:x}", digest)[..11].to_string()
}
