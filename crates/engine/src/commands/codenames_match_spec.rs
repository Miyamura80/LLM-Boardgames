//! Input resolution and resume guards for the Codenames match commands: pool
//! lookup, the base per-game config, and the two checks that keep a resumed run
//! comparable with the games already stored under it. Kept beside
//! `codenames_match.rs` so the command surface stays readable.

use crate::codenames::runner::{rules_from_config, wordlist_from_config, AgentSpec, GameConfig};
use crate::commands::CommandError;

pub(crate) fn resolve_pool(
    cfg: &app_config::AppConfig,
    name: &str,
) -> Result<Vec<AgentSpec>, CommandError> {
    let pool = cfg.codenames.pools.get(name).ok_or_else(|| {
        CommandError::InvalidInput(format!(
            "unknown codenames pool '{name}' (configured: {:?})",
            cfg.codenames.pools.keys().collect::<Vec<_>>()
        ))
    })?;
    pool.iter()
        .map(|a| AgentSpec::from_config(a).map_err(CommandError::InvalidInput))
        .collect()
}

/// `create_run` is `ON CONFLICT DO NOTHING`, so losing a race on an explicit
/// `run_id` is silent: this caller would carry on with *its* spec while the row
/// (and every game keyed to it) belongs to someone else's. Re-reading the row
/// and comparing turns that into a clear error before any game is played.
pub(crate) fn ensure_stored_spec_matches(
    run_id: &str,
    local: &serde_json::Value,
    stored: &serde_json::Value,
) -> Result<(), CommandError> {
    if local == stored {
        return Ok(());
    }
    Err(CommandError::InvalidInput(format!(
        "run '{run_id}' already exists with a different specification (another caller created it \
         concurrently, or the id is being reused): resume it with the same request, or choose a \
         new run_id"
    )))
}

/// A resume must play under the config the run was created with. Live config is
/// re-read on every call, so without this an edit to the wordlist, the
/// clue-word cap, or the retry budget would silently mix games played under two
/// rule sets into one rating.
pub(crate) fn ensure_fingerprint_matches(
    run_id: &str,
    stored: Option<&str>,
    live: &str,
) -> Result<(), CommandError> {
    match stored {
        // Rows written before fingerprinting resume unchecked rather than
        // becoming unresumable.
        None => Ok(()),
        Some(fp) if fp == live => Ok(()),
        Some(fp) => Err(CommandError::InvalidInput(format!(
            "run '{run_id}' was created under a different codenames configuration \
             (fingerprint {fp}, now {live}): the wordlist, clue_word_max_len, or retry_budget \
             changed, and resuming would mix incomparable games — restore the configuration or \
             start a new run"
        ))),
    }
}

/// The base per-game config a match plays under (rules knobs + the pool every
/// board is drawn from). Resolved once per call so a broken `wordlist_path`,
/// `vectors_path`, or clue-word cap fails before any game starts.
pub(crate) fn base_game_config(cfg: &app_config::AppConfig) -> Result<GameConfig, CommandError> {
    Ok(GameConfig {
        retry_budget: cfg.codenames.retry_budget,
        rules: rules_from_config(&cfg.codenames).map_err(CommandError::InvalidInput)?,
        wordlist: wordlist_from_config(&cfg.codenames).map_err(CommandError::InvalidInput)?,
        ..GameConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::runner::{AgentFactory, CodenamesAgentKind, MatchSpec, StoredSpec};
    use crate::codenames::types::MIN_CLUE_WORD_MAX_LEN;

    /// The shipped defaults must describe a run that can actually start. The
    /// default pool named the `codenames-embedding` anchor while `vectors_path`
    /// ships null, so the default controlled match failed at seat construction
    /// before any run row existed.
    #[test]
    fn the_shipped_default_pool_builds_under_the_shipped_config() {
        let cfg = app_config::get_config();
        let pool = resolve_pool(cfg, "pool-a").expect("the shipped pool resolves");
        assert_eq!(pool.len(), 3, "controlled mode seats exactly three anchors");
        let factory = AgentFactory::from_app_config(cfg).expect("the shipped factory builds");
        for seat in &pool {
            if let Err(e) = factory.build(seat, 0).map(drop) {
                panic!("shipped anchor '{}' cannot be built: {e}", seat.name);
            }
        }
        // Anchors are told apart by name even when two share an agent kind.
        let mut names: Vec<&str> = pool.iter().map(|a| a.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), pool.len(), "anchor names must be distinct");
    }

    fn spec_with_boards(boards: u32) -> MatchSpec {
        MatchSpec::Controlled {
            candidate: AgentSpec::llm("cand"),
            pool_name: "pool-a".into(),
            pool: (0..3)
                .map(|_| AgentSpec::bot(CodenamesAgentKind::Random))
                .collect(),
            boards,
            k: 1,
            match_seed: 7,
        }
    }

    fn spec() -> MatchSpec {
        spec_with_boards(2)
    }

    fn stored(spec: MatchSpec, fingerprint: &str) -> serde_json::Value {
        serde_json::to_value(StoredSpec {
            spec,
            config_fingerprint: Some(fingerprint.into()),
        })
        .expect("spec serializes")
    }

    /// The fingerprint rides in the spec's JSON, and a plain `MatchSpec` — what
    /// `finalize_match` parses to rebuild the planned distribution — still
    /// reads out of it unchanged.
    #[test]
    fn the_stored_spec_carries_the_fingerprint_without_changing_its_shape() {
        let value = stored(spec(), "abc");
        assert_eq!(value["mode"], "controlled");
        assert_eq!(value["config_fingerprint"], "abc");
        let bare: MatchSpec = serde_json::from_value(value.clone()).expect("still a MatchSpec");
        assert_eq!(bare.schedule().unwrap().len(), 2 * 4);

        let round_trip: StoredSpec = serde_json::from_value(value).expect("round trips");
        assert_eq!(round_trip.config_fingerprint.as_deref(), Some("abc"));

        // A row written before fingerprinting still parses, and resumes.
        let legacy = serde_json::to_value(spec()).expect("spec serializes");
        let legacy: StoredSpec = serde_json::from_value(legacy).expect("legacy row parses");
        assert!(legacy.config_fingerprint.is_none());
        assert!(ensure_fingerprint_matches("r", legacy.config_fingerprint.as_deref(), "x").is_ok());
    }

    /// Losing the `ON CONFLICT DO NOTHING` race is silent at the SQL level, so
    /// the re-read has to be the thing that catches it.
    #[test]
    fn a_concurrently_created_run_with_a_different_spec_is_rejected() {
        let mine = stored(spec(), "abc");
        assert!(ensure_stored_spec_matches("r", &mine, &mine.clone()).is_ok());

        let theirs = stored(spec_with_boards(5), "abc");
        let err = match ensure_stored_spec_matches("r", &mine, &theirs) {
            Err(CommandError::InvalidInput(e)) => e,
            other => panic!("expected invalid input, got {other:?}"),
        };
        assert!(err.contains("different specification"), "{err}");
    }

    /// A resume under an edited config is refused rather than quietly mixing
    /// two rule sets into one rating.
    #[test]
    fn a_resume_under_a_changed_config_is_rejected() {
        let base = GameConfig::default();
        let live = base.fingerprint();
        assert!(ensure_fingerprint_matches("r", Some(&live), &live).is_ok());

        let mut changed = base.clone();
        changed.rules.clue_word_max_len = MIN_CLUE_WORD_MAX_LEN;
        assert_ne!(changed.fingerprint(), live, "the cap is fingerprinted");
        let mut changed = base.clone();
        changed.retry_budget += 1;
        assert_ne!(
            changed.fingerprint(),
            live,
            "the retry budget is fingerprinted"
        );
        let mut changed = base.clone();
        changed.wordlist = crate::codenames::testkit::test_wordlist();
        assert_ne!(changed.fingerprint(), live, "the wordlist is fingerprinted");

        let err = match ensure_fingerprint_matches("r", Some(&changed.fingerprint()), &live) {
            Err(CommandError::InvalidInput(e)) => e,
            other => panic!("expected invalid input, got {other:?}"),
        };
        assert!(err.contains("different codenames configuration"), "{err}");
        // Per-game fields are not part of it: they differ by design in a run.
        let per_game = GameConfig {
            game_id: "other".into(),
            seed: 99,
            schedule_label: "arena/g3".into(),
            ..base.clone()
        };
        assert_eq!(per_game.fingerprint(), live);
    }
}
