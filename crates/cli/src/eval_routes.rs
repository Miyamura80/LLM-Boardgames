//! HTTP read endpoints for the eval store (feature = "store"): leaderboard and
//! game transcripts, backed by Postgres. Split from `serve_http` to keep that
//! file within the line budget.

use crate::serve_http::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;

/// Connect to the eval store from `DATABASE_URL`, if set and reachable.
pub async fn connect_store() -> Option<Arc<crate::store::Store>> {
    let url = std::env::var("DATABASE_URL").ok()?;
    match crate::store::Store::connect(&url).await {
        Ok(s) => Some(Arc::new(s)),
        Err(e) => {
            eprintln!("warning: eval store unavailable ({e:#}); /leaderboard and /games disabled");
            None
        }
    }
}

fn store_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "eval store not configured (set DATABASE_URL)" })),
    )
        .into_response()
}

fn server_error(e: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
        .into_response()
}

/// GET /api/v1/leaderboard — the most recent match's ratings/metrics/distribution.
pub async fn leaderboard(State(st): State<AppState>) -> Response {
    let Some(store) = st.store else {
        return store_unavailable();
    };
    match store.latest_match().await {
        Ok(Some(payload)) => Json(payload).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no matches yet" })),
        )
            .into_response(),
        Err(e) => server_error(e),
    }
}

/// GET /api/v1/games — recent game summaries.
pub async fn list_games(State(st): State<AppState>) -> Response {
    let Some(store) = st.store else {
        return store_unavailable();
    };
    match store.list_games(100).await {
        Ok(games) => Json(games).into_response(),
        Err(e) => server_error(e),
    }
}

/// GET /api/v1/games/:id — a single game's full transcript record.
pub async fn get_game(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(store) = st.store else {
        return store_unavailable();
    };
    let Ok(uuid) = id.parse::<uuid::Uuid>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid game id" })),
        )
            .into_response();
    };
    match store.get_game(uuid).await {
        Ok(Some(record)) => Json(record).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "game not found" })),
        )
            .into_response(),
        Err(e) => server_error(e),
    }
}
