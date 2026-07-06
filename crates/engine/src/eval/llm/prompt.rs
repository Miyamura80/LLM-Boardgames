//! Rendering a seat [`Observation`] into a prompt and parsing the model's
//! strict-JSON reply back into an [`Action`]. This is the versioned *scaffold* —
//! the rated unit is "model + scaffold", so `SCAFFOLD_VERSION` is part of the
//! agent name.

use crate::eval::agent::{AgentError, Beliefs};
use crate::game::{Action, Decision, DecisionKind, Event, Observation, Party, Policy, Role};
use std::collections::BTreeMap;

pub const SCAFFOLD_VERSION: &str = "sh-scaffold-v1";

pub fn system_prompt() -> String {
    "You are an expert player of the social-deduction game Secret Hitler \
(7 players: 4 Liberals, 3 Fascists including Hitler). Liberals win by enacting 5 \
Liberal policies or executing Hitler. Fascists win by enacting 6 Fascist policies \
or getting Hitler elected Chancellor once 3+ Fascist policies are enacted. \
Regular Fascists know each other and Hitler; Hitler knows no one; Liberals know no \
one. Play to win for YOUR faction: deduce, persuade, and (as a Fascist) deceive. \
Always reply with a SINGLE JSON object and nothing else — no markdown, no prose \
outside the JSON. Include a brief \"thought_process\" string for your own reasoning."
        .to_string()
}

/// Render the public + private state this seat is entitled to.
pub fn render_state(obs: &Observation) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "You are seat {}. Your secret role: {}.\n",
        obs.you,
        role_name(obs.your_role)
    ));
    if let Some(team) = &obs.known_team {
        s.push_str(&format!(
            "You KNOW the Fascists are seats {:?} and Hitler is seat {}.\n",
            team.fascists, team.hitler
        ));
    } else if obs.your_role == Role::Hitler {
        s.push_str("You are Hitler; you do NOT know your teammates.\n");
    }
    s.push_str(&format!(
        "Board: {} Liberal / {} Fascist policies. Election tracker: {}/3.\n",
        obs.board.liberal, obs.board.fascist, obs.election_tracker
    ));
    if obs.veto_unlocked {
        s.push_str("Veto power is unlocked.\n");
    }
    let alive: Vec<usize> = obs
        .players
        .iter()
        .filter(|p| p.alive)
        .map(|p| p.seat)
        .collect();
    let dead: Vec<usize> = obs
        .players
        .iter()
        .filter(|p| !p.alive)
        .map(|p| p.seat)
        .collect();
    s.push_str(&format!("Living seats: {alive:?}. Dead: {dead:?}.\n"));
    if !obs.term_limited.is_empty() {
        s.push_str(&format!(
            "Term-limited (ineligible as Chancellor): {:?}.\n",
            obs.term_limited
        ));
    }
    if !obs.your_investigations.is_empty() {
        let inv: Vec<String> = obs
            .your_investigations
            .iter()
            .map(|(seat, party)| format!("seat {seat}={}", party_name(*party)))
            .collect();
        s.push_str(&format!(
            "Your investigation results: {}.\n",
            inv.join(", ")
        ));
    }
    s.push_str("\nRecent history:\n");
    for line in obs.history.iter().filter_map(|e| format_event(e, obs.you)) {
        s.push_str(&format!("- {line}\n"));
    }
    s
}

/// The decision-specific instruction + required JSON shape.
pub fn decision_instructions(decision: &Decision, feedback: Option<&str>) -> String {
    let mut s = String::new();
    if let Some(fb) = feedback {
        s.push_str(&format!("Your previous answer was rejected: {fb}\n\n"));
    }
    let body = match &decision.kind {
        DecisionKind::Nominate { eligible, .. } => format!(
            "As President, nominate a Chancellor. Legal seats: {eligible:?}. \
Reply: {{\"thought_process\": \"...\", \"nominee\": <seat>}}"
        ),
        DecisionKind::Vote { president, nominee, .. } => format!(
            "Vote on the government President={president}, Chancellor={nominee}. \
Reply: {{\"thought_process\": \"...\", \"vote\": \"ja\" | \"nein\"}}"
        ),
        DecisionKind::PresidentDiscard { drawn, .. } => format!(
            "As President you drew these 3 policies (indexed): {}. Discard ONE; \
the other two go to the Chancellor. Reply: {{\"thought_process\": \"...\", \"discard_index\": 0|1|2}}",
            index_policies(drawn)
        ),
        DecisionKind::ChancellorEnact { hand, veto_available, .. } => {
            let veto = if *veto_available {
                " You may instead propose a veto."
            } else {
                ""
            };
            format!(
                "As Chancellor you hold these 2 policies (indexed): {}. Enact ONE.{veto} \
Reply: {{\"thought_process\": \"...\", \"choice\": \"enact\"|\"veto\", \"enact_index\": 0|1}} \
(enact_index required when choice=enact)",
                index_policies(hand)
            )
        }
        DecisionKind::VetoConsent { .. } => {
            "As President, the Chancellor proposed a veto. Consent to discard both policies? \
Reply: {\"thought_process\": \"...\", \"consent\": true|false}".to_string()
        }
        DecisionKind::Investigate { eligible, .. } => format!(
            "As President, investigate a player's party membership (private). Legal seats: {eligible:?}. \
Reply: {{\"thought_process\": \"...\", \"target\": <seat>}}"
        ),
        DecisionKind::SpecialElection { eligible, .. } => format!(
            "As President, appoint the next Presidential candidate. Legal seats: {eligible:?}. \
Reply: {{\"thought_process\": \"...\", \"target\": <seat>}}"
        ),
        DecisionKind::Execution { eligible, .. } => format!(
            "As President, execute a player. Legal seats: {eligible:?}. \
Reply: {{\"thought_process\": \"...\", \"target\": <seat>}}"
        ),
    };
    s.push_str(&body);
    s
}

/// Parse the model's JSON reply into an [`Action`] for this decision.
pub fn parse_action(decision: &Decision, you: usize, text: &str) -> Result<Action, AgentError> {
    let v = parse_json(text)?;
    let seat_field = |key: &str| -> Result<usize, AgentError> {
        v.get(key)
            .and_then(|x| x.as_u64())
            .map(|n| n as usize)
            .ok_or_else(|| AgentError::Malformed(format!("missing integer field '{key}'")))
    };

    match &decision.kind {
        DecisionKind::Nominate { .. } => Ok(Action::Nominate(seat_field("nominee")?)),
        DecisionKind::Vote { .. } => {
            let vote = v
                .get("vote")
                .and_then(|x| x.as_str())
                .ok_or_else(|| AgentError::Malformed("missing 'vote'".into()))?;
            let ja = matches!(vote.to_ascii_lowercase().as_str(), "ja" | "yes" | "true");
            Ok(Action::CastVotes(BTreeMap::from([(you, ja)])))
        }
        DecisionKind::PresidentDiscard { .. } => {
            Ok(Action::Discard(index_field(&v, "discard_index")?))
        }
        DecisionKind::ChancellorEnact { veto_available, .. } => {
            let choice = v.get("choice").and_then(|x| x.as_str()).unwrap_or("enact");
            if choice.eq_ignore_ascii_case("veto") && *veto_available {
                Ok(Action::ProposeVeto)
            } else {
                Ok(Action::Enact(index_field(&v, "enact_index")?))
            }
        }
        DecisionKind::VetoConsent { .. } => {
            let consent = v
                .get("consent")
                .and_then(|x| x.as_bool())
                .ok_or_else(|| AgentError::Malformed("missing boolean 'consent'".into()))?;
            Ok(Action::VetoConsent(consent))
        }
        DecisionKind::Investigate { .. } => Ok(Action::Investigate(seat_field("target")?)),
        DecisionKind::SpecialElection { .. } => Ok(Action::SpecialElection(seat_field("target")?)),
        DecisionKind::Execution { .. } => Ok(Action::Execute(seat_field("target")?)),
    }
}

pub fn discussion_prompt(round: u8, total: u8) -> String {
    format!(
        "Discussion round {}/{}. Say ONE short statement (max ~40 words) to the table \
to advance your faction's goals. Reply: {{\"statement\": \"...\"}}",
        round + 1,
        total
    )
}

pub fn parse_discussion(text: &str) -> Option<String> {
    let v = parse_json(text).ok()?;
    v.get("statement")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn beliefs_prompt(obs: &Observation) -> String {
    let others: Vec<usize> = obs
        .players
        .iter()
        .filter(|p| p.alive && p.seat != obs.you)
        .map(|p| p.seat)
        .collect();
    format!(
        "Privately estimate, for each of these living seats {others:?}, the probability \
(0.0–1.0) that they are on the FASCIST team (a Fascist or Hitler). This is private \
and does not affect the game. Reply: {{\"fascist_probabilities\": {{\"<seat>\": <prob>, ...}}}}"
    )
}

pub fn parse_beliefs(text: &str) -> Option<Beliefs> {
    let v = parse_json(text).ok()?;
    let obj = v.get("fascist_probabilities")?.as_object()?;
    let mut fascist_prob = BTreeMap::new();
    for (k, val) in obj {
        if let (Ok(seat), Some(p)) = (k.parse::<usize>(), val.as_f64()) {
            fascist_prob.insert(seat, p.clamp(0.0, 1.0));
        }
    }
    (!fascist_prob.is_empty()).then_some(Beliefs { fascist_prob })
}

// ---- helpers --------------------------------------------------------------

fn index_field(v: &serde_json::Value, key: &str) -> Result<usize, AgentError> {
    v.get(key)
        .and_then(|x| x.as_u64())
        .map(|n| n as usize)
        .ok_or_else(|| AgentError::Malformed(format!("missing integer field '{key}'")))
}

/// Parse JSON, tolerating a ```json fenced block or surrounding prose.
fn parse_json(text: &str) -> Result<serde_json::Value, AgentError> {
    let trimmed = text.trim();
    let candidate = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };
    serde_json::from_str(candidate).map_err(|e| AgentError::Malformed(format!("invalid JSON: {e}")))
}

fn index_policies(tiles: &[Policy]) -> String {
    tiles
        .iter()
        .enumerate()
        .map(|(i, p)| format!("[{i}]={}", policy_name(*p)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn role_name(r: Role) -> &'static str {
    match r {
        Role::Liberal => "Liberal",
        Role::Fascist => "Fascist",
        Role::Hitler => "Hitler",
    }
}
fn party_name(p: Party) -> &'static str {
    match p {
        Party::Liberal => "Liberal",
        Party::Fascist => "Fascist",
    }
}
fn policy_name(p: Policy) -> &'static str {
    match p {
        Policy::Liberal => "Liberal",
        Policy::Fascist => "Fascist",
    }
}

/// Format an event as a short public-history line from `you`'s perspective.
fn format_event(e: &Event, you: usize) -> Option<String> {
    let line = match e {
        Event::PresidencyBegan {
            president,
            special_election,
        } => {
            if *special_election {
                format!("Seat {president} becomes President (special election).")
            } else {
                format!("Seat {president} becomes President.")
            }
        }
        Event::ChancellorNominated { president, nominee } => {
            format!("President {president} nominated seat {nominee} as Chancellor.")
        }
        Event::Utterance { seat, text, .. } => format!("Seat {seat} says: \"{text}\""),
        Event::VotesCast {
            votes,
            ja,
            needed,
            passed,
        } => {
            let detail: Vec<String> = votes
                .iter()
                .map(|(s, v)| format!("{s}:{}", if *v { "ja" } else { "nein" }))
                .collect();
            format!(
                "Vote {} ({ja}/{needed} needed): {}",
                if *passed { "PASSED" } else { "failed" },
                detail.join(",")
            )
        }
        Event::GovernmentElected {
            president,
            chancellor,
        } => {
            format!("Government elected: President {president}, Chancellor {chancellor}.")
        }
        Event::ElectionFailed { tracker } => format!("Election failed. Tracker now {tracker}/3."),
        Event::ChaosPolicyEnacted {
            policy,
            liberal,
            fascist,
        } => format!(
            "Chaos! Top policy auto-enacted ({}). Board {liberal}L/{fascist}F.",
            policy_name(*policy)
        ),
        Event::PolicyEnacted {
            policy,
            liberal,
            fascist,
            ..
        } => {
            format!(
                "Policy enacted: {}. Board {liberal}L/{fascist}F.",
                policy_name(*policy)
            )
        }
        Event::PowerGranted { president, power } => {
            format!("President {president} gains a power: {power:?}.")
        }
        Event::LoyaltyInvestigated { president, target } => {
            format!("President {president} investigated seat {target} (result private).")
        }
        Event::SpecialElectionCalled {
            president,
            appointed,
        } => {
            format!("President {president} called a special election, appointing seat {appointed}.")
        }
        Event::PlayerExecuted { president, target } => {
            format!("President {president} executed seat {target}.")
        }
        Event::VetoProposed { chancellor } => format!("Chancellor {chancellor} proposed a veto."),
        Event::VetoResolved {
            president,
            consented,
        } => format!(
            "President {president} {} the veto.",
            if *consented { "accepted" } else { "rejected" }
        ),
        Event::GameOver { winner, reason } => format!("GAME OVER: {winner:?} win ({reason:?})."),
        Event::ForcedDefault { seat, decision } => {
            format!("Seat {seat} timed out; a forced default was applied ({decision}).")
        }
        // Private events for `you` only.
        Event::RoleAssigned { role } => format!("(private) Your role: {}.", role_name(*role)),
        Event::FascistTeamRevealed { fascists, hitler } => {
            format!("(private) Fascists: {fascists:?}, Hitler: seat {hitler}.")
        }
        Event::DrewPolicies { policies } => {
            format!("(private) You drew: {}.", index_policies(policies))
        }
        Event::ReceivedPolicies { policies } => {
            format!("(private) You received: {}.", index_policies(policies))
        }
        Event::InvestigationResult { target, party } => {
            format!("(private) Seat {target} is {}.", party_name(*party))
        }
        Event::GameStarted { .. } => return None,
    };
    // `you` is used to keep the signature future-proof (already filtered by log).
    let _ = you;
    Some(line)
}
