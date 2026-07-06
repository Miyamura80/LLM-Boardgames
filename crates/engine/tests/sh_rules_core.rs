//! Rules suite: setup, knowledge graph, elections, term limits, tracker,
//! observation privacy.

mod common;

use common::*;
use engine::secret_hitler::actions::Action;
use engine::secret_hitler::events::GameEvent;
use engine::secret_hitler::state::{GameState, Phase};
use engine::secret_hitler::types::*;

#[test]
fn setup_deals_4_liberals_2_fascists_1_hitler_and_17_tiles() {
    let state = GameState::new(123);
    let mut lib = 0;
    let mut fasc = 0;
    let mut hitler = 0;
    for p in &state.players {
        match p.role {
            Role::Liberal => lib += 1,
            Role::Fascist => fasc += 1,
            Role::Hitler => hitler += 1,
        }
    }
    assert_eq!((lib, fasc, hitler), (4, 2, 1));
    assert_eq!(state.deck.total(), 17);
    assert_eq!(state.deck.draw_count(), 17);
}

#[test]
fn knowledge_graph_is_exact_at_7_players() {
    let state = scripted(&deck_f_then_l(11, 6));
    // Regular Fascists (4, 5) know each other and Hitler (6).
    for fascist in [4u8, 5u8] {
        let obs = state.observe(fascist);
        let mut known = obs.known_teammates.clone();
        known.sort();
        let other = if fascist == 4 { 5 } else { 4 };
        assert_eq!(known, vec![(other, Role::Fascist), (6, Role::Hitler)]);
    }
    // Hitler knows NO teammates at 7 players.
    assert!(state.observe(6).known_teammates.is_empty());
    // Liberals know nobody.
    for lib in 0..4 {
        assert!(state.observe(lib).known_teammates.is_empty());
    }
}

#[test]
fn observations_do_not_leak_roles_or_private_events() {
    let mut state = scripted(&deck_f_then_l(11, 6));
    pass_government(&mut state, 1, Party::Fascist);

    // A Liberal's history contains no other seat's RolesDealt.
    let obs = state.observe(2);
    for rec in &obs.history {
        if let GameEvent::RolesDealt { seat, .. } = &rec.event {
            assert_eq!(*seat, 2, "role deal for seat {seat} leaked to seat 2");
        }
        // Nobody but the President saw the drawn tiles.
        assert!(
            !matches!(rec.event, GameEvent::PresidentDrew { .. }),
            "president's tiles leaked to seat 2"
        );
    }
    // The President (seat 0) does see their draw.
    assert!(state
        .observe(0)
        .history
        .iter()
        .any(|r| matches!(r.event, GameEvent::PresidentDrew { .. })));
    // The Chancellor (seat 1) sees their two tiles, seat 2 does not.
    assert!(state
        .observe(1)
        .history
        .iter()
        .any(|r| matches!(r.event, GameEvent::ChancellorReceived { .. })));
}

#[test]
fn ballots_stay_hidden_until_all_votes_are_in() {
    let mut state = scripted(&deck_f_then_l(11, 6));
    state.apply(0, Action::Nominate { target: 1 }).unwrap();
    state.apply(0, Action::Vote { ja: true }).unwrap();
    state.apply(1, Action::Vote { ja: true }).unwrap();

    assert!(!state
        .events
        .iter()
        .any(|r| matches!(r.event, GameEvent::VotesRevealed { .. })));
    // The remaining seats still owe a ballot.
    let pending = state.pending_decisions();
    assert_eq!(pending.len(), 5);
    // Double-voting is illegal.
    assert!(state.apply(0, Action::Vote { ja: false }).is_err());
}

#[test]
fn majority_passes_and_non_majority_fails() {
    // 7 voters: 4 Ja / 3 Nein passes.
    let mut state = scripted(&deck_f_then_l(11, 6));
    state.apply(0, Action::Nominate { target: 1 }).unwrap();
    vote(&mut state, &[0, 1, 2, 3]);
    assert!(matches!(state.phase, Phase::LegislativePresident { .. }));

    // 3 Ja / 4 Nein fails and advances the tracker + presidency.
    let mut state = scripted(&deck_f_then_l(11, 6));
    state.apply(0, Action::Nominate { target: 1 }).unwrap();
    vote(&mut state, &[0, 1, 2]);
    assert_eq!(state.election_tracker, 1);
    assert_eq!(state.presidency, 1);
    assert!(matches!(state.phase, Phase::Nomination));
}

#[test]
fn term_limits_exclude_last_elected_president_and_chancellor() {
    let mut state = scripted(&deck_f_then_l(11, 6));
    pass_government(&mut state, 1, Party::Fascist); // P0, C1 elected
    assert_eq!(state.presidency, 1);

    let eligible = state.eligible_chancellors();
    // Seat 0 (last president) and seat 1 (last chancellor = sitting president,
    // excluded twice over) are out.
    assert!(
        !eligible.contains(&0),
        "last president must be term-limited"
    );
    assert!(!eligible.contains(&1));
    // Nominating the term-limited player is an illegal move with a reason.
    let err = state.apply(1, Action::Nominate { target: 0 }).unwrap_err();
    assert!(err.0.contains("not an eligible Chancellor"));

    // Electing a new government moves the limits.
    pass_government(&mut state, 2, Party::Fascist); // P1, C2
    let eligible = state.eligible_chancellors();
    assert!(eligible.contains(&0), "old limits must clear");
    assert!(!eligible.contains(&1) || state.presidency == 1);
    assert!(!eligible.contains(&2));
}

#[test]
fn dead_players_are_excluded_everywhere() {
    let mut state = scripted(&[Party::Fascist; 17]);
    reach_fascist_policies(&mut state, 4); // 4th policy = Execution, kills someone
    let dead: Vec<Seat> = (0..7).filter(|&s| !state.is_alive(s)).collect();
    assert_eq!(dead.len(), 1);
    let victim = dead[0];

    // Dead player cannot act…
    let err = state.apply(victim, Action::Vote { ja: true }).unwrap_err();
    assert!(err.0.contains("dead"));
    // …is never asked to act…
    for d in state.pending_decisions() {
        assert_ne!(d.seat(), victim);
    }
    // …and is not an eligible chancellor.
    assert!(!state.eligible_chancellors().contains(&victim));
}

#[test]
fn election_tracker_topdecks_after_3_failures_no_power_and_limits_reset() {
    // Top tile is Fascist — enough fascist tiles that a power WOULD fire if
    // this were a government policy.
    let mut state = scripted(&[Party::Fascist; 17]);
    pass_government(&mut state, 1, Party::Fascist); // 1st fascist policy, no power
    pass_government(&mut state, 2, Party::Fascist); // 2nd → Investigate
    use_power(&mut state, 0);

    // Three consecutive failed elections.
    for _ in 0..3 {
        let p = state.presidency;
        let nominee = safe_chancellor(&state);
        state
            .apply(p, Action::Nominate { target: nominee })
            .unwrap();
        vote(&mut state, &[]);
    }

    // 3rd fascist policy top-decked: NO Special Election fires.
    assert_eq!(state.fascist_policies, 3);
    assert!(
        matches!(state.phase, Phase::Nomination),
        "top-deck must not grant a power, got {:?}",
        state.phase
    );
    assert_eq!(state.election_tracker, 0, "tracker resets");
    assert!(state
        .events
        .iter()
        .any(|r| matches!(r.event, GameEvent::TopDeckEnacted { .. })));
    // Term-limit memory cleared.
    let eligible = state.eligible_chancellors();
    assert_eq!(
        eligible.len(),
        state.alive_count() as usize - 1,
        "everyone but the president is eligible after a top-deck: {eligible:?}"
    );
}

#[test]
fn term_limit_relaxes_for_last_president_at_5_players() {
    let mut state = scripted(&[Party::Fascist; 17]);
    // Reach 5 fascist policies: two executions happen along the way.
    reach_fascist_policies(&mut state, 5);
    assert_eq!(state.alive_count(), 5);
    assert!(!state.is_over());

    let last_p = state.term_limited_president.unwrap();
    let last_c = state.term_limited_chancellor.unwrap();
    let eligible = state.eligible_chancellors();

    // Only the last chancellor stays limited at ≤5 players.
    if last_p != state.presidency && state.is_alive(last_p) {
        assert!(
            eligible.contains(&last_p),
            "last president {last_p} must be eligible again at 5 players (eligible: {eligible:?})"
        );
    }
    if state.is_alive(last_c) {
        assert!(!eligible.contains(&last_c), "last chancellor stays limited");
    }
}

#[test]
fn presidency_rotates_clockwise_skipping_dead_players() {
    let mut state = scripted(&[Party::Fascist; 17]);
    assert_eq!(state.presidency, 0);
    pass_government(&mut state, 1, Party::Fascist);
    assert_eq!(state.presidency, 1);
    pass_government(&mut state, 2, Party::Fascist); // investigate fires
    use_power(&mut state, 0);
    assert_eq!(state.presidency, 2);
}
