//! Doctor – gather environment facts for diagnostics.

use crate::types::*;
use std::time::Instant;

/// Run the doctor check and return a full report as a CommandResult.
pub fn run_doctor() -> CommandResult {
    let run_id = new_run_id();
    let start = Instant::now();

    let report = gather_report();

    let mut r = result_ok("doctor", "env", &run_id, start.elapsed().as_millis() as u64);
    r.data = Some(serde_json::to_value(&report).unwrap_or_default());
    r
}

fn gather_report() -> DoctorReport {
    DoctorReport {
        os_name: os_name(),
        os_version: os_version(),
        kernel: kernel_version(),
        arch: std::env::consts::ARCH.to_string(),
        user_id: get_uid(),
        effective_user_id: get_euid(),
        is_admin: is_admin(),
        headless: detect_headless(),
        session_type: session_type(),
        display_server: display_server(),
        proxy_env_set: crate::env::proxy_env_names(),
    }
}

fn os_name() -> String {
    std::env::consts::OS.to_string()
}

fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        run_cmd("sw_vers", &["-productVersion"]).unwrap_or_else(|| "unknown".into())
    }
    #[cfg(target_os = "linux")]
    {
        // Try /etc/os-release
        if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if let Some(ver) = line.strip_prefix("PRETTY_NAME=") {
                    return ver.trim_matches('"').to_string();
                }
            }
        }
        "unknown".to_string()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "unknown".to_string()
    }
}

fn kernel_version() -> String {
    run_cmd("uname", &["-r"]).unwrap_or_else(|| "unknown".into())
}

#[cfg(unix)]
fn get_uid() -> Option<u32> {
    // SAFETY: getuid() is always safe to call
    Some(libc_free_getuid())
}
#[cfg(not(unix))]
fn get_uid() -> Option<u32> {
    None
}

#[cfg(unix)]
fn get_euid() -> Option<u32> {
    Some(libc_free_geteuid())
}
#[cfg(not(unix))]
fn get_euid() -> Option<u32> {
    None
}

/// Get UID without linking libc – shell out to `id -u`.
#[cfg(unix)]
fn libc_free_getuid() -> u32 {
    run_cmd("id", &["-u"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(u32::MAX)
}

/// Get EUID – on most systems same as UID unless setuid.
#[cfg(unix)]
fn libc_free_geteuid() -> u32 {
    // EUID == UID unless setuid binary; `id -u` returns effective UID
    libc_free_getuid()
}

fn is_admin() -> bool {
    #[cfg(unix)]
    {
        get_euid() == Some(0)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn session_type() -> Option<String> {
    std::env::var("XDG_SESSION_TYPE").ok()
}

fn display_server() -> Option<String> {
    if let Ok(d) = std::env::var("WAYLAND_DISPLAY") {
        return Some(format!("wayland ({})", d));
    }
    if let Ok(d) = std::env::var("DISPLAY") {
        return Some(format!("x11 ({})", d));
    }
    #[cfg(target_os = "macos")]
    {
        Some("quartz".to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn run_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}
