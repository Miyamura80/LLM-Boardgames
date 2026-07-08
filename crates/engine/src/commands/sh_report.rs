//! `sh_export_game_report` — render a stored game into a single
//! self-contained HTML observability page (transcript with discussion and
//! collapsible per-decision thoughts, belief heatmaps, reliability & cost).
//!
//! This is the native, token-free path for producing game reports: the
//! template lives in the repo (`crates/engine/templates/game_report.html`).
//! [`render_game_report`] is the shared renderer — this command feeds it a
//! record from the Postgres store, while `sh_play_game { report_path }` feeds it
//! an in-memory record for a DB-free artifact.

use crate::commands::sh_match::open_store;
use crate::commands::{Command, CommandError, Expose};
use crate::context::Ctx;
use crate::register_command;
use crate::secret_hitler::runner::record::GameRecord;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

const TEMPLATE: &str = include_str!("../../templates/game_report.html");

/// Render a complete game record into the self-contained HTML report. Shared by
/// the store-backed `sh_export_game_report` command and the DB-free
/// `sh_play_game { report_path }` path, so both emit byte-identical artifacts.
pub(crate) fn render_game_report(record: &GameRecord) -> Result<String, CommandError> {
    let rendered: Vec<String> = record
        .events
        .iter()
        .map(|r| r.event.render_omniscient())
        .collect();
    // `</` must not appear inside the inline <script> payload.
    let escape = |v: &serde_json::Value| serde_json::to_string(v).map(|s| s.replace("</", "<\\/"));
    let data =
        escape(&serde_json::to_value(record).map_err(|e| CommandError::Other(e.to_string()))?)
            .map_err(|e| CommandError::Other(e.to_string()))?;
    let lines =
        escape(&serde_json::to_value(&rendered).map_err(|e| CommandError::Other(e.to_string()))?)
            .map_err(|e| CommandError::Other(e.to_string()))?;
    // Split at the markers instead of sequential global replaces: model-authored
    // text inside the payloads could itself contain a marker.
    let (before, rest) = TEMPLATE
        .split_once("__GAME_DATA__")
        .ok_or_else(|| CommandError::Other("template missing __GAME_DATA__".into()))?;
    let (between, after) = rest
        .split_once("__RENDERED__")
        .ok_or_else(|| CommandError::Other("template missing __RENDERED__".into()))?;
    Ok(format!("{before}{data}{between}{lines}{after}"))
}

#[derive(Default)]
pub struct ShExportGameReport;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShExportGameReportInput {
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
pub struct ShExportGameReportOutput {
    pub game_id: String,
    pub bytes: usize,
    pub output_path: Option<String>,
    pub html: Option<String>,
}

#[async_trait]
impl Command for ShExportGameReport {
    type Input = ShExportGameReportInput;
    type Output = ShExportGameReportOutput;

    fn name(&self) -> &'static str {
        "sh_export_game_report"
    }
    fn description(&self) -> &'static str {
        "Render a stored game into a self-contained HTML observability report (transcript, thoughts, belief heatmaps, reliability, cost)."
    }
    fn expose(&self) -> Expose {
        // Writes caller-supplied paths — keep it off the unauthenticated API.
        Expose::cli_only()
    }

    async fn run(
        &self,
        input: ShExportGameReportInput,
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
            (Some(id), None) => open_store()
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
        let html = render_game_report(&record)?;
        let bytes = html.len();

        if let Some(path) = &input.output_path {
            cx.fs().write_file(Path::new(path), html.as_bytes())?;
        }
        let include_html = input.include_html.unwrap_or(input.output_path.is_none());
        Ok(ShExportGameReportOutput {
            game_id,
            bytes,
            output_path: input.output_path,
            html: include_html.then_some(html),
        })
    }
}

register_command!(ShExportGameReport);
