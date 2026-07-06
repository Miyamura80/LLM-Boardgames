//! Postgres persistence for runs, games, seat metrics, and ratings.
//!
//! Connection string resolution: `AppConfig.database_url` →
//! `DATABASE_URL` env var. Migrations are embedded and applied on connect.

use super::metrics::SeatMetrics;
use super::rating::{RatingState, RatingTable};
use super::runner::record::GameRecord;
use super::types::Role;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

pub type StoreResult<T> = Result<T, sqlx::Error>;

pub struct Store {
    pool: PgPool,
}

/// Resolve the Postgres URL from config/env.
pub fn database_url() -> Option<String> {
    app_config::try_get_config()
        .ok()
        .and_then(|c| c.database_url.clone())
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .filter(|s| !s.trim().is_empty())
}

/// Lightweight game row for listings.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GameSummary {
    pub game_id: String,
    pub run_id: String,
    pub schedule_label: String,
    pub winner: String,
    pub win_condition: String,
    pub rounds: i32,
    pub duration_ms: i64,
}

/// Per-model aggregation across a run (SQL-computed).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelMetricSummary {
    pub model_id: String,
    pub is_anchor: bool,
    pub seats: i64,
    pub suspicion_brier: Option<f64>,
    pub liberal_suspicion_brier: Option<f64>,
    pub goal_aligned_num: i64,
    pub goal_aligned_den: i64,
    pub throughput_num: i64,
    pub throughput_den: i64,
    pub exec_hits: i64,
    pub exec_shots: i64,
    pub malformed: i64,
    pub illegal: i64,
    pub forced: i64,
    pub transport: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunSummary {
    pub run_id: String,
    pub mode: String,
    pub status: String,
    pub games_played: i64,
    pub summary: Option<serde_json::Value>,
}

impl Store {
    /// Connect and run embedded migrations.
    pub async fn connect(url: &str) -> StoreResult<Self> {
        let pool = PgPoolOptions::new().max_connections(8).connect(url).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn create_run(
        &self,
        run_id: &str,
        mode: &str,
        spec: &serde_json::Value,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO sh_runs (id, mode, spec) VALUES ($1, $2, $3)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(run_id)
        .bind(mode)
        .bind(spec)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_run_spec(
        &self,
        run_id: &str,
    ) -> StoreResult<Option<(String, serde_json::Value)>> {
        let row = sqlx::query("SELECT mode, spec FROM sh_runs WHERE id = $1")
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| (r.get("mode"), r.get("spec"))))
    }

    pub async fn set_run_status(
        &self,
        run_id: &str,
        status: &str,
        summary: Option<&serde_json::Value>,
    ) -> StoreResult<()> {
        sqlx::query(
            "UPDATE sh_runs SET status = $2, summary = COALESCE($3, summary), updated_at = now()
             WHERE id = $1",
        )
        .bind(run_id)
        .bind(status)
        .bind(summary)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_runs(&self) -> StoreResult<Vec<RunSummary>> {
        let rows = sqlx::query(
            "SELECT r.id, r.mode, r.status, r.summary,
                    (SELECT count(*) FROM sh_games g WHERE g.run_id = r.id) AS games
             FROM sh_runs r ORDER BY r.created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| RunSummary {
                run_id: r.get("id"),
                mode: r.get("mode"),
                status: r.get("status"),
                games_played: r.get("games"),
                summary: r.get("summary"),
            })
            .collect())
    }

    pub async fn game_exists(&self, game_id: &str) -> StoreResult<bool> {
        let row = sqlx::query("SELECT 1 AS one FROM sh_games WHERE id = $1")
            .bind(game_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    /// Persist a finished game with its per-seat metrics, atomically.
    pub async fn insert_game(
        &self,
        run_id: &str,
        record: &GameRecord,
        metrics: &[SeatMetrics],
    ) -> StoreResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO sh_games (id, run_id, seed, schedule_label, winner, win_condition, rounds, duration_ms, record)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (id) DO NOTHING",
        )
        .bind(&record.game_id)
        .bind(run_id)
        .bind(record.seed as i64)
        .bind(&record.schedule_label)
        .bind(record.winner.as_str())
        .bind(format!("{:?}", record.win_condition))
        .bind(record.rounds as i32)
        .bind(record.duration_ms as i64)
        .bind(serde_json::to_value(record).expect("GameRecord serializes"))
        .execute(&mut *tx)
        .await?;

        for (seat, m) in record.seats.iter().zip(metrics.iter()) {
            sqlx::query(
                "INSERT INTO sh_seats (game_id, seat, model_id, agent_kind, scaffold_version, role,
                    is_anchor, survived, won, malformed, illegal, forced, transport,
                    prompt_tokens, completion_tokens, suspicion_brier,
                    goal_aligned_num, goal_aligned_den, throughput_num, throughput_den,
                    exec_hits, exec_shots, hitler_survived_rounds)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)
                 ON CONFLICT (game_id, seat) DO NOTHING",
            )
            .bind(&record.game_id)
            .bind(seat.seat as i16)
            .bind(&seat.model_id)
            .bind(&seat.agent_kind)
            .bind(&seat.scaffold_version)
            .bind(seat.role.as_str())
            .bind(seat.is_anchor)
            .bind(seat.survived)
            .bind(seat.won)
            .bind(seat.reliability.malformed_outputs as i32)
            .bind(seat.reliability.illegal_moves as i32)
            .bind(seat.reliability.forced_defaults as i32)
            .bind(seat.reliability.transport_failures as i32)
            .bind(seat.usage.prompt_tokens as i64)
            .bind(seat.usage.completion_tokens as i64)
            .bind(m.suspicion_brier)
            .bind(m.goal_aligned_num as i32)
            .bind(m.goal_aligned_den as i32)
            .bind(m.throughput_num as i32)
            .bind(m.throughput_den as i32)
            .bind(m.exec_hits as i32)
            .bind(m.exec_shots as i32)
            .bind(m.hitler_survived_rounds.map(|r| r as i32))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    pub async fn list_games(&self, run_id: &str) -> StoreResult<Vec<GameSummary>> {
        let rows = sqlx::query(
            "SELECT id, run_id, schedule_label, winner, win_condition, rounds, duration_ms
             FROM sh_games WHERE run_id = $1 ORDER BY created_at",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| GameSummary {
                game_id: r.get("id"),
                run_id: r.get("run_id"),
                schedule_label: r.get("schedule_label"),
                winner: r.get("winner"),
                win_condition: r.get("win_condition"),
                rounds: r.get("rounds"),
                duration_ms: r.get("duration_ms"),
            })
            .collect())
    }

    pub async fn get_game(&self, game_id: &str) -> StoreResult<Option<GameRecord>> {
        let row = sqlx::query("SELECT record FROM sh_games WHERE id = $1")
            .bind(game_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|r| serde_json::from_value(r.get("record")).ok()))
    }

    /// Every stored game of a run, oldest first (rating recomputation).
    pub async fn run_records(&self, run_id: &str) -> StoreResult<Vec<GameRecord>> {
        let rows = sqlx::query("SELECT record FROM sh_games WHERE run_id = $1 ORDER BY created_at")
            .bind(run_id)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|r| {
                serde_json::from_value(r.get("record"))
                    .map_err(|e| sqlx::Error::Decode(Box::new(e)))
            })
            .collect()
    }

    pub async fn save_ratings(&self, run_id: &str, table: &RatingTable) -> StoreResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM sh_ratings WHERE run_id = $1")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        for ((model_id, role), st) in &table.entities {
            sqlx::query(
                "INSERT INTO sh_ratings (run_id, model_id, role, mu, sigma, games, wins, is_anchor)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
            )
            .bind(run_id)
            .bind(model_id)
            .bind(role.as_str())
            .bind(st.mu)
            .bind(st.sigma)
            .bind(st.games as i32)
            .bind(st.wins as i32)
            .bind(st.is_anchor)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    /// Per-model aggregation of the objective metric suite + reliability
    /// counters + token cost across one run.
    pub async fn model_metric_summary(&self, run_id: &str) -> StoreResult<Vec<ModelMetricSummary>> {
        let rows = sqlx::query(
            "SELECT s.model_id,
                    bool_or(s.is_anchor) AS is_anchor,
                    count(*) AS seats,
                    avg(s.suspicion_brier) AS suspicion_brier,
                    avg(s.suspicion_brier) FILTER (WHERE s.role = 'Liberal') AS liberal_suspicion_brier,
                    sum(s.goal_aligned_num)::bigint AS ga_num, sum(s.goal_aligned_den)::bigint AS ga_den,
                    sum(s.throughput_num)::bigint AS tp_num, sum(s.throughput_den)::bigint AS tp_den,
                    sum(s.exec_hits)::bigint AS exec_hits, sum(s.exec_shots)::bigint AS exec_shots,
                    sum(s.malformed)::bigint AS malformed, sum(s.illegal)::bigint AS illegal,
                    sum(s.forced)::bigint AS forced, sum(s.transport)::bigint AS transport,
                    sum(s.prompt_tokens)::bigint AS prompt_tokens,
                    sum(s.completion_tokens)::bigint AS completion_tokens
             FROM sh_seats s JOIN sh_games g ON g.id = s.game_id
             WHERE g.run_id = $1
             GROUP BY s.model_id ORDER BY s.model_id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ModelMetricSummary {
                model_id: r.get("model_id"),
                is_anchor: r.get("is_anchor"),
                seats: r.get("seats"),
                suspicion_brier: r.get("suspicion_brier"),
                liberal_suspicion_brier: r.get("liberal_suspicion_brier"),
                goal_aligned_num: r.get::<i64, _>("ga_num"),
                goal_aligned_den: r.get::<i64, _>("ga_den"),
                throughput_num: r.get::<i64, _>("tp_num"),
                throughput_den: r.get::<i64, _>("tp_den"),
                exec_hits: r.get::<i64, _>("exec_hits"),
                exec_shots: r.get::<i64, _>("exec_shots"),
                malformed: r.get::<i64, _>("malformed"),
                illegal: r.get::<i64, _>("illegal"),
                forced: r.get::<i64, _>("forced"),
                transport: r.get::<i64, _>("transport"),
                prompt_tokens: r.get::<i64, _>("prompt_tokens"),
                completion_tokens: r.get::<i64, _>("completion_tokens"),
            })
            .collect())
    }

    pub async fn load_ratings(&self, run_id: &str) -> StoreResult<RatingTable> {
        let rows = sqlx::query(
            "SELECT model_id, role, mu, sigma, games, wins, is_anchor
             FROM sh_ratings WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        let mut table = RatingTable::default();
        for r in rows {
            let role = match r.get::<String, _>("role").as_str() {
                "Liberal" => Role::Liberal,
                "Fascist" => Role::Fascist,
                "Hitler" => Role::Hitler,
                other => {
                    return Err(sqlx::Error::Decode(
                        format!("unknown role in sh_ratings: {other}").into(),
                    ))
                }
            };
            table.entities.insert(
                (r.get("model_id"), role),
                RatingState {
                    mu: r.get("mu"),
                    sigma: r.get("sigma"),
                    games: r.get::<i32, _>("games") as u32,
                    wins: r.get::<i32, _>("wins") as u32,
                    is_anchor: r.get("is_anchor"),
                },
            );
        }
        Ok(table)
    }
}
