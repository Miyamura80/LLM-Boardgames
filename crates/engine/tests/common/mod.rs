//! Shared helpers for the Secret Hitler rules test suite.

use engine::secret_hitler::actions::Action;
use engine::secret_hitler::state::{GameState, Phase};
use engine::secret_hitler::types::{Party, Role, Seat};

/// Canonical scripted layout: seats 0–3 Liberal, 4–5 regular Fascist, 6 Hitler.
pub const ROLES: [Role; 7] = [
    Role::Liberal,
    Role::Liberal,
    Role::Liberal,
    Role::Liberal,
    Role::Fascist,
    Role::Fascist,
    Role::Hitler,
];

pub fn scripted(deck_top_first: &[Party]) -> GameState {
    GameState::new_scripted(42, ROLES, deck_top_first)
}

/// A deck of `n` Fascist tiles followed by `m` Liberal tiles.
pub fn deck_f_then_l(f: usize, l: usize) -> Vec<Party> {
    let mut d = vec![Party::Fascist; f];
    d.extend(vec![Party::Liberal; l]);
    d
}

/// Every living seat votes; `ja_seats` vote Ja, the rest Nein.
pub fn vote(state: &mut GameState, ja_seats: &[Seat]) {
    for s in state.alive_seats() {
        state
            .apply(
                s,
                Action::Vote {
                    ja: ja_seats.contains(&s),
                },
            )
            .unwrap();
    }
}

/// Everyone votes Ja.
pub fn vote_unanimous(state: &mut GameState) {
    let alive = state.alive_seats();
    vote(state, &alive);
}

/// Nominate `chancellor`, elect unanimously.
pub fn elect(state: &mut GameState, chancellor: Seat) {
    let president = state.presidency;
    state
        .apply(president, Action::Nominate { target: chancellor })
        .unwrap();
    vote_unanimous(state);
}

/// Run a full legislative session enacting `policy`, discarding whatever keeps
/// that possible. Panics if the drawn tiles cannot produce `policy`.
pub fn legislate(state: &mut GameState, policy: Party) {
    let president = state.presidency;
    let Phase::LegislativePresident { tiles } = state.phase.clone() else {
        panic!("expected legislative-president, got {:?}", state.phase);
    };
    let other = match policy {
        Party::Liberal => Party::Fascist,
        Party::Fascist => Party::Liberal,
    };
    let discard = if tiles.contains(&other) {
        other
    } else {
        policy
    };
    state
        .apply(president, Action::Discard { policy: discard })
        .unwrap();

    let chancellor = state.gov_chancellor.unwrap();
    state.apply(chancellor, Action::Enact { policy }).unwrap();
}

/// Elect `chancellor` and enact `policy` in one go.
pub fn pass_government(state: &mut GameState, chancellor: Seat, policy: Party) {
    elect(state, chancellor);
    legislate(state, policy);
}

/// First eligible chancellor that isn't Hitler (seat 6 in the scripted layout).
pub fn safe_chancellor(state: &GameState) -> Seat {
    state
        .eligible_chancellors()
        .into_iter()
        .find(|&s| s != 6)
        .expect("an eligible non-Hitler chancellor exists")
}

/// Resolve a pending executive power on `target`.
pub fn use_power(state: &mut GameState, target: Seat) {
    let president = state.presidency;
    state.apply(president, Action::UsePower { target }).unwrap();
}

/// Drive fascist policies onto the board until `count`, resolving each granted
/// power on the first legal non-Hitler target so the game cannot end early.
pub fn reach_fascist_policies(state: &mut GameState, count: u8) {
    while state.fascist_policies < count && !state.is_over() {
        let chancellor = safe_chancellor(state);
        pass_government(state, chancellor, Party::Fascist);
        if let Phase::ExecutiveAction { power } = state.phase {
            let target = state
                .power_targets(power)
                .into_iter()
                .find(|&s| s != 6)
                .expect("a non-Hitler power target exists");
            use_power(state, target);
        }
    }
}
