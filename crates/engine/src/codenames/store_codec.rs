//! Row-codec and write-guard helpers for the Codenames store: the pure
//! functions that map the schema's text encodings onto the engine's enums, and
//! the length check that keeps a game's seat rows and metrics in lockstep.
//! Kept beside `store.rs` so the query layer stays free of decoding detail.

use super::store::StoreResult;
use super::types::{Role, Team};

pub(crate) fn role_from_str(s: &str) -> Option<Role> {
    match s {
        "spymaster" => Some(Role::Spymaster),
        "operative" => Some(Role::Operative),
        _ => None,
    }
}

pub(crate) fn team_from_side(s: &str) -> Option<Team> {
    match s {
        "starting" => Some(Team::A),
        "second" => Some(Team::B),
        _ => None,
    }
}

/// A stored encoding that no longer maps onto the engine's enums is a decode
/// failure, not an absent row.
fn decode_error(column: &str, value: &str, expected: &str) -> sqlx::Error {
    sqlx::Error::Decode(
        format!("codenames {column} {value:?} is not a known encoding (expected {expected})")
            .into(),
    )
}

pub(crate) fn decode_role(s: &str) -> StoreResult<Role> {
    role_from_str(s).ok_or_else(|| decode_error("role", s, "spymaster | operative"))
}

pub(crate) fn decode_side(s: &str) -> StoreResult<Team> {
    team_from_side(s).ok_or_else(|| decode_error("side", s, "starting | second"))
}

/// Seat rows and their metrics are written in lockstep; a length mismatch is a
/// caller bug that must not reach the database half-applied.
pub(crate) fn check_metrics_len(seats: usize, metrics: usize) -> StoreResult<()> {
    if seats != metrics {
        return Err(sqlx::Error::Protocol(format!(
            "codenames game has {seats} seat(s) but {metrics} metric row(s): refusing to persist \
             a game with incomplete seat metrics"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::rating::side_name;

    /// The text encodings the schema stores must round-trip, or a rating row
    /// would come back as a different entity than it went in as.
    #[test]
    fn stored_enum_encodings_round_trip() {
        for role in [Role::Spymaster, Role::Operative] {
            assert_eq!(role_from_str(role.as_str()), Some(role));
        }
        for team in [Team::A, Team::B] {
            assert_eq!(team_from_side(side_name(team)), Some(team));
        }
        assert_eq!(role_from_str("liberal"), None);
        assert_eq!(team_from_side("a"), None);
    }

    /// Drift in a stored encoding surfaces as a decode error; silently skipping
    /// the row would look like a model that simply has no rating.
    #[test]
    fn unknown_stored_encodings_fail_to_decode() {
        assert_eq!(
            decode_role("spymaster").expect("known role"),
            Role::Spymaster
        );
        assert_eq!(decode_side("second").expect("known side"), Team::B);

        let err = decode_role("liberal")
            .expect_err("unknown role")
            .to_string();
        assert!(err.contains("role") && err.contains("liberal"), "{err}");
        let err = decode_side("a").expect_err("unknown side").to_string();
        assert!(
            err.contains("side") && err.contains("starting | second"),
            "{err}"
        );
    }

    /// A game is persisted with metrics for every seat or not at all.
    #[test]
    fn metrics_must_cover_every_seat_before_a_game_is_persisted() {
        assert!(check_metrics_len(4, 4).is_ok());
        for (seats, metrics) in [(4, 3), (4, 0), (3, 4)] {
            let err = check_metrics_len(seats, metrics)
                .expect_err("length mismatch")
                .to_string();
            assert!(err.contains("incomplete seat metrics"), "{err}");
        }
    }
}
