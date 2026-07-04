//! Application context – holds capability trait objects and config.
//!
//! Two layers, mirroring the transport design:
//! - [`AppContext`] holds the shared OS capabilities (fs/net). It is
//!   built once at process/transport start and shared (e.g. behind an `Arc` in
//!   the HTTP server's state).
//! - [`Ctx`] is constructed **per request/invocation**, borrowing the shared
//!   capabilities and carrying request-scoped data (`request_id`, `deadline`).
//!   Commands receive `&Ctx`. This is the seam where auth/identity would later
//!   attach — no identity field exists yet, by design.

use crate::platform::{ReqwestNetwork, StdFilesystem};
use crate::traits::*;
use crate::types::new_run_id;
use std::time::Instant;

/// Central context passed to all engine operations.
///
/// Holds trait-object capabilities. [`AppContext::default`] wires the real
/// platform implementations; [`AppContext::new`] is the injection seam — pass
/// stub capabilities to it to run a command against fakes (e.g. an offline test
/// filesystem/network).
pub struct AppContext {
    fs: Box<dyn FilesystemOps>,
    network: Box<dyn NetworkOps>,
    /// Target URL the `network` probe hits. Defaults to
    /// [`DEFAULT_NETWORK_PROBE_HOST`], overridable via the
    /// `APP__NETWORK_PROBE_HOST` env var or by setting this field directly
    /// (e.g. to an internal health endpoint you control).
    pub network_probe_host: String,
}

/// Default target for the `network` probe when no override is set.
pub const DEFAULT_NETWORK_PROBE_HOST: &str = "https://httpbin.org/get";

impl Default for AppContext {
    /// Wire the real platform capabilities. Use [`AppContext::new`] to inject
    /// stub implementations instead.
    ///
    /// This is the production path, so it also honors the
    /// `APP__NETWORK_PROBE_HOST` env override for the `network` probe target.
    /// [`AppContext::new`] deliberately does **not** read env, so stub-injecting
    /// tests stay deterministic regardless of ambient environment.
    fn default() -> Self {
        let mut ctx = Self::new(Box::new(StdFilesystem), Box::new(ReqwestNetwork));
        if let Some(host) = std::env::var("APP__NETWORK_PROBE_HOST")
            .ok()
            .filter(|v| !v.trim().is_empty())
        {
            ctx.network_probe_host = host;
        }
        ctx
    }
}

impl AppContext {
    /// Build a context over caller-supplied capabilities. This is the seam for
    /// injecting stubs (tests, offline runs); [`AppContext::default`] wires the
    /// real platform ones. Pure: reads no environment, so tests are
    /// deterministic. `network_probe_host` starts at
    /// [`DEFAULT_NETWORK_PROBE_HOST`] and can be overridden by setting the field
    /// directly.
    pub fn new(fs: Box<dyn FilesystemOps>, network: Box<dyn NetworkOps>) -> Self {
        Self {
            fs,
            network,
            network_probe_host: DEFAULT_NETWORK_PROBE_HOST.to_string(),
        }
    }

    pub fn fs(&self) -> &dyn FilesystemOps {
        self.fs.as_ref()
    }

    pub fn network(&self) -> &dyn NetworkOps {
        self.network.as_ref()
    }
}

/// Per-request context passed to every [`Command`](crate::commands::Command).
///
/// Borrows the shared [`AppContext`] capabilities and adds request-scoped
/// state. Built fresh for each invocation so each request gets its own
/// `request_id` (and, when set, `deadline`).
pub struct Ctx<'a> {
    /// Unique id for this invocation (surfaced as `run_id` in the CLI envelope
    /// and the `x-run-id` HTTP header).
    pub request_id: String,
    /// Optional wall-clock deadline for this invocation. Enforcement lives in
    /// the transport (scenario runner / tower timeout); commands may consult it.
    pub deadline: Option<Instant>,
    caps: &'a AppContext,
}

impl<'a> Ctx<'a> {
    /// Build a per-request context over shared capabilities with a fresh id.
    pub fn new(caps: &'a AppContext) -> Self {
        Self {
            request_id: new_run_id(),
            deadline: None,
            caps,
        }
    }

    /// Build a context with a caller-supplied request id (e.g. an incoming
    /// `x-run-id`/trace header).
    pub fn with_request_id(caps: &'a AppContext, request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            deadline: None,
            caps,
        }
    }

    /// Attach a deadline (builder-style).
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// The shared capability bundle.
    pub fn caps(&self) -> &AppContext {
        self.caps
    }

    pub fn fs(&self) -> &dyn FilesystemOps {
        self.caps.fs()
    }

    pub fn network(&self) -> &dyn NetworkOps {
        self.caps.network()
    }
}
