//! `sh_export_game_report` — render a stored game into a single
//! self-contained HTML observability page (transcript with discussion and
//! collapsible per-decision thoughts, belief heatmaps, reliability & cost).
//!
//! This is the native, token-free path for producing game reports: the
//! template lives in the repo (`crates/engine/templates/game_report.html`)
//! and the data comes straight from the Postgres store.

use crate::commands::{Command, CommandError, Expose};
use crate::context::Ctx;
use crate::register_command;
use crate::secret_hitler::store::{database_url, Store};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

const TEMPLATE: &str = include_str!("../../templates/game_report.html");

#[derive(Default)]
pub struct ShExportGameReport;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShExportGameReportInput {
    pub game_id: String,
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
        let url = database_url().ok_or_else(|| {
            CommandError::Unsupported("game reports need a configured DATABASE_URL".into())
        })?;
        let store = Store::connect(&url)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?;
        let record = store
            .get_game(&input.game_id)
            .await
            .map_err(|e| CommandError::Other(e.to_string()))?
            .ok_or_else(|| {
                CommandError::InvalidInput(format!("unknown game: {}", input.game_id))
            })?;

        // `</` must not appear inside the inline <script> payload.
        let data = serde_json::to_string(&record)
            .map_err(|e| CommandError::Other(e.to_string()))?
            .replace("</", "<\\/");
        let html = TEMPLATE.replace("__GAME_DATA__", &data);
        let bytes = html.len();

        if let Some(path) = &input.output_path {
            cx.fs().write_file(Path::new(path), html.as_bytes())?;
        }
        let include_html = input.include_html.unwrap_or(input.output_path.is_none());
        Ok(ShExportGameReportOutput {
            game_id: input.game_id,
            bytes,
            output_path: input.output_path,
            html: include_html.then_some(html),
        })
    }
}

register_command!(ShExportGameReport);
