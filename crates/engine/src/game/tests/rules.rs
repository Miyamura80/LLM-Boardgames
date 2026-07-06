//! Scenario coverage for every US-002 rules edge case.

use super::{ballot, elect, LAYOUT};
use crate::game::log::WinReason;
use crate::game::roles::Party;
use crate::game::*;
use std::collections::BTreeMap;
use Policy::{Fascist as F, Liberal as L};
use Role::{Fascist as RF, Hitler as RH, Liberal as RL};

// ===========================================================================
// Setup, knowledge graph, information isolation (US-003)
// ===========================================================================

#[test]
fn seven_player_knowledge_graph_is_exact() {
    let g = GameState::test_game(LAYOUT, vec![L, L, L], 0);

    // Regular Fascists (4, 5) know each other and know Hitler (6).
    for f in [4, 5] {
        let obs = g.observation(f);
        assert_eq!(obs.your_role, RF);
        let team = obs.known_team.expect("regular Fascist knows the team");
        assert!(team.fascists.contains(&4) && team.fascists.contains(&5));
        assert_eq!(team.hitler, 6);
    }

    // Hitler is blind: no team, no reveal event.
    let h = g.observation(6);
    assert_eq!(h.your_role, RH);
    assert!(h.known_team.is_none());
    assert!(!h
        .history
        .iter()
        .any(|e| matches!(e, Event::FascistTeamRevealed { .. })));

    // Liberals know nothing about others.
    let lib = g.observation(0);
    assert_eq!(lib.your_role, RL);
    assert!(lib.known_team.is_none());
}

#[test]
fn observation_never_leaks_a_foreign_role() {
    let g = GameState::test_game(LAYOUT, vec![L, L, L], 0);
    for seat in 0..7 {
        let obs = g.observation(seat);
        // The only RoleAssigned event a seat may witness is its own.
        let roles: Vec<_> = obs
            .history
            .iter()
            .filter(|e| matches!(e, Event::RoleAssigned { .. }))
            .collect();
        assert_eq!(
            roles.len(),
            1,
            "seat {seat} sees exactly one role (its own)"
        );
        // A Liberal/Hitler must not witness the Fascist team reveal.
        if obs.your_role != RF {
            assert!(!obs
                .history
                .iter()
                .any(|e| matches!(e, Event::FascistTeamRevealed { .. })));
        }
    }
}

#[test]
fn drawn_tiles_and_investigations_stay_private() {
    let mut g = GameState::test_game(LAYOUT, vec![F, L, L, L, L, L], 0);
    elect(&mut g, 1); // President 0, Chancellor 1

    // President (0) sees its drawn tiles; nobody else does.
    assert!(g
        .observation(0)
        .history
        .iter()
        .any(|e| matches!(e, Event::DrewPolicies { .. })));
    for other in [2, 3, 4, 5, 6] {
        assert!(!g
            .observation(other)
            .history
            .iter()
            .any(|e| matches!(e, Event::DrewPolicies { .. })));
    }

    // President discards → Chancellor (1) alone receives the two-tile hand.
    g.apply(Action::Discard(1)).unwrap();
    assert!(g
        .observation(1)
        .history
        .iter()
        .any(|e| matches!(e, Event::ReceivedPolicies { .. })));
    assert!(!g
        .observation(0)
        .history
        .iter()
        .any(|e| matches!(e, Event::ReceivedPolicies { .. })));
}

// ===========================================================================
// Win conditions
// ===========================================================================

#[test]
fn liberals_win_on_fifth_liberal_policy() {
    let mut g = GameState::test_game(LAYOUT, vec![L, L, L], 0);
    g.test_set_board(4, 0);
    elect(&mut g, 1);
    g.apply(Action::Discard(0)).unwrap(); // discard a Liberal, keep [L,L]
    g.apply(Action::Enact(0)).unwrap(); // enact Liberal → 5
    assert_eq!(g.winner(), Some(Faction::Liberal));
    assert_eq!(g.win_reason(), Some(WinReason::LiberalPolicies));
}

#[test]
fn fascists_win_on_sixth_fascist_policy() {
    let mut g = GameState::test_game(LAYOUT, vec![F, F, F], 0);
    g.test_set_board(0, 5);
    elect(&mut g, 1);
    g.apply(Action::Discard(0)).unwrap(); // keep [F,F]
    g.apply(Action::Enact(0)).unwrap(); // enact Fascist → 6
    assert_eq!(g.winner(), Some(Faction::Fascist));
    assert_eq!(g.win_reason(), Some(WinReason::FascistPolicies));
}

#[test]
fn hitler_elected_chancellor_after_three_fascist_policies_wins() {
    let mut g = GameState::test_game(LAYOUT, vec![L, L, L], 0);
    g.test_set_board(0, 3);
    elect(&mut g, 6); // nominate Hitler, elect
    assert_eq!(g.winner(), Some(Faction::Fascist));
    assert_eq!(g.win_reason(), Some(WinReason::HitlerChancellor));
    // No policy phase happened — the win fires on the vote.
    assert!(g.current_decision().is_none());
}

#[test]
fn hitler_chancellor_below_three_fascist_policies_does_not_win() {
    let mut g = GameState::test_game(LAYOUT, vec![L, L, L], 0);
    g.test_set_board(0, 2);
    elect(&mut g, 6);
    assert!(!g.is_over());
    // Proceeds to the legislative session.
    assert!(matches!(
        g.current_decision().unwrap().kind,
        DecisionKind::PresidentDiscard { .. }
    ));
}

// ===========================================================================
// Presidential powers (7–8 board)
// ===========================================================================

#[test]
fn investigate_loyalty_reveals_party_privately() {
    let mut g = GameState::test_game(LAYOUT, vec![F, L, L, L, L, L], 0);
    g.test_set_board(0, 1); // next Fascist policy → investigate square
    elect(&mut g, 1);
    g.apply(Action::Discard(1)).unwrap(); // keep [F,L]
    g.apply(Action::Enact(0)).unwrap(); // Fascist → 2 → investigate power

    let d = g.current_decision().unwrap();
    let DecisionKind::Investigate { eligible, .. } = d.kind else {
        panic!("expected investigate power");
    };
    assert!(!eligible.contains(&0), "cannot investigate self");

    // Investigate Hitler (6): the card reads Fascist.
    g.apply(Action::Investigate(6)).unwrap();
    let pres = g.observation(0);
    assert_eq!(pres.your_investigations, vec![(6, Party::Fascist)]);
    // The result is private to the investigating President.
    assert!(g.observation(1).your_investigations.is_empty());
    assert!(!g
        .observation(1)
        .history
        .iter()
        .any(|e| matches!(e, Event::InvestigationResult { .. })));
    // The *fact* of investigation is public, though.
    assert!(g
        .observation(1)
        .history
        .iter()
        .any(|e| matches!(e, Event::LoyaltyInvestigated { .. })));
}

#[test]
fn special_election_returns_to_normal_order_after_the_special_term() {
    let mut g = GameState::test_game(LAYOUT, vec![F, L, L], 2);
    g.test_set_board(0, 2); // next Fascist → special election square
    elect(&mut g, 0); // President 2, Chancellor 0
    g.apply(Action::Discard(1)).unwrap(); // keep [F,L]
    g.apply(Action::Enact(0)).unwrap(); // Fascist → 3 → special election

    assert!(matches!(
        g.current_decision().unwrap().kind,
        DecisionKind::SpecialElection { .. }
    ));
    g.apply(Action::SpecialElection(5)).unwrap(); // appoint seat 5
    assert_eq!(g.president(), 5, "appointed player is next President");

    // End seat 5's special term with a failed election.
    g.apply(Action::Nominate(1)).unwrap();
    let b = ballot(&g, false);
    g.apply(b).unwrap();

    // Presidency resumes at the seat after the invoker (2) → seat 3.
    assert_eq!(g.president(), 3);
}

#[test]
fn executing_hitler_wins_for_liberals_immediately() {
    let mut g = GameState::test_game(LAYOUT, vec![F, L, L], 0);
    g.test_set_board(0, 3); // next Fascist → execution square
    elect(&mut g, 1);
    g.apply(Action::Discard(1)).unwrap();
    g.apply(Action::Enact(0)).unwrap(); // Fascist → 4 → execution
    assert!(matches!(
        g.current_decision().unwrap().kind,
        DecisionKind::Execution { .. }
    ));
    g.apply(Action::Execute(6)).unwrap(); // shoot Hitler
    assert_eq!(g.winner(), Some(Faction::Liberal));
    assert_eq!(g.win_reason(), Some(WinReason::HitlerExecuted));
}

#[test]
fn executing_a_liberal_continues_the_game_and_relaxes_term_limits_below_six() {
    // Drive two executions to bring the table to 5 living players and verify the
    // ≤5 relaxation: only the last Chancellor stays term-limited.
    let mut g = GameState::test_game(LAYOUT, vec![F, L, L, F, L, L], 0);
    g.test_set_board(0, 3);

    // Gov 1: President 0 / Chancellor 1, enact Fascist → 4 → execution.
    elect(&mut g, 1);
    g.apply(Action::Discard(1)).unwrap();
    g.apply(Action::Enact(0)).unwrap();
    g.apply(Action::Execute(2)).unwrap(); // kill a Liberal (seat 2)
    assert!(!g.is_over());
    assert!(!g.is_alive(2));
    assert_eq!(g.living_count(), 6);
    assert_eq!(g.president(), 1); // rotation advanced 0 → 1

    // Gov 2: President 1 must nominate someone not term-limited.
    // last_president=0, last_chancellor=1 with 6 alive → both barred.
    let elig = match g.current_decision().unwrap().kind {
        DecisionKind::Nominate { eligible, .. } => eligible,
        _ => panic!("expected nomination"),
    };
    assert!(!elig.contains(&0) && !elig.contains(&1));
    elect(&mut g, 3);
    g.apply(Action::Discard(1)).unwrap();
    g.apply(Action::Enact(0)).unwrap(); // Fascist → 5 → execution
    g.apply(Action::Execute(4)).unwrap(); // kill a Fascist (seat 4)
    assert_eq!(g.living_count(), 5);

    // Now 5 alive: last_president=1 is freed, only last_chancellor=3 barred.
    let obs = g.observation(g.president());
    assert!(obs.term_limited.contains(&3));
    assert!(!obs.term_limited.contains(&1));
    let elig = match g.current_decision().unwrap().kind {
        DecisionKind::Nominate { eligible, .. } => eligible,
        _ => panic!("expected nomination"),
    };
    assert!(
        elig.contains(&1),
        "last President is eligible again below 6"
    );
    assert!(!elig.contains(&3), "last Chancellor still term-limited");
}

// ===========================================================================
// Term limits at 7 players
// ===========================================================================

#[test]
fn both_last_president_and_chancellor_are_term_limited_at_seven() {
    let mut g = GameState::test_game(LAYOUT, vec![L, L, L], 0);
    elect(&mut g, 1); // President 0, Chancellor 1
    g.apply(Action::Discard(0)).unwrap();
    g.apply(Action::Enact(0)).unwrap(); // Liberal enacted, no power → next gov
    assert_eq!(g.president(), 1);
    let elig = match g.current_decision().unwrap().kind {
        DecisionKind::Nominate { eligible, .. } => eligible,
        _ => panic!("expected nomination"),
    };
    // last_president=0 and last_chancellor=1 both barred (0 also happens to be
    // barred; 1 is the current president anyway).
    assert!(!elig.contains(&0));
    assert!(!elig.contains(&1));
}

// ===========================================================================
// Election tracker top-deck (chaos)
// ===========================================================================

#[test]
fn three_failed_elections_topdeck_grants_no_power_and_clears_term_limits() {
    let mut g = GameState::test_game(LAYOUT, vec![L, L, L, F, L, F], 0);
    g.test_set_board(0, 1);

    // One successful government first, to set term-limit memory.
    elect(&mut g, 1);
    g.apply(Action::Discard(0)).unwrap();
    g.apply(Action::Enact(0)).unwrap(); // Liberal → 1, president → 1

    // Three failed elections → chaos top-deck (top tile is Fascist → 2).
    for _ in 0..3 {
        let elig = match g.current_decision().unwrap().kind {
            DecisionKind::Nominate { eligible, .. } => eligible,
            _ => panic!("expected nomination"),
        };
        g.apply(Action::Nominate(elig[0])).unwrap();
        let b = ballot(&g, false);
        g.apply(b).unwrap();
    }

    assert_eq!(
        g.board().fascist,
        2,
        "top-decked Fascist advanced the track"
    );
    assert_eq!(g.election_tracker(), 0, "tracker reset after chaos");
    // Landing on the investigate square via chaos grants NO power.
    assert!(matches!(
        g.current_decision().unwrap().kind,
        DecisionKind::Nominate { .. }
    ));
    assert!(!g
        .log()
        .public_events()
        .any(|e| matches!(e, Event::PowerGranted { .. })));
    // Term-limit memory cleared.
    assert!(g.observation(g.president()).term_limited.is_empty());
}

// ===========================================================================
// Veto (unlocked at 5 Fascist policies)
// ===========================================================================

#[test]
fn veto_with_consent_enacts_nothing_and_advances_the_tracker() {
    let mut g = GameState::test_game(LAYOUT, vec![F, F, F], 0);
    g.test_set_board(0, 5); // veto unlocked
    elect(&mut g, 1);
    g.apply(Action::Discard(0)).unwrap(); // hand [F,F]
    match g.current_decision().unwrap().kind {
        DecisionKind::ChancellorEnact { veto_available, .. } => assert!(veto_available),
        _ => panic!("expected chancellor enact"),
    }
    g.apply(Action::ProposeVeto).unwrap();
    g.apply(Action::VetoConsent(true)).unwrap();
    assert_eq!(g.board().fascist, 5, "no policy enacted on veto");
    assert_eq!(g.election_tracker(), 1, "veto advances the tracker");
    assert!(!g.is_over());
}

#[test]
fn veto_refused_forces_chancellor_to_enact() {
    let mut g = GameState::test_game(LAYOUT, vec![F, F, F], 0);
    g.test_set_board(0, 5);
    elect(&mut g, 1);
    g.apply(Action::Discard(0)).unwrap();
    g.apply(Action::ProposeVeto).unwrap();
    g.apply(Action::VetoConsent(false)).unwrap();
    // Veto is spent; proposing again is illegal.
    assert_eq!(
        g.apply(Action::ProposeVeto),
        Err(IllegalAction::VetoUnavailable)
    );
    g.apply(Action::Enact(0)).unwrap(); // Fascist → 6
    assert_eq!(g.winner(), Some(Faction::Fascist));
}

#[test]
fn veto_unavailable_before_five_fascist_policies() {
    let mut g = GameState::test_game(LAYOUT, vec![F, F, F], 0);
    g.test_set_board(0, 4);
    elect(&mut g, 1);
    g.apply(Action::Discard(0)).unwrap();
    assert_eq!(
        g.apply(Action::ProposeVeto),
        Err(IllegalAction::VetoUnavailable)
    );
}

// ===========================================================================
// Illegal actions → typed errors, state unchanged
// ===========================================================================

#[test]
fn illegal_actions_are_rejected_without_mutating_state() {
    let mut g = GameState::test_game(LAYOUT, vec![L, L, L], 0);

    // Nominating the President (self) is illegal.
    assert_eq!(
        g.apply(Action::Nominate(0)),
        Err(IllegalAction::IllegalTarget(0))
    );
    // Wrong action type for a nomination.
    assert_eq!(
        g.apply(Action::Enact(0)),
        Err(IllegalAction::WrongActionType)
    );
    // Malformed ballot during voting.
    g.apply(Action::Nominate(1)).unwrap();
    let mut partial = BTreeMap::new();
    partial.insert(0usize, true); // missing the other voters
    assert_eq!(
        g.apply(Action::CastVotes(partial)),
        Err(IllegalAction::MalformedBallot)
    );
    // A valid ballot still works afterward (state was untouched).
    let b = ballot(&g, true);
    g.apply(b).unwrap();
    // Discard index out of range.
    assert_eq!(
        g.apply(Action::Discard(9)),
        Err(IllegalAction::IndexOutOfRange(9))
    );
}

#[test]
fn actions_after_game_over_are_rejected() {
    let mut g = GameState::test_game(LAYOUT, vec![L, L, L], 0);
    g.test_set_board(0, 3);
    elect(&mut g, 6); // Hitler chancellor win
    assert!(g.is_over());
    assert_eq!(g.apply(Action::Nominate(1)), Err(IllegalAction::GameOver));
}

#[test]
fn forced_default_is_first_eligible_nominee() {
    let mut g = GameState::test_game(LAYOUT, vec![L, L, L], 0);
    let def = g.forced_default().unwrap();
    match def {
        Action::Nominate(seat) => {
            let elig = match g.current_decision().unwrap().kind {
                DecisionKind::Nominate { eligible, .. } => eligible,
                _ => unreachable!(),
            };
            assert_eq!(seat, elig[0]);
        }
        other => panic!("expected nominate default, got {other:?}"),
    }
}
