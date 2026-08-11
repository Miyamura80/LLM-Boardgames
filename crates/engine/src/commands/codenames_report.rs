//! `codenames_export_game_report` — render a stored Codenames game into a
//! single self-contained HTML observability page (replayable 5×5 grid with the
//! spymaster key, clue-by-clue transcript with guess outcomes and thoughts,
//! seat reliability & cost).
//!
//! The Codenames sibling of `catan_export_game_report`: the template lives in
//! the repo (`crates/engine/templates/codenames_game_report.html`) and the
//! record can come from the Postgres store (`game_id`) or a `GameRecord` JSON
//! file (`record_path`) for a DB-free artifact.
//!
//! The key card is engine state and never appears in a record (see
//! `codenames::events`), so it is reconstructed from the record's seed against
//! the configured wordlist exactly as scoring does. A record drawn from a
//! different pool simply renders without the full key — the reveals still
//! carry every identity the game disclosed.

use crate::codenames::board::KeyCard;
use crate::codenames::metrics::key_card_for;
use crate::codenames::runner::{wordlist_from_config, GameRecord};
use crate::commands::codenames_match::open_codenames_store;
use crate::commands::{Command, CommandError, Expose};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

const TEMPLATE: &str = include_str!("../../templates/codenames_game_report.html");

/// Render a complete Codenames game record into the self-contained HTML report.
/// `key` is the reconstructed key card when it is available; `None` renders the
/// key section from the identities the transcript revealed.
pub(crate) fn render_codenames_game_report(
    record: &GameRecord,
    key: Option<&KeyCard>,
) -> Result<String, CommandError> {
    let rendered: Vec<String> = record.events.iter().map(|r| r.event.render()).collect();
    // `</` must not appear inside the inline <script> payload.
    let payload = |v: serde_json::Result<serde_json::Value>| -> Result<String, CommandError> {
        let v = v.map_err(|e| CommandError::Other(e.to_string()))?;
        serde_json::to_string(&v)
            .map(|s| s.replace("</", "<\\/"))
            .map_err(|e| CommandError::Other(e.to_string()))
    };
    let data = payload(serde_json::to_value(record))?;
    let lines = payload(serde_json::to_value(&rendered))?;
    let key_json = payload(serde_json::to_value(key))?;

    // Split at the markers instead of sequential global replaces: model-authored
    // text inside the payloads could itself contain a marker.
    let (before, rest) = TEMPLATE
        .split_once("__GAME_DATA__")
        .ok_or_else(|| CommandError::Other("template missing __GAME_DATA__".into()))?;
    let (between, rest) = rest
        .split_once("__RENDERED__")
        .ok_or_else(|| CommandError::Other("template missing __RENDERED__".into()))?;
    let (between2, after) = rest
        .split_once("__KEY_CARD__")
        .ok_or_else(|| CommandError::Other("template missing __KEY_CARD__".into()))?;
    Ok(format!(
        "{before}{data}{between}{lines}{between2}{key_json}{after}"
    ))
}

/// The key card the report shows, when the configured wordlist is the one the
/// game was drawn from. A mismatch (or an unreadable wordlist) is not an error:
/// the report falls back to the revealed identities.
fn reconstruct_key(record: &GameRecord) -> Option<KeyCard> {
    let cfg = app_config::get_config();
    let wordlist = wordlist_from_config(&cfg.codenames).ok()?;
    key_card_for(record, &wordlist)
}

#[derive(Default)]
pub struct CodenamesExportGameReport;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodenamesExportGameReportInput {
    /// A stored game id (needs Postgres). Provide this or `record_path`.
    pub game_id: Option<String>,
    /// Path to a `GameRecord` JSON to render directly, bypassing the store —
    /// the reproducible "fixed JSON" path (no database required).
    pub record_path: Option<String>,
    /// Where to write the HTML file. Omit to only return the HTML inline.
    pub output_path: Option<String>,
    /// Include the rendered HTML in the response (default true when no
    /// output_path is given).
    pub include_html: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodenamesExportGameReportOutput {
    pub game_id: String,
    pub bytes: usize,
    pub output_path: Option<String>,
    pub html: Option<String>,
}

#[async_trait]
impl Command for CodenamesExportGameReport {
    type Input = CodenamesExportGameReportInput;
    type Output = CodenamesExportGameReportOutput;

    fn name(&self) -> &'static str {
        "codenames_export_game_report"
    }
    fn description(&self) -> &'static str {
        "Render a stored Codenames game into a self-contained HTML observability report (replayable grid, spymaster key, clue-by-clue transcript, thoughts, reliability, cost)."
    }
    fn expose(&self) -> Expose {
        // Writes caller-supplied paths — keep it off the unauthenticated API.
        Expose::cli_only()
    }

    async fn run(
        &self,
        input: CodenamesExportGameReportInput,
        cx: &Ctx<'_>,
    ) -> Result<Self::Output, CommandError> {
        // game_id and record_path are mutually exclusive — reject both rather
        // than silently ignoring game_id and rendering the wrong game.
        let record = match (&input.game_id, &input.record_path) {
            (Some(_), Some(_)) => {
                return Err(CommandError::InvalidInput(
                    "provide either game_id (stored) or record_path (a GameRecord JSON file), not both"
                        .into(),
                ));
            }
            (None, Some(path)) => {
                let bytes = cx.fs().read_file(Path::new(path))?;
                serde_json::from_slice::<GameRecord>(&bytes).map_err(|e| {
                    CommandError::InvalidInput(format!("invalid GameRecord JSON in {path}: {e}"))
                })?
            }
            (Some(id), None) => open_codenames_store()
                .await?
                .get_game(id)
                .await
                .map_err(|e| CommandError::Other(e.to_string()))?
                .ok_or_else(|| CommandError::InvalidInput(format!("unknown game: {id}")))?,
            (None, None) => {
                return Err(CommandError::InvalidInput(
                    "provide either game_id (stored) or record_path (a GameRecord JSON file)"
                        .into(),
                ));
            }
        };

        let game_id = record.game_id.clone();
        let key = reconstruct_key(&record);
        let html = render_codenames_game_report(&record, key.as_ref())?;
        let bytes = html.len();

        if let Some(path) = &input.output_path {
            cx.fs().write_file(Path::new(path), html.as_bytes())?;
        }
        let include_html = input.include_html.unwrap_or(input.output_path.is_none());
        Ok(CodenamesExportGameReportOutput {
            game_id,
            bytes,
            output_path: input.output_path,
            html: include_html.then_some(html),
        })
    }
}

register_command!(CodenamesExportGameReport);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codenames::events::CodenamesEvent;
    use crate::codenames::wordlist::Wordlist;

    const FIXTURE: &str = include_str!("../../fixtures/codenames_bots.json");

    fn fixture_record() -> GameRecord {
        serde_json::from_str(FIXTURE).expect("fixture is a GameRecord")
    }

    /// The fixture must render into a page that is self-contained (no network
    /// fetches), carries every board word, and shows the clue history.
    #[test]
    fn fixture_renders_a_complete_offline_report() {
        let record = fixture_record();
        let key = key_card_for(&record, &Wordlist::default_embedded());
        assert!(
            key.is_some(),
            "the vendored wordlist must still reproduce the fixture's key card"
        );
        let html = render_codenames_game_report(&record, key.as_ref()).expect("renders");

        // Every marker was substituted.
        for marker in ["__GAME_DATA__", "__RENDERED__", "__KEY_CARD__"] {
            assert!(!html.contains(marker), "{marker} left unrendered");
        }
        assert!(html.starts_with("<!doctype html>") && html.trim_end().ends_with("</html>"));
        // Offline: no external subresources or requests.
        for probe in [
            "http://",
            "https://",
            "src=\"//",
            "fetch(",
            "XMLHttpRequest",
        ] {
            assert!(!html.contains(probe), "template reaches out via {probe}");
        }

        // The 25 words and every clue reached the page.
        let words = record
            .events
            .iter()
            .find_map(|r| match &r.event {
                CodenamesEvent::BoardLaid { words } => Some(words.clone()),
                _ => None,
            })
            .expect("fixture lays a board");
        assert_eq!(words.len(), 25);
        for w in &words {
            assert!(html.contains(w.as_str()), "board word {w} missing");
        }
        let clues: Vec<_> = record
            .events
            .iter()
            .filter_map(|r| match &r.event {
                CodenamesEvent::ClueGiven { clue, .. } => Some(clue.word.clone()),
                _ => None,
            })
            .collect();
        assert!(clues.len() >= 2, "fixture should carry a clue history");
        for c in &clues {
            assert!(html.contains(c.as_str()), "clue {c} missing");
        }

        // Headline facts and the sections the report promises.
        assert!(html.contains(&record.game_id));
        assert!(html.contains("Spymaster key"));
        assert!(html.contains("Clue by clue"));
        assert!(html.contains("Seats, reliability"));
        for seat in &record.seats {
            assert!(html.contains(seat.model_id.as_str()));
        }
    }

    /// A record whose pool cannot be identified still renders — the key falls
    /// back to the identities the transcript revealed.
    #[test]
    fn renders_without_a_reconstructed_key() {
        let html = render_codenames_game_report(&fixture_record(), None).expect("renders");
        assert!(html.contains("const KEY = null;"));
        assert!(!html.contains("__KEY_CARD__"));
    }
}
