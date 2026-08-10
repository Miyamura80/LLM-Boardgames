//! Postgres persistence for Catan runs, games, seat metrics, and ratings —
//! the `catan_*` tables, parallel to the SH store (unified only if a third
//! game ever lands).

use super::metrics::SeatMetrics;
use super::rating::{RatingState, RatingTable};
use super::runner::GameRecord;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;

pub type StoreResult<T> = Result<T, sqlx::Error>;

pub use crate::secret_hitler::store::database_url;

pub struct CatanStore {
    pool: PgPool,
}

/// Lightweight game row for listings.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GameSummary {
    pub game_id: String,
    pub run_id: String,
    pub schedule_label: String,
    pub winner: i16,
    pub turns: i32,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunSummary {
    pub run_id: String,
    pub mode: String,
    pub status: String,
    pub games_played: i64,
    pub summary: Option<serde_json::Value>,
}

/// Per-model aggregation across a run (SQL-computed).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelMetricSummary {
    pub model_id: String,
    pub is_anchor: bool,
    pub seats: i64,
    pub avg_final_vp: Option<f64>,
    pub avg_placement: Option<f64>,
    pub production_actual: i64,
    pub production_expected: f64,
    pub placement_pips_pct: Option<f64>,
    pub trades_proposed: i64,
    pub trades_executed: i64,
    pub robber_moves: i64,
    pub robber_on_leader: i64,
    pub malformed: i64,
    pub illegal: i64,
    pub forced: i64,
    pub transport: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

impl CatanStore {
    /// Connect and run embedded migrations (shared migration set with SH).
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
            "INSERT INTO catan_runs (id, mode, spec) VALUES ($1, $2, $3)
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
        let row = sqlx::query("SELECT mode, spec FROM catan_runs WHERE id = $1")
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
            "UPDATE catan_runs SET status = $2, summary = COALESCE($3, summary), updated_at = now()
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
                    (SELECT count(*) FROM catan_games g WHERE g.run_id = r.id) AS games
             FROM catan_runs r ORDER BY r.created_at DESC",
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
        let row = sqlx::query("SELECT 1 AS one FROM catan_games WHERE id = $1")
            .bind(game_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    /// Persist a finished game with its per-seat metrics, atomically. Returns
    /// `false` when the game id already existed (concurrent resume).
    pub async fn insert_game(
        &self,
        run_id: &str,
        record: &GameRecord,
        metrics: &[SeatMetrics],
    ) -> StoreResult<bool> {
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO catan_games (id, run_id, seed, schedule_label, winner, turns, duration_ms, record)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (id) DO NOTHING",
        )
        .bind(&record.game_id)
        .bind(run_id)
        .bind(format!("{:016x}", record.seed))
        .bind(&record.schedule_label)
        .bind(record.winner as i16)
        .bind(record.turns as i32)
        .bind(record.duration_ms as i64)
        .bind(serde_json::to_value(record).expect("GameRecord serializes"))
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;

        for (seat, m) in record.seats.iter().zip(metrics.iter()) {
            sqlx::query(
                "INSERT INTO catan_seats (game_id, seat, model_id, agent_kind, scaffold_version,
                    is_anchor, final_vp, public_vp, placement, won, knights,
                    malformed, illegal, forced, transport, prompt_tokens, completion_tokens,
                    production_actual, production_expected, placement_pips_pct,
                    trades_proposed, trades_executed, trade_cards_out, trade_cards_in,
                    robber_moves, robber_on_leader)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26)
                 ON CONFLICT (game_id, seat) DO NOTHING",
            )
            .bind(&record.game_id)
            .bind(seat.seat as i16)
            .bind(&seat.model_id)
            .bind(&seat.agent_kind)
            .bind(&seat.scaffold_version)
            .bind(seat.is_anchor)
            .bind(seat.final_vp as i16)
            .bind(seat.public_vp as i16)
            .bind(seat.placement as i16)
            .bind(seat.won)
            .bind(seat.knights_played as i16)
            .bind(seat.reliability.malformed_outputs as i32)
            .bind(seat.reliability.illegal_moves as i32)
            .bind(seat.reliability.forced_defaults as i32)
            .bind(seat.reliability.transport_failures as i32)
            .bind(seat.usage.prompt_tokens as i64)
            .bind(seat.usage.completion_tokens as i64)
            .bind(m.production_actual as i32)
            .bind(m.production_expected)
            .bind(m.placement_pips_pct)
            .bind(m.trades_proposed as i32)
            .bind(m.trades_executed as i32)
            .bind(m.trade_cards_out as i32)
            .bind(m.trade_cards_in as i32)
            .bind(m.robber_moves as i32)
            .bind(m.robber_on_leader as i32)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(inserted)
    }

    pub async fn list_games(&self, run_id: &str) -> StoreResult<Vec<GameSummary>> {
        let rows = sqlx::query(
            "SELECT id, run_id, schedule_label, winner, turns, duration_ms
             FROM catan_games WHERE run_id = $1 ORDER BY created_at",
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
                turns: r.get("turns"),
                duration_ms: r.get("duration_ms"),
            })
            .collect())
    }

    pub async fn get_game(&self, game_id: &str) -> StoreResult<Option<GameRecord>> {
        let row = sqlx::query("SELECT record FROM catan_games WHERE id = $1")
            .bind(game_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| {
            serde_json::from_value(r.get("record")).map_err(|e| sqlx::Error::Decode(Box::new(e)))
        })
        .transpose()
    }

    pub async fn run_records(&self, run_id: &str) -> StoreResult<Vec<GameRecord>> {
        let rows =
            sqlx::query("SELECT record FROM catan_games WHERE run_id = $1 ORDER BY created_at")
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
        sqlx::query("DELETE FROM catan_ratings WHERE run_id = $1")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        for ((model_id, seat), st) in &table.entities {
            sqlx::query(
                "INSERT INTO catan_ratings (run_id, model_id, seat, mu, sigma, games, wins, is_anchor)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
            )
            .bind(run_id)
            .bind(model_id)
            .bind(*seat as i16)
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

    pub async fn load_ratings(&self, run_id: &str) -> StoreResult<RatingTable> {
        let rows = sqlx::query(
            "SELECT model_id, seat, mu, sigma, games, wins, is_anchor
             FROM catan_ratings WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        let mut table = RatingTable::default();
        for r in rows {
            table.entities.insert(
                (r.get("model_id"), r.get::<i16, _>("seat") as u8),
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

    pub async fn model_metric_summary(&self, run_id: &str) -> StoreResult<Vec<ModelMetricSummary>> {
        let rows = sqlx::query(
            "SELECT s.model_id,
                    bool_or(s.is_anchor) AS is_anchor,
                    count(*) AS seats,
                    avg(s.final_vp)::float8 AS avg_final_vp,
                    avg(s.placement)::float8 AS avg_placement,
                    sum(s.production_actual)::bigint AS production_actual,
                    sum(s.production_expected) AS production_expected,
                    avg(s.placement_pips_pct) AS placement_pips_pct,
                    sum(s.trades_proposed)::bigint AS trades_proposed,
                    sum(s.trades_executed)::bigint AS trades_executed,
                    sum(s.robber_moves)::bigint AS robber_moves,
                    sum(s.robber_on_leader)::bigint AS robber_on_leader,
                    sum(s.malformed)::bigint AS malformed, sum(s.illegal)::bigint AS illegal,
                    sum(s.forced)::bigint AS forced, sum(s.transport)::bigint AS transport,
                    sum(s.prompt_tokens)::bigint AS prompt_tokens,
                    sum(s.completion_tokens)::bigint AS completion_tokens
             FROM catan_seats s JOIN catan_games g ON g.id = s.game_id
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
                avg_final_vp: r.get("avg_final_vp"),
                avg_placement: r.get("avg_placement"),
                production_actual: r.get("production_actual"),
                production_expected: r.get("production_expected"),
                placement_pips_pct: r.get("placement_pips_pct"),
                trades_proposed: r.get("trades_proposed"),
                trades_executed: r.get("trades_executed"),
                robber_moves: r.get("robber_moves"),
                robber_on_leader: r.get("robber_on_leader"),
                malformed: r.get("malformed"),
                illegal: r.get("illegal"),
                forced: r.get("forced"),
                transport: r.get("transport"),
                prompt_tokens: r.get("prompt_tokens"),
                completion_tokens: r.get("completion_tokens"),
            })
            .collect())
    }

    /// Realized model→seat occurrence counts from persisted seat rows.
    pub async fn realized_distribution(
        &self,
        run_id: &str,
    ) -> StoreResult<BTreeMap<String, BTreeMap<String, u32>>> {
        let rows = sqlx::query(
            "SELECT s.model_id, s.seat, count(*)::bigint AS n
             FROM catan_seats s JOIN catan_games g ON g.id = s.game_id
             WHERE g.run_id = $1 GROUP BY s.model_id, s.seat",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        let mut dist: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
        for r in rows {
            let model_id: String = r.get("model_id");
            let seat: i16 = r.get("seat");
            let n = r.get::<i64, _>("n") as u32;
            *dist
                .entry(model_id)
                .or_default()
                .entry(format!("seat{seat}"))
                .or_default() += n;
        }
        Ok(dist)
    }
}
