//! `catan_export_game_report` — render a stored Catan game into a single
//! self-contained HTML observability page (replayable board with step slider,
//! turn-grouped transcript with collapsible per-decision thoughts, trade-flow
//! matrix, reliability & cost).
//!
//! The Catan sibling of `sh_export_game_report`: the template lives in the
//! repo (`crates/engine/templates/catan_game_report.html`) and the record can
//! come from the Postgres store (`game_id`) or a `GameRecord` JSON file
//! (`record_path`) for a DB-free artifact.

use crate::catan::runner::GameRecord;
use crate::commands::catan_match::open_catan_store;
use crate::commands::{Command, CommandError, Expose};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

const TEMPLATE: &str = include_str!("../../templates/catan_game_report.html");

/// Render a complete Catan game record into the self-contained HTML report.
pub(crate) fn render_catan_game_report(record: &GameRecord) -> Result<String, CommandError> {
    let rendered: Vec<String> = record.events.iter().map(|r| r.event.render()).collect();
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
pub struct CatanExportGameReport;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CatanExportGameReportInput {
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
pub struct CatanExportGameReportOutput {
    pub game_id: String,
    pub bytes: usize,
    pub output_path: Option<String>,
    pub html: Option<String>,
}

#[async_trait]
impl Command for CatanExportGameReport {
    type Input = CatanExportGameReportInput;
    type Output = CatanExportGameReportOutput;

    fn name(&self) -> &'static str {
        "catan_export_game_report"
    }
    fn description(&self) -> &'static str {
        "Render a stored Catan game into a self-contained HTML observability report (replayable board, transcript, thoughts, trade flow, reliability, cost)."
    }
    fn expose(&self) -> Expose {
        // Writes caller-supplied paths — keep it off the unauthenticated API.
        Expose::cli_only()
    }

    async fn run(
        &self,
        input: CatanExportGameReportInput,
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
            (Some(id), None) => open_catan_store()
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
        let html = render_catan_game_report(&record)?;
        let bytes = html.len();

        if let Some(path) = &input.output_path {
            cx.fs().write_file(Path::new(path), html.as_bytes())?;
        }
        let include_html = input.include_html.unwrap_or(input.output_path.is_none());
        Ok(CatanExportGameReportOutput {
            game_id,
            bytes,
            output_path: input.output_path,
            html: include_html.then_some(html),
        })
    }
}

register_command!(CatanExportGameReport);
