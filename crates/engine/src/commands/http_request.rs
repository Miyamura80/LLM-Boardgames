//! `http_request` – perform an outbound HTTP GET.
//!
//! The canonical async example: it awaits real network I/O through the
//! [`NetworkOps`](crate::traits::NetworkOps) capability, exercising the typed
//! contract, the per-request [`Ctx`], and the capability-error mapping.

use crate::commands::{Command, CommandError, Expose};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Default per-request timeout when the caller does not specify one.
const DEFAULT_TIMEOUT_MS: u64 = 10_000;

#[derive(Default)]
pub struct HttpRequest;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HttpRequestInput {
    /// Absolute URL to fetch. Must use the `https://` scheme.
    pub url: String,
    /// HTTP method. Only `GET` is supported by the network capability today;
    /// anything else returns an `Unsupported` error.
    #[serde(default)]
    pub method: Option<String>,
    /// Request timeout in milliseconds (default 10000).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HttpRequestOutput {
    pub status: u16,
    /// First few KiB of the response body.
    pub body_snippet: String,
}

#[async_trait]
impl Command for HttpRequest {
    type Input = HttpRequestInput;
    type Output = HttpRequestOutput;

    fn name(&self) -> &'static str {
        "http_request"
    }

    fn description(&self) -> &'static str {
        "Perform an outbound HTTP GET and return the status and a body snippet."
    }

    /// Fetches a caller-supplied URL with no scheme/host allowlist — CLI-only so
    /// it is not reachable as an unauthenticated SSRF primitive over the HTTP API.
    fn expose(&self) -> Expose {
        Expose::cli_only()
    }

    async fn run(
        &self,
        input: HttpRequestInput,
        cx: &Ctx<'_>,
    ) -> Result<HttpRequestOutput, CommandError> {
        let method = input.method.as_deref().unwrap_or("GET");
        if !method.eq_ignore_ascii_case("GET") {
            return Err(CommandError::Unsupported(format!(
                "method {method} is not supported (only GET)"
            )));
        }

        // The capability is `https_get`; enforce the scheme rather than silently
        // issuing a cleartext request for an `http://` URL. Schemes are
        // case-insensitive (RFC 3986 §3.1).
        let scheme_ok = input
            .url
            .split_once("://")
            .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("https"));
        if !scheme_ok {
            return Err(CommandError::InvalidInput(
                "url must use the https:// scheme".to_string(),
            ));
        }

        // A `0` timeout would fire instantly; treat it (and absent) as the default.
        let timeout_ms = input
            .timeout_ms
            .filter(|&t| t > 0)
            .unwrap_or(DEFAULT_TIMEOUT_MS);
        let (status, body_snippet) = cx.network().https_get(&input.url, timeout_ms).await?;
        Ok(HttpRequestOutput {
            status,
            body_snippet,
        })
    }
}

register_command!(HttpRequest);
