//! HTTP API – an `axum` app that exposes the engine registry over `/api/v1`.
//!
//! Routes are **auto-derived** from the command registry (one `POST` per
//! command), mirroring the reference. Cross-cutting concerns live here in the
//! tower middleware stack (CORS, tracing, timeout, request-id) — never in
//! `engine`. Responses are the **bare typed `Output`**; the `run_id` rides in
//! the `x-run-id` header. The router is built by [`build_app`] so it can be
//! driven in-process by `tower::ServiceExt::oneshot` in tests, and is shaped so
//! a `/mcp` sub-router can be nested later without reshaping the app.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use engine::types::ErrorCode;
use engine::{AppContext, CommandError, CommandRegistry, Ctx};
use serde_json::{json, Value};
use tower_http::{
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

/// Shared application state: capabilities + registry, both cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub caps: Arc<AppContext>,
    pub registry: Arc<CommandRegistry>,
}

/// Operational tunables for the HTTP server, sourced from `global_config.yaml`
/// (`server.*`). Kept out of [`AppState`] since middleware — not handlers —
/// consumes them.
pub struct ServeSettings {
    pub request_timeout: Duration,
    pub cors_allow_origins: Vec<String>,
}

impl Default for ServeSettings {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            cors_allow_origins: Vec::new(),
        }
    }
}

impl ServeSettings {
    /// Project the loaded `server` config block into serve-time settings. A
    /// `0` (or absent) timeout falls back to 30s rather than 408-ing instantly.
    pub fn from_config(cfg: &app_config::ServerConfig) -> Self {
        let secs = cfg.request_timeout_secs.filter(|&s| s > 0).unwrap_or(30);
        Self {
            request_timeout: Duration::from_secs(secs),
            cors_allow_origins: cfg.cors_allow_origins.clone(),
        }
    }
}

/// Permissive CORS when no origins are configured (dev default); otherwise an
/// explicit origin allowlist. Set `server.cors_allow_origins` to lock down.
fn cors_layer(origins: &[String]) -> CorsLayer {
    if origins.is_empty() {
        return CorsLayer::permissive();
    }
    // Surface misconfigured origins instead of silently narrowing the allowlist
    // (an all-invalid list would otherwise deny every cross-origin request).
    let allowed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|o| match o.parse::<HeaderValue>() {
            Ok(v) => Some(v),
            Err(_) => {
                eprintln!("warning: ignoring unparseable CORS origin: {o}");
                None
            }
        })
        .collect();
    CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods(Any)
        .allow_headers(Any)
}

/// Build the router. Pure function of state + settings → easy to unit-test.
pub fn build_app(state: AppState, settings: &ServeSettings) -> Router {
    let api = Router::new()
        .route("/commands", get(list_commands))
        .route("/commands/:name", post(run_command))
        .route("/probe/:target", post(run_probe))
        .route("/doctor", get(doctor))
        .route("/config", get(get_config));

    Router::new()
        .route("/healthz", get(healthz))
        .nest("/api/v1", api)
        // Middleware seam: auth / rate-limit slot in here later, never in engine.
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer(&settings.cors_allow_origins))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            settings.request_timeout,
        ))
        .with_state(state)
}

/// Bind and serve until SIGTERM/SIGINT, then shut down gracefully.
pub async fn run_server(
    host: String,
    port: u16,
    caps: AppContext,
    registry: CommandRegistry,
    settings: ServeSettings,
) {
    let state = AppState {
        caps: Arc::new(caps),
        registry: Arc::new(registry),
    };
    // Permissive CORS is fine for local dev, but warn if it's left open outside
    // a dev environment so a production deploy doesn't silently accept any origin.
    if settings.cors_allow_origins.is_empty() && app_config::get_config().dev_env != "dev" {
        eprintln!(
            "warning: CORS is permissive (any origin); set server.cors_allow_origins for production"
        );
    }

    let app = build_app(state, &settings);

    // Bind with a (host, port) tuple rather than a formatted string so a bare
    // IPv6 host (e.g. `::1`) resolves correctly instead of being mis-parsed.
    let listener = match tokio::net::TcpListener::bind((host.as_str(), port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot bind {host}:{port}: {e}");
            std::process::exit(2);
        }
    };
    // Bracket a bare IPv6 host so the logged URL is valid (`http://[::1]:8080`).
    let display_host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.clone()
    };
    eprintln!("shbench serve listening on http://{display_host}:{port}");

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn healthz() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

/// List commands exposed to the API, with their JSON Schemas (introspection).
async fn list_commands(State(st): State<AppState>) -> impl IntoResponse {
    let schemas: Vec<_> = st
        .registry
        .schemas()
        .into_iter()
        .filter(|s| s.expose.api)
        .collect();
    Json(schemas)
}

/// Run a command. Body = `Input` JSON; response = bare `Output`.
async fn run_command(
    State(st): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Only API-exposed commands are reachable over HTTP; anything else — unknown
    // or CLI-only — is a 404 with no distinction (don't leak the CLI surface).
    if !matches!(st.registry.schema(&name), Some(s) if s.expose.api) {
        return problem(
            StatusCode::NOT_FOUND,
            ErrorCode::InvalidInput,
            format!("unknown command: {name}"),
            None,
        );
    }

    let args: Value = if body.is_empty() {
        Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return problem(
                    StatusCode::BAD_REQUEST,
                    ErrorCode::InvalidInput,
                    format!("invalid JSON body: {e}"),
                    None,
                )
            }
        }
    };

    // Honor an inbound run/trace id if provided, else mint a fresh one.
    let cx = match incoming_run_id(&headers) {
        Some(id) => Ctx::with_request_id(&st.caps, id),
        None => Ctx::new(&st.caps),
    };
    let run_id = cx.request_id.clone();

    match st.registry.call(&name, args, &cx).await {
        Ok(value) => {
            let mut resp = Json(value).into_response();
            insert_run_id(resp.headers_mut(), &run_id);
            resp
        }
        Err(e) => {
            let code = e.error_code();
            problem(status_for(&e), code, e.to_string(), Some(&run_id))
        }
    }
}

/// Run a capability probe. Returns the diagnostic `CommandResult` envelope.
async fn run_probe(State(st): State<AppState>, Path(target): Path<String>) -> impl IntoResponse {
    let result = engine::probes::run_probe(&target, &st.caps).await;
    Json(result)
}

/// Environment report (diagnostic envelope).
async fn doctor() -> impl IntoResponse {
    Json(engine::doctor::run_doctor())
}

/// Sanitized configuration for the frontend. Security boundary: `FrontendConfig`
/// never carries secrets (enforced + tested in `app-config`).
async fn get_config() -> impl IntoResponse {
    Json(app_config::get_frontend_config())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn incoming_run_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-run-id")
        .or_else(|| headers.get("x-request-id"))
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn insert_run_id(headers: &mut HeaderMap, run_id: &str) {
    if let Ok(v) = HeaderValue::from_str(run_id) {
        headers.insert("x-run-id", v);
    }
}

/// Map a command error onto an HTTP status.
fn status_for(err: &CommandError) -> StatusCode {
    match err.error_code() {
        ErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
        ErrorCode::NetworkError => StatusCode::BAD_GATEWAY,
        ErrorCode::Timeout => StatusCode::GATEWAY_TIMEOUT,
        ErrorCode::Unsupported => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::Unimplemented => StatusCode::NOT_IMPLEMENTED,
        ErrorCode::IoError
        | ErrorCode::DependencyMissing
        | ErrorCode::ExternalInterference
        | ErrorCode::UserSkipped
        | ErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// A small RFC-7807-ish problem body: `{ "error": { "code", "message" } }`.
fn problem(status: StatusCode, code: ErrorCode, message: String, run_id: Option<&str>) -> Response {
    let body = Json(json!({ "error": { "code": code.to_string(), "message": message } }));
    let mut resp = (status, body).into_response();
    if let Some(id) = run_id {
        insert_run_id(resp.headers_mut(), id);
    }
    resp
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

// ===========================================================================
// Integration tests (in-process, via tower::ServiceExt::oneshot – no socket)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt; // for `oneshot`

    fn test_app() -> Router {
        build_app(
            AppState {
                caps: Arc::new(AppContext::default()),
                registry: Arc::new(CommandRegistry::new()),
            },
            &ServeSettings::default(),
        )
    }

    async fn body_json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        }
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn post(uri: &str, json_body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(json_body.to_owned()))
            .unwrap()
    }

    #[tokio::test]
    async fn healthz_ok() {
        let resp = test_app().oneshot(get("/healthz")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["status"], "ok");
    }

    #[tokio::test]
    async fn commands_introspection_lists_ping_with_schema() {
        let resp = test_app().oneshot(get("/api/v1/commands")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        let arr = body.as_array().unwrap();
        let ping = arr
            .iter()
            .find(|c| c["name"] == "ping")
            .expect("ping present");
        assert!(ping["input_schema"].is_object());
        assert!(ping["output_schema"].is_object());
    }

    #[tokio::test]
    async fn run_ping_returns_bare_output_and_run_id_header() {
        let resp = test_app()
            .oneshot(post("/api/v1/commands/ping", "{}"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().contains_key("x-run-id"));
        // Bare Output, not the CommandResult envelope.
        let body = body_json(resp).await;
        assert_eq!(body, json!({ "pong": true }));
    }

    #[tokio::test]
    async fn empty_body_is_treated_as_empty_object() {
        let resp = test_app()
            .oneshot(post("/api/v1/commands/ping", ""))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn honors_inbound_run_id() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/commands/ping")
            .header("content-type", "application/json")
            .header("x-run-id", "trace-123")
            .body(Body::from("{}"))
            .unwrap();
        let resp = test_app().oneshot(req).await.unwrap();
        assert_eq!(resp.headers()["x-run-id"], "trace-123");
    }

    #[tokio::test]
    async fn cli_only_command_is_not_exposed_over_http() {
        // read_file / write_file / list_dir / http_request are `Expose::cli_only()`
        // (arbitrary file access + SSRF). The HTTP surface must 404 them — not
        // dispatch, and not leak that they exist — same as an unknown command.
        for name in ["read_file", "write_file", "list_dir", "http_request"] {
            let resp = test_app()
                .oneshot(post(&format!("/api/v1/commands/{name}"), "{}"))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{name} must be 404");
        }
    }

    #[test]
    fn status_for_maps_command_error_codes() {
        // The 422 mapping was previously exercised via http_request over HTTP;
        // now that it's CLI-only, cover the error→status contract directly.
        assert_eq!(
            status_for(&CommandError::InvalidInput("x".into())),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_for(&CommandError::Unsupported("x".into())),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn unknown_command_maps_to_404() {
        let resp = test_app()
            .oneshot(post("/api/v1/commands/nope", "{}"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn invalid_json_body_maps_to_400() {
        let resp = test_app()
            .oneshot(post("/api/v1/commands/ping", "{not json"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn config_endpoint_never_serializes_secrets() {
        let resp = test_app().oneshot(get("/api/v1/config")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!text.contains("api_key"), "config leaked a secret: {text}");
    }

    #[tokio::test]
    async fn probe_filesystem_ok() {
        let resp = test_app()
            .oneshot(post("/api/v1/probe/filesystem", ""))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Probes return the diagnostic envelope.
        assert_eq!(body_json(resp).await["status"], "pass");
    }
}
