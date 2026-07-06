//! Rules suite: executive powers, veto, win conditions, reshuffle,
//! determinism, and randomized invariants.

mod common;

use common::*;
use engine::secret_hitler::actions::Action;
use engine::secret_hitler::events::{GameEvent, Visibility};
use engine::secret_hitler::state::{GameState, Phase};
use engine::secret_hitler::types::*;

#[test]
fn powers_fire_in_board_order() {
    let mut state = scripted(&[Party::Fascist; 17]);
    pass_government(&mut state, 1, Party::Fascist);
    assert!(
        matches!(state.phase, Phase::Nomination),
        "1st policy: no power"
    );

    pass_government(&mut state, 2, Party::Fascist);
    assert!(matches!(
        state.phase,
        Phase::ExecutiveAction {
            power: Power::InvestigateLoyalty
        }
    ));
    use_power(&mut state, 0);

    pass_government(&mut state, 3, Party::Fascist);
    assert!(matches!(
        state.phase,
        Phase::ExecutiveAction {
            power: Power::SpecialElection
        }
    ));
    let next = state.next_alive_after(state.presidency);
    use_power(&mut state, next); // keeps rotation order unchanged

    let c = safe_chancellor(&state);
    pass_government(&mut state, c, Party::Fascist);
    assert!(matches!(
        state.phase,
        Phase::ExecutiveAction {
            power: Power::Execution
        }
    ));
}

#[test]
fn investigation_is_private_reads_party_and_cannot_repeat() {
    let mut state = scripted(&[Party::Fascist; 17]);
    pass_government(&mut state, 1, Party::Fascist);
    pass_government(&mut state, 2, Party::Fascist);
    let president = state.presidency;

    // Investigate Hitler (seat 6): the card must read Fascist, never Hitler.
    use_power(&mut state, 6);
    let result = state
        .events
        .iter()
        .find(|r| matches!(r.event, GameEvent::InvestigationResult { .. }))
        .unwrap();
    assert_eq!(result.visibility, Visibility::Private(president));
    let GameEvent::InvestigationResult { target, party } = &result.event else {
        unreachable!()
    };
    assert_eq!((*target, *party), (6, Party::Fascist));

    // Only the investigator saw the result.
    for seat in (0..7).filter(|&s| s != president) {
        assert!(!state
            .observe(seat)
            .history
            .iter()
            .any(|r| matches!(r.event, GameEvent::InvestigationResult { .. })));
    }
    // A player may never be investigated twice.
    assert!(!state.power_targets(Power::InvestigateLoyalty).contains(&6));
}

#[test]
fn special_election_returns_to_normal_order() {
    let mut state = scripted(&[Party::Fascist; 17]);
    pass_government(&mut state, 1, Party::Fascist); // P0 C1
    pass_government(&mut state, 2, Party::Fascist); // P1 C2 → investigate
    use_power(&mut state, 0);
    pass_government(&mut state, 3, Party::Fascist); // P2 C3 → special election
    let caller = state.presidency;
    assert_eq!(caller, 2);

    use_power(&mut state, 5); // appoint seat 5 President
    assert_eq!(state.presidency, 5);

    // The special round: P5 forms a government (no power on this policy? 4th
    // fascist policy → Execution fires; resolve it on a Liberal).
    let c = safe_chancellor(&state);
    pass_government(&mut state, c, Party::Fascist);
    if let Phase::ExecutiveAction { .. } = state.phase {
        use_power(&mut state, 1);
    }
    // Normal order resumes left of the caller (seat 3).
    assert_eq!(state.presidency, 3, "rotation must resume after the caller");
}

#[test]
fn failed_special_election_still_returns_to_normal_order() {
    let mut state = scripted(&[Party::Fascist; 17]);
    pass_government(&mut state, 1, Party::Fascist);
    pass_government(&mut state, 2, Party::Fascist);
    use_power(&mut state, 0);
    pass_government(&mut state, 3, Party::Fascist);
    use_power(&mut state, 5);

    // The special government is voted down.
    let nominee = safe_chancellor(&state);
    state
        .apply(5, Action::Nominate { target: nominee })
        .unwrap();
    vote(&mut state, &[]);
    assert_eq!(state.election_tracker, 1);
    assert_eq!(
        state.presidency, 3,
        "rotation resumes even when the special election fails"
    );
}

#[test]
fn executing_hitler_wins_for_liberals_other_executions_stay_hidden() {
    let mut state = scripted(&[Party::Fascist; 17]);
    reach_fascist_policies(&mut state, 4); // grants Execution
                                           // reach_… resolved the 4th-policy execution already on a non-Hitler; drive
                                           // to the 5th policy for a second Execution.
    reach_fascist_policies(&mut state, 5);
    assert!(matches!(state.phase, Phase::Nomination));

    // Roles of previously executed players were not revealed.
    for r in &state.events {
        if let GameEvent::Executed { was_hitler, .. } = &r.event {
            assert!(!was_hitler);
        }
    }

    // Now a fresh game where the execution hits Hitler.
    let mut state = scripted(&[Party::Fascist; 17]);
    reach_fascist_policies(&mut state, 3);
    let c = safe_chancellor(&state);
    pass_government(&mut state, c, Party::Fascist);
    assert!(matches!(
        state.phase,
        Phase::ExecutiveAction {
            power: Power::Execution
        }
    ));
    use_power(&mut state, 6);
    assert_eq!(state.winner, Some(WinCondition::HitlerExecuted));
    assert!(state.is_over());
}

#[test]
fn veto_unlocks_after_5th_fascist_policy_and_follows_the_flow() {
    let mut state = scripted(&[Party::Fascist; 17]);
    reach_fascist_policies(&mut state, 5);
    assert!(state.veto_unlocked());

    // Before 5 policies, ProposeVeto is illegal (checked on a fresh game).
    let mut early = scripted(&[Party::Fascist; 17]);
    elect(&mut early, 1);
    early
        .apply(
            0,
            Action::Discard {
                policy: Party::Fascist,
            },
        )
        .unwrap();
    assert!(early.apply(1, Action::ProposeVeto).is_err());

    // Veto approved: both tiles discarded, tracker advances, next government.
    let tracker_before = state.election_tracker;
    let round_before = state.round;
    let c = safe_chancellor(&state);
    elect(&mut state, c);
    let president = state.presidency;
    let chancellor = state.gov_chancellor.unwrap();
    state
        .apply(
            president,
            Action::Discard {
                policy: Party::Fascist,
            },
        )
        .unwrap();
    state.apply(chancellor, Action::ProposeVeto).unwrap();
    assert!(matches!(state.phase, Phase::VetoConsent { .. }));
    state
        .apply(president, Action::VetoConsent { approve: true })
        .unwrap();
    assert_eq!(state.election_tracker, tracker_before + 1);
    assert_eq!(state.fascist_policies, 5, "no policy enacted on a veto");
    assert!(state.round > round_before);

    // Veto declined: the chancellor must enact and cannot re-propose.
    let c = safe_chancellor(&state);
    elect(&mut state, c);
    let president = state.presidency;
    let chancellor = state.gov_chancellor.unwrap();
    state
        .apply(
            president,
            Action::Discard {
                policy: Party::Fascist,
            },
        )
        .unwrap();
    state.apply(chancellor, Action::ProposeVeto).unwrap();
    state
        .apply(president, Action::VetoConsent { approve: false })
        .unwrap();
    assert!(state.apply(chancellor, Action::ProposeVeto).is_err());
    state
        .apply(
            chancellor,
            Action::Enact {
                policy: Party::Fascist,
            },
        )
        .unwrap();
    assert_eq!(state.winner, Some(WinCondition::FascistPolicies));
}

#[test]
fn liberal_policies_win() {
    let mut state = scripted(&[Party::Liberal; 17]);
    for _ in 0..5 {
        let c = safe_chancellor(&state);
        pass_government(&mut state, c, Party::Liberal);
    }
    assert_eq!(state.winner, Some(WinCondition::LiberalPolicies));
}

#[test]
fn hitler_chancellor_wins_only_after_3_fascist_policies() {
    // With only 2 fascist policies, electing Hitler is safe.
    let mut state = scripted(&[Party::Fascist; 17]);
    reach_fascist_policies(&mut state, 2);
    elect(&mut state, 6);
    assert!(
        !state.is_over(),
        "Hitler as Chancellor before 3 policies is not a win"
    );

    // With 3, the moment Hitler is elected the Fascists win.
    let mut state = scripted(&[Party::Fascist; 17]);
    reach_fascist_policies(&mut state, 3);
    elect(&mut state, 6);
    assert_eq!(state.winner, Some(WinCondition::HitlerChancellor));
}

#[test]
fn deck_reshuffles_discards_when_below_3_and_conserves_tiles() {
    let mut state = scripted(&deck_f_then_l(11, 6));
    // Burn through governments; each consumes 3 tiles from a 17-tile deck.
    // Prefer enacting Liberal so no faction reaches a policy win.
    for _ in 0..7 {
        if state.is_over() {
            break;
        }
        let c = safe_chancellor(&state);
        elect(&mut state, c);
        let president = state.presidency;
        let Phase::LegislativePresident { tiles } = state.phase.clone() else {
            unreachable!()
        };
        let discard = if tiles.contains(&Party::Fascist) {
            Party::Fascist
        } else {
            Party::Liberal
        };
        state
            .apply(president, Action::Discard { policy: discard })
            .unwrap();
        let Phase::LegislativeChancellor { tiles, .. } = state.phase.clone() else {
            unreachable!()
        };
        let chancellor = state.gov_chancellor.unwrap();
        let enact = if tiles.contains(&Party::Liberal) {
            Party::Liberal
        } else {
            Party::Fascist
        };
        state
            .apply(chancellor, Action::Enact { policy: enact })
            .unwrap();
        if let Phase::ExecutiveAction { power } = state.phase {
            let t = state
                .power_targets(power)
                .into_iter()
                .find(|&s| s != 6)
                .unwrap();
            use_power(&mut state, t);
        }
        let on_board = (state.liberal_policies + state.fascist_policies) as usize;
        assert_eq!(state.deck.total() + on_board, 17, "tiles must be conserved");
    }
    // 6+ governments × 3 tiles > the 17-tile deck: a reshuffle must occur.
    assert!(state
        .events
        .iter()
        .any(|r| matches!(r.event, GameEvent::DeckReshuffled { .. })));
}

#[test]
fn same_seed_reproduces_the_game_bit_for_bit() {
    let run = |seed: u64| -> String {
        let mut state = GameState::new(seed);
        let mut guard = 0;
        while !state.is_over() {
            let decisions = state.pending_decisions();
            for d in decisions {
                let action = state.forced_default(&d);
                state.apply(d.seat(), action).unwrap();
            }
            guard += 1;
            assert!(guard < 10_000, "game did not terminate");
        }
        serde_json::to_string(&state.events).unwrap()
    };
    for seed in [1u64, 7, 42, 99] {
        assert_eq!(
            run(seed),
            run(seed),
            "seed {seed} must reproduce identically"
        );
    }
}
