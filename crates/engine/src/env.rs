//! Shared environment-variable helpers.

/// Proxy-related env var names inspected by the doctor report and the network
/// probe. Both surfaces expose **names only** — never values — because
/// `HTTP_PROXY`/`HTTPS_PROXY` can embed credentials and both are reachable over
/// the HTTP API.
pub(crate) const PROXY_VAR_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// Sorted names of the proxy-related env vars that are currently set.
pub(crate) fn proxy_env_names() -> Vec<String> {
    // `var_os` (not `var`) so a proxy var with a non-UTF-8 value still counts as
    // set — this is presence-only reporting, the value is never read.
    let mut out: Vec<String> = PROXY_VAR_KEYS
        .iter()
        .filter(|k| std::env::var_os(k).is_some())
        .map(|k| (*k).to_string())
        .collect();
    out.sort();
    out
}
