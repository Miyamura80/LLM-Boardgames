//! CLI diagnostic subcommand implementations (`doctor`, `call`, `probe`,
//! `run-scenario`) and their human/JSON output + artifact helpers.
//!
//! Split out of `main.rs` so the entrypoint stays focused on clap wiring. The
//! whole module is gated behind the `cli` feature, so a `server-only` prune
//! drops it wholesale.

use engine::types::*;
use engine::{AppContext, CommandRegistry, CommandResult, Ctx};
use std::path::PathBuf;
use std::time::{Duration, Instant};

// Subcommand implementations (CLI diagnostics)

pub(crate) async fn cmd_doctor(json: bool, out: Option<PathBuf>) {
    let result = engine::doctor::run_doctor();
    if let Some(ref path) = out {
        write_result_file(path, &result);
    }
    output_result(&result, json);
}

/// Parse a human timeout string (`"30s"`, `"5000ms"`, `"2m"`, or a bare number
/// of seconds) into a [`Duration`]. The `ms` suffix is checked before `s` so
/// `"500ms"` isn't misread as seconds.
fn parse_timeout(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let invalid = || format!("invalid timeout '{s}' (use e.g. '30s', '500ms', '2m')");
    // `try_from_secs_f64` (not `from_secs_f64`) so a negative, non-finite, or
    // overflowing value yields an `Err` rather than panicking the process.
    if let Some(ms) = s.strip_suffix("ms") {
        ms.trim()
            .parse::<u64>()
            .map(Duration::from_millis)
            .map_err(|_| invalid())
    } else if let Some(sec) = s.strip_suffix('s') {
        let v: f64 = sec.trim().parse().map_err(|_| invalid())?;
        Duration::try_from_secs_f64(v).map_err(|_| invalid())
    } else if let Some(min) = s.strip_suffix('m') {
        let v: f64 = min.trim().parse().map_err(|_| invalid())?;
        Duration::try_from_secs_f64(v * 60.0).map_err(|_| invalid())
    } else {
        s.parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|_| invalid())
    }
}

pub(crate) async fn cmd_call(
    cmd: &str,
    args_str: &str,
    json: bool,
    timeout: Option<String>,
    artifacts: Option<PathBuf>,
    ctx: &AppContext,
    registry: &CommandRegistry,
) {
    let args: serde_json::Value = match serde_json::from_str(args_str) {
        Ok(v) => v,
        Err(e) => {
            let r = result_err(
                "call",
                cmd,
                &new_run_id(),
                0,
                ErrorCode::InvalidInput,
                format!("invalid JSON args: {}", e),
            );
            output_result(&r, json);
            return;
        }
    };

    let timeout_dur = match timeout {
        Some(ref s) => match parse_timeout(s) {
            Ok(d) => Some(d),
            Err(msg) => {
                let r = result_err("call", cmd, &new_run_id(), 0, ErrorCode::InvalidInput, msg);
                output_result(&r, json);
                return;
            }
        },
        None => None,
    };

    let mut cx = Ctx::new(ctx);
    let started = Instant::now();
    if let Some(dur) = timeout_dur {
        cx = cx.with_deadline(started + dur);
    }
    let run_id = cx.request_id.clone();

    let result = match timeout_dur {
        Some(dur) => match tokio::time::timeout(dur, registry.execute(cmd, args, &cx)).await {
            Ok(r) => r,
            Err(_) => result_err(
                "call",
                cmd,
                &run_id,
                started.elapsed().as_millis() as u64,
                ErrorCode::Timeout,
                format!(
                    "command '{cmd}' timed out after {}",
                    timeout.unwrap_or_default()
                ),
            ),
        },
        None => registry.execute(cmd, args, &cx).await,
    };

    if let Some(ref dir) = artifacts {
        write_artifacts(dir, &result);
    }
    output_result(&result, json);
}

pub(crate) async fn cmd_probe(
    target: &str,
    json: bool,
    artifacts: Option<PathBuf>,
    ctx: &AppContext,
) {
    let result = engine::probes::run_probe(target, ctx).await;
    if let Some(ref dir) = artifacts {
        write_artifacts(dir, &result);
    }
    output_result(&result, json);
}

pub(crate) async fn cmd_run_scenario(
    file: &PathBuf,
    json: bool,
    interactive: bool,
    artifacts: Option<PathBuf>,
    ctx: &AppContext,
    registry: &CommandRegistry,
) {
    let yaml = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            let r = result_err(
                "run-scenario",
                &file.display().to_string(),
                &new_run_id(),
                0,
                ErrorCode::IoError,
                format!("cannot read scenario file: {}", e),
            );
            output_result(&r, json);
            return;
        }
    };

    let scenario = match engine::scenario::load_scenario(&yaml) {
        Ok(s) => s,
        Err(e) => {
            let r = result_err(
                "run-scenario",
                &file.display().to_string(),
                &new_run_id(),
                0,
                ErrorCode::InvalidInput,
                e,
            );
            output_result(&r, json);
            return;
        }
    };

    let scenario_result = if interactive {
        if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            eprintln!("error: --interactive requires a TTY (stdin is not a terminal)");
            std::process::exit(1);
        }
        engine::scenario::run_scenario_interactive(
            &scenario,
            ctx,
            registry,
            |idx, total, label, can_go_back| {
                use engine::scenario::StepChoice;

                // block_in_place tells Tokio this closure will block on TTY I/O,
                // so it can move async tasks off this worker thread.
                tokio::task::block_in_place(|| {
                    eprintln!("\n--- Step {}/{}: {} ---", idx + 1, total, label);

                    let mut choices = vec!["Run", "Skip"];
                    if can_go_back {
                        choices.push("\u{2190} Go back");
                    }

                    let selection = match dialoguer::Select::new()
                        .with_prompt("Run this step?")
                        .items(&choices)
                        .default(0)
                        .interact_opt()
                    {
                        Ok(Some(s)) => s,
                        Ok(None) => return None,
                        Err(e) => {
                            eprintln!("error: interactive prompt failed: {e}");
                            return None;
                        }
                    };

                    Some(match choices[selection] {
                        "Run" => StepChoice::Run,
                        "Skip" => StepChoice::Skip,
                        _ => StepChoice::GoBack,
                    })
                })
            },
            |idx, total, label| {
                use engine::scenario::FailureChoice;

                tokio::task::block_in_place(|| {
                    eprintln!("\n--- Step {}/{}: {} FAILED ---", idx + 1, total, label);

                    let choices = ["Continue to next step", "Abort scenario"];
                    let selection = match dialoguer::Select::new()
                        .with_prompt("Step failed. What would you like to do?")
                        .items(choices)
                        .default(0)
                        .interact_opt()
                    {
                        Ok(Some(s)) => s,
                        Ok(None) => return None,
                        Err(e) => {
                            eprintln!("error: interactive prompt failed: {e}");
                            return None;
                        }
                    };

                    Some(match choices[selection] {
                        "Continue to next step" => FailureChoice::Continue,
                        _ => FailureChoice::Abort,
                    })
                })
            },
        )
        .await
    } else {
        engine::scenario::run_scenario(&scenario, ctx, registry).await
    };

    if json {
        let j = serde_json::to_string_pretty(&scenario_result).unwrap_or_default();
        println!("{}", j);
    } else {
        println!(
            "Scenario: {}",
            scenario_result.name.as_deref().unwrap_or("<unnamed>")
        );
        println!("Overall: {:?}", scenario_result.overall_status);
        for (i, sr) in scenario_result.step_results.iter().enumerate() {
            println!(
                "  Step {}: {} -> {:?} ({}ms)",
                i, sr.target, sr.status, sr.timing_ms.total
            );
        }
    }

    if let Some(ref dir) = artifacts {
        let run_id = new_run_id();
        let art_dir = dir.join(&run_id);
        let _ = std::fs::create_dir_all(&art_dir);
        let result_path = art_dir.join("result.json");
        let j = serde_json::to_string_pretty(&scenario_result).unwrap_or_default();
        let _ = std::fs::write(&result_path, j);

        // Write per-step results as events.jsonl
        let events_path = art_dir.join("events.jsonl");
        let mut lines = String::new();
        for sr in &scenario_result.step_results {
            if let Ok(line) = serde_json::to_string(sr) {
                lines.push_str(&line);
                lines.push('\n');
            }
        }
        let _ = std::fs::write(&events_path, lines);
    }
}

// Output helpers

fn output_result(result: &CommandResult, json: bool) {
    if json {
        let j = serde_json::to_string_pretty(result).unwrap_or_default();
        println!("{}", j);
    } else {
        print_human(result);
    }

    // Exit with non-zero status on error/fail
    match result.status {
        Status::Pass | Status::Skip => {}
        Status::Fail => std::process::exit(1),
        Status::Error => std::process::exit(2),
    }
}

fn print_human(r: &CommandResult) {
    let status_icon = match r.status {
        Status::Pass => "PASS",
        Status::Fail => "FAIL",
        Status::Skip => "SKIP",
        Status::Error => "ERROR",
    };

    println!("[{}] {} {}", status_icon, r.command, r.target);
    println!("  run_id: {}", r.run_id);
    println!("  timing: {}ms", r.timing_ms.total);

    if !r.timing_ms.steps.is_empty() {
        for (step, ms) in &r.timing_ms.steps {
            println!("    {}: {}ms", step, ms);
        }
    }

    if let Some(ref err) = r.error {
        println!("  error:  {} – {}", err.code, err.message);
    }

    if let Some(ref data) = r.data {
        // Print compact data for human output
        if let Ok(s) = serde_json::to_string_pretty(data) {
            // Indent each line
            for line in s.lines() {
                println!("  {}", line);
            }
        }
    }

    println!(
        "  env: os={} arch={} headless={}",
        r.env_summary.os, r.env_summary.arch, r.env_summary.headless
    );
}

// Artifact helpers

fn write_result_file(path: &std::path::Path, result: &CommandResult) {
    let j = serde_json::to_string_pretty(result).unwrap_or_default();
    if let Err(e) = std::fs::write(path, &j) {
        eprintln!(
            "warning: failed to write result to {}: {}",
            path.display(),
            e
        );
    }
}

fn write_artifacts(dir: &std::path::Path, result: &CommandResult) {
    let art_dir = dir.join(&result.run_id);
    if let Err(e) = std::fs::create_dir_all(&art_dir) {
        eprintln!(
            "warning: failed to create artifacts dir {}: {}",
            art_dir.display(),
            e
        );
        return;
    }

    // result.json
    let result_path = art_dir.join("result.json");
    let j = serde_json::to_string_pretty(result).unwrap_or_default();
    let _ = std::fs::write(&result_path, &j);

    // events.jsonl (single event for non-scenario)
    let events_path = art_dir.join("events.jsonl");
    if let Ok(line) = serde_json::to_string(result) {
        let _ = std::fs::write(&events_path, format!("{}\n", line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timeout_units() {
        assert_eq!(parse_timeout("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_timeout("2m"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_timeout("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_timeout("1.5s"), Ok(Duration::from_millis(1500)));
        // bare number = seconds; surrounding whitespace tolerated
        assert_eq!(parse_timeout("  10 "), Ok(Duration::from_secs(10)));
    }

    #[test]
    fn parse_timeout_ms_takes_precedence_over_s() {
        // "500ms" ends in 's' too — must be read as milliseconds, not seconds.
        assert_eq!(parse_timeout("5000ms"), Ok(Duration::from_millis(5000)));
    }

    #[test]
    fn parse_timeout_rejects_garbage() {
        assert!(parse_timeout("bogus").is_err());
        assert!(parse_timeout("").is_err());
        assert!(parse_timeout("s").is_err());
        assert!(parse_timeout("12x").is_err());
    }

    #[test]
    fn parse_timeout_rejects_negative_and_nonfinite() {
        // These parse as valid f64 but must NOT reach Duration::from_secs_f64,
        // which panics on negative / non-finite / overflowing input.
        assert!(parse_timeout("-5s").is_err());
        assert!(parse_timeout("-1m").is_err());
        assert!(parse_timeout("NaNs").is_err());
        assert!(parse_timeout("infs").is_err());
        assert!(parse_timeout("1e400s").is_err()); // parses to f64::INFINITY
    }
}
