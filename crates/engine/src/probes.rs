//! Targeted capability probes – filesystem, network.

use crate::context::AppContext;
use crate::traits::CapError;
use crate::types::*;
use std::collections::HashMap;
use std::time::Instant;

/// Run a probe by name and return a full CommandResult.
pub async fn run_probe(name: &str, ctx: &AppContext) -> CommandResult {
    match name {
        "filesystem" => probe_filesystem(ctx),
        "network" => probe_network(ctx).await,
        _ => {
            let run_id = new_run_id();
            result_err(
                "probe",
                name,
                &run_id,
                0,
                ErrorCode::InvalidInput,
                format!("unknown probe: {} (available: filesystem, network)", name),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Filesystem probe
// ---------------------------------------------------------------------------

fn probe_filesystem(ctx: &AppContext) -> CommandResult {
    let run_id = new_run_id();
    let start = Instant::now();
    let mut steps = HashMap::new();

    let tmp_dir = ctx
        .fs()
        .temp_dir()
        .join(format!("engine_probe_{}", &run_id[..8]));

    // Step 1: create temp directory
    let t0 = Instant::now();
    if let Err(e) = ctx.fs().create_dir_all(&tmp_dir) {
        return probe_fs_err(&run_id, start, steps, "create_dir", e);
    }
    steps.insert("create_dir".into(), t0.elapsed().as_millis() as u64);

    // Step 2: write a test file
    let test_file = tmp_dir.join("probe_test.txt");
    let payload = b"engine filesystem probe";
    let t1 = Instant::now();
    if let Err(e) = ctx.fs().write_file(&test_file, payload) {
        let _ = ctx.fs().remove_dir_all(&tmp_dir);
        return probe_fs_err(&run_id, start, steps, "write_file", e);
    }
    steps.insert("write_file".into(), t1.elapsed().as_millis() as u64);

    // Step 3: read it back and verify
    let t2 = Instant::now();
    match ctx.fs().read_file(&test_file) {
        Ok(data) => {
            if data != payload {
                let _ = ctx.fs().remove_dir_all(&tmp_dir);
                return result_err(
                    "probe",
                    "filesystem",
                    &run_id,
                    start.elapsed().as_millis() as u64,
                    ErrorCode::ExternalInterference,
                    "read-back data does not match written data",
                );
            }
        }
        Err(e) => {
            let _ = ctx.fs().remove_dir_all(&tmp_dir);
            return probe_fs_err(&run_id, start, steps, "read_file", e);
        }
    }
    steps.insert("read_verify".into(), t2.elapsed().as_millis() as u64);

    // Step 4: cleanup
    let t3 = Instant::now();
    let _ = ctx.fs().remove_dir_all(&tmp_dir);
    steps.insert("cleanup".into(), t3.elapsed().as_millis() as u64);

    let mut r = result_ok(
        "probe",
        "filesystem",
        &run_id,
        start.elapsed().as_millis() as u64,
    );
    r.timing_ms.steps = steps;
    r.data = Some(serde_json::json!({
        "temp_dir_used": tmp_dir.display().to_string(),
    }));
    r
}

fn probe_fs_err(
    run_id: &str,
    start: Instant,
    steps: HashMap<String, u64>,
    failed_step: &str,
    err: CapError,
) -> CommandResult {
    let code = match &err {
        CapError::PermissionDenied(_) => ErrorCode::PermissionDenied,
        CapError::Io(_) => ErrorCode::IoError,
        _ => ErrorCode::InternalError,
    };
    let mut r = result_err(
        "probe",
        "filesystem",
        run_id,
        start.elapsed().as_millis() as u64,
        code,
        format!("filesystem probe failed at {}: {}", failed_step, err),
    );
    r.timing_ms.steps = steps;
    r
}

// ---------------------------------------------------------------------------
// Network probe
// ---------------------------------------------------------------------------

async fn probe_network(ctx: &AppContext) -> CommandResult {
    let run_id = new_run_id();
    let start = Instant::now();
    let mut steps = HashMap::new();

    let host = &ctx.network_probe_host;
    // Extract hostname for DNS (strip scheme + path)
    let dns_host = host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(host);

    // Step 1: DNS resolve
    let t0 = Instant::now();
    let addrs = match ctx.network().dns_resolve(dns_host).await {
        Ok(addrs) => addrs,
        Err(e) => {
            steps.insert("dns_resolve".into(), t0.elapsed().as_millis() as u64);
            let msg = format!("DNS resolution failed: {}", e);
            return probe_net_err(&run_id, start, steps, ErrorCode::NetworkError, msg);
        }
    };
    steps.insert("dns_resolve".into(), t0.elapsed().as_millis() as u64);

    // Step 2: HTTPS GET
    let t1 = Instant::now();
    let status = match ctx.network().https_get(host, 10_000).await {
        Ok((status, _snippet)) => status,
        Err(e) => {
            steps.insert("https_get".into(), t1.elapsed().as_millis() as u64);
            let code = match &e {
                CapError::Timeout => ErrorCode::Timeout,
                _ => ErrorCode::NetworkError,
            };
            let msg = format!("HTTPS GET failed: {}", e);
            return probe_net_err(&run_id, start, steps, code, msg);
        }
    };
    steps.insert("https_get".into(), t1.elapsed().as_millis() as u64);

    let mut r = result_ok(
        "probe",
        "network",
        &run_id,
        start.elapsed().as_millis() as u64,
    );
    r.timing_ms.steps = steps;
    r.data = Some(serde_json::json!({
        "dns_addresses": addrs,
        "http_status": status,
        // Names only — values can embed credentials (e.g. an authenticated
        // HTTP_PROXY URL) and this probe is reachable over the HTTP API.
        "proxy_env_set": crate::env::proxy_env_names(),
        "target_url": host,
    }));
    r
}

fn probe_net_err(
    run_id: &str,
    start: Instant,
    steps: HashMap<String, u64>,
    code: ErrorCode,
    message: String,
) -> CommandResult {
    let mut r = result_err(
        "probe",
        "network",
        run_id,
        start.elapsed().as_millis() as u64,
        code,
        message,
    );
    r.timing_ms.steps = steps;
    r
}
