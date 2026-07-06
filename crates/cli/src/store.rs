//! Postgres persistence for game transcripts, leaderboards, and metrics.
//!
//! Records are stored as JSONB — the engine's `GameRecord` and `MatchOutcome`
//! are the source of truth and evolve without migrations. The `DATABASE_URL`
//! points at a local Postgres in the sandbox or a deployed instance in prod;
//! the schema self-initialises on connect.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use engine::eval::GameRecord;
use serde::Serialize;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct Store {
    pool: PgPool,
}

/// A row summary for the games list endpoint.
#[derive(Debug, Serialize)]
pub struct GameSummary {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub seed: i64,
    pub winner: String,
    pub win_reason: String,
}

impl Store {
    /// Connect and ensure the schema exists.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("connecting to Postgres")?;
        let store = Self { pool };
        store.init_schema().await?;
        Ok(store)
    }

    async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sh_games (
                id          UUID PRIMARY KEY,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
                seed        BIGINT NOT NULL,
                winner      TEXT NOT NULL,
                win_reason  TEXT NOT NULL,
                match_id    UUID,
                record      JSONB NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sh_matches (
                id          UUID PRIMARY KEY,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
                games       INT NOT NULL,
                payload     JSONB NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Persist one game record; returns its id.
    pub async fn insert_game(&self, record: &GameRecord, match_id: Option<Uuid>) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let json = serde_json::to_value(record)?;
        sqlx::query(
            "INSERT INTO sh_games (id, seed, winner, win_reason, match_id, record) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(record.seed as i64)
        .bind(format!("{:?}", record.winner).to_lowercase())
        .bind(format!("{:?}", record.win_reason))
        .bind(match_id)
        .bind(json)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Persist a match outcome (leaderboard + metrics + distribution).
    pub async fn insert_match(&self, games: usize, payload: &serde_json::Value) -> Result<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO sh_matches (id, games, payload) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(games as i32)
            .bind(payload)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn list_games(&self, limit: i64) -> Result<Vec<GameSummary>> {
        let rows = sqlx::query(
            "SELECT id, created_at, seed, winner, win_reason FROM sh_games \
             ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| GameSummary {
                id: r.get("id"),
                created_at: r.get("created_at"),
                seed: r.get("seed"),
                winner: r.get("winner"),
                win_reason: r.get("win_reason"),
            })
            .collect())
    }

    pub async fn get_game(&self, id: Uuid) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query("SELECT record FROM sh_games WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<serde_json::Value, _>("record")))
    }

    pub async fn latest_match(&self) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query("SELECT payload FROM sh_matches ORDER BY created_at DESC LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<serde_json::Value, _>("payload")))
    }
}
