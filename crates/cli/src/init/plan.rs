//! Dry-run plan rendering. Prints the resolved [`Config`] and every mutation it
//! implies as a table, *before* anything is written. `appctl init` prints this
//! for `--dry-run` and again (for confirmation) before a real apply.

use super::config::{Config, Surface};
use comfy_table::{presets::UTF8_FULL, Cell, Color, Table};
use std::path::Path;

/// Render the full plan (settings + rename + prune) as a printable string.
pub fn render(config: &Config, root: &Path) -> String {
    let mut out = String::new();
    out.push_str(&render_settings(config));
    out.push('\n');
    out.push_str(&render_rename(config));
    out.push('\n');
    out.push_str(&render_prune(config, root));
    out
}

fn render_settings(config: &Config) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec![Cell::new("Setting"), Cell::new("Value")]);

    table.add_row(vec!["profile", config.profile.as_str()]);
    table.add_row(vec!["project name", &config.project_name]);
    table.add_row(vec!["cli name", &config.cli_name]);
    table.add_row(vec!["org", &config.org]);

    let surfaces: Vec<&str> = config.surfaces.iter().map(|s| s.as_str()).collect();
    table.add_row(vec!["surfaces", &surfaces.join(", ")]);
    table.add_row(vec![
        Cell::new("cli diagnostics"),
        yes_no(config.has_surface(Surface::Cli)),
    ]);
    table.add_row(vec![
        Cell::new("http api"),
        yes_no(config.has_surface(Surface::HttpApi)),
    ]);
    table.add_row(vec![Cell::new("frontend"), yes_no(config.frontend)]);
    table.add_row(vec![Cell::new("docs site"), yes_no(config.docs)]);
    table.add_row(vec![Cell::new("dockerfile"), yes_no(config.docker)]);

    format!("Resolved configuration:\n{table}\n")
}

fn render_rename(config: &Config) -> String {
    let rules = config.rename_rules();
    if rules.is_empty() {
        return "Rename: nothing to rename (names unchanged from the template).\n".to_string();
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec![Cell::new("Replace"), Cell::new("With")]);
    for (from, to) in rules {
        table.add_row(vec![from, to]);
    }
    format!("Rename (applied across the source tree):\n{table}\n")
}

fn render_prune(config: &Config, root: &Path) -> String {
    let ops = config.prune_ops(root);
    if ops.is_empty() {
        return "Prune: nothing to remove (every surface kept).\n".to_string();
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec![Cell::new("#"), Cell::new("Prune action")]);
    for (i, op) in ops.iter().enumerate() {
        table.add_row(vec![
            Cell::new(i + 1),
            Cell::new(op.describe()).fg(Color::Yellow),
        ]);
    }
    format!("Prune (removes the surfaces you turned off):\n{table}\n")
}

fn yes_no(v: bool) -> Cell {
    if v {
        Cell::new("yes").fg(Color::Green)
    } else {
        Cell::new("no").fg(Color::DarkGrey)
    }
}
