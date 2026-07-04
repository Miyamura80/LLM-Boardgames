use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Final result JSON – the stable output contract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub run_id: String,
    pub command: String,
    pub target: String,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    pub timing_ms: TimingInfo,
    #[serde(default)]
    pub artifacts: Vec<String>,
    pub env_summary: EnvSummary,
    /// Arbitrary command-specific payload returned on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    Skip,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidInput,
    Unsupported,
    Unimplemented,
    DependencyMissing,
    PermissionDenied,
    NetworkError,
    IoError,
    Timeout,
    ExternalInterference,
    InternalError,
    UserSkipped,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", self));
        f.write_str(&s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimingInfo {
    pub total: u64,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub steps: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvSummary {
    pub os: String,
    pub arch: String,
    pub headless: bool,
}

impl Default for EnvSummary {
    fn default() -> Self {
        Self {
            os: current_os().to_string(),
            arch: std::env::consts::ARCH.to_string(),
            headless: detect_headless(),
        }
    }
}

// ---------------------------------------------------------------------------
// Doctor-specific types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub os_name: String,
    pub os_version: String,
    pub kernel: String,
    pub arch: String,
    pub user_id: Option<u32>,
    pub effective_user_id: Option<u32>,
    pub is_admin: bool,
    pub headless: bool,
    pub session_type: Option<String>,
    pub display_server: Option<String>,
    /// Names (never values) of the proxy-related env vars that are set — the
    /// report is served over the HTTP API and values can carry credentials.
    pub proxy_env_set: Vec<String>,
}

// ---------------------------------------------------------------------------
// Scenario types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    #[serde(default)]
    pub name: Option<String>,
    pub steps: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScenarioStep {
    Call {
        call: String,
        #[serde(default)]
        args: serde_json::Value,
        #[serde(default = "default_expect_status")]
        expect_status: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    Probe {
        probe: String,
    },
}

fn default_expect_status() -> String {
    "pass".to_string()
}

fn default_timeout_ms() -> u64 {
    30_000
}

// ---------------------------------------------------------------------------
// Scenario result
// ---------------------------------------------------------------------------

/// Result of a scenario run.
///
/// When using `run_scenario_interactive`, `step_results` may contain fewer
/// entries than the declared scenario steps if the user aborts mid-run or
/// a step fails and the user chooses not to continue. `overall_status` values:
/// - `Pass` – every step was decided (run or skip) and all executed steps
///   met expectations. Note: individual steps may have been skipped by the
///   user — `Pass` does not guarantee every step ran.
/// - `Skip` – user aborted before all steps were reached, OR every step was
///   explicitly skipped.
/// - `Fail` – at least one step failed its expectation (run may be partial if
///   user also aborted at the failure dialog).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub name: Option<String>,
    pub overall_status: Status,
    pub step_results: Vec<CommandResult>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn current_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => other,
    }
}

pub fn detect_headless() -> bool {
    match std::env::consts::OS {
        "linux" => {
            // No X11 or Wayland display → headless
            std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err()
        }
        "macos" => {
            // Best-effort: assume not headless unless SSH_TTY is set and no display
            std::env::var("SSH_TTY").is_ok() && std::env::var("DISPLAY").is_err()
        }
        _ => false,
    }
}

/// Generate a new run ID (UUIDv4).
pub fn new_run_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Build a successful CommandResult shell (caller fills in data).
pub fn result_ok(command: &str, target: &str, run_id: &str, total_ms: u64) -> CommandResult {
    CommandResult {
        run_id: run_id.to_string(),
        command: command.to_string(),
        target: target.to_string(),
        status: Status::Pass,
        error: None,
        timing_ms: TimingInfo {
            total: total_ms,
            steps: HashMap::new(),
        },
        artifacts: vec![],
        env_summary: EnvSummary::default(),
        data: None,
    }
}

/// Build an error CommandResult.
pub fn result_err(
    command: &str,
    target: &str,
    run_id: &str,
    total_ms: u64,
    code: ErrorCode,
    message: impl Into<String>,
) -> CommandResult {
    CommandResult {
        run_id: run_id.to_string(),
        command: command.to_string(),
        target: target.to_string(),
        status: Status::Error,
        error: Some(ErrorInfo {
            code,
            message: message.into(),
            details: serde_json::Value::Null,
        }),
        timing_ms: TimingInfo {
            total: total_ms,
            steps: HashMap::new(),
        },
        artifacts: vec![],
        env_summary: EnvSummary::default(),
        data: None,
    }
}

/// Build a skip CommandResult.
pub fn result_skip(
    command: &str,
    target: &str,
    run_id: &str,
    total_ms: u64,
    reason: impl Into<String>,
) -> CommandResult {
    CommandResult {
        run_id: run_id.to_string(),
        command: command.to_string(),
        target: target.to_string(),
        status: Status::Skip,
        error: Some(ErrorInfo {
            code: ErrorCode::Unsupported,
            message: reason.into(),
            details: serde_json::Value::Null,
        }),
        timing_ms: TimingInfo {
            total: total_ms,
            steps: HashMap::new(),
        },
        artifacts: vec![],
        env_summary: EnvSummary::default(),
        data: None,
    }
}
