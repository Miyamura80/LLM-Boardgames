//! Postgres persistence for Codenames runs, games, seat metrics, and ratings —
//! the `codenames_*` tables (US-CN11), parallel to `sh_*` and `catan_*`; the
//! three-way unification is due but split into its own PR (§9 decision 9).
//! Queries are runtime (`sqlx::query` + `Row::get`) like the other two stores,
//! so a build never needs a live database or an offline schema cache.

use super::metrics::SeatMetrics;
use super::rating::{side_name, RatingState, RatingTable, SideStats};
use super::runner::GameRecord;
use super::types::{Role, Team};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;

pub type StoreResult<T> = Result<T, sqlx::Error>;

/// Connection-string resolution (config `database_url` → `DATABASE_URL`),
/// shared with the other games exactly as the Catan store reuses it.
pub use crate::secret_hitler::store::database_url;

pub struct CodenamesStore {
    pool: PgPool,
}

/// Lightweight game row for listings.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GameSummary {
    pub game_id: String,
    pub run_id: String,
    pub schedule_label: String,
    /// `a` (starting team) or `b`.
    pub winner: String,
    pub end_reason: String,
    pub turns: i32,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunSummary {
    pub run_id: String,
    /// The discriminator a cross-game run picker keys on: always `codenames`.
    pub game: String,
    pub mode: String,
    pub status: String,
    pub games_played: i64,
    pub summary: Option<serde_json::Value>,
}

/// Per-model aggregation across a run (SQL-computed). Numerators and their
/// denominators travel together, so partial runs aggregate correctly.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelMetricSummary {
    pub model_id: String,
    pub is_anchor: bool,
    pub seats: i64,
    pub wins: i64,
    pub assassin_losses: i64,
    pub clues_given: i64,
    pub clue_number_sum: i64,
    pub agents_found: i64,
    pub clue_yield_den: i64,
    pub enemy_hits_caused: i64,
    pub bystander_hits_caused: i64,
    pub assassin_hits_caused: i64,
    pub guesses: i64,
    pub guess_hits: i64,
    pub first_guesses: i64,
    pub first_guess_hits: i64,
    pub bonus_guesses: i64,
    pub bonus_misses: i64,
    pub passes: i64,
    pub pass_hazard_sum: Option<f64>,
    pub assassin_hits: i64,
    pub malformed: i64,
    pub illegal: i64,
    pub forced: i64,
    pub transport: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

impl CodenamesStore {
    /// Connect and run embedded migrations (one migration set for all games).
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
            "INSERT INTO codenames_runs (id, mode, spec) VALUES ($1, $2, $3)
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
        let row = sqlx::query("SELECT mode, spec FROM codenames_runs WHERE id = $1")
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
            "UPDATE codenames_runs SET status = $2, summary = COALESCE($3, summary),
                    updated_at = now()
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
                    (SELECT count(*) FROM codenames_games g WHERE g.run_id = r.id) AS games
             FROM codenames_runs r ORDER BY r.created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| RunSummary {
                run_id: r.get("id"),
                game: "codenames".into(),
                mode: r.get("mode"),
                status: r.get("status"),
                games_played: r.get("games"),
                summary: r.get("summary"),
            })
            .collect())
    }

    pub async fn game_exists(&self, game_id: &str) -> StoreResult<bool> {
        let row = sqlx::query("SELECT 1 AS one FROM codenames_games WHERE id = $1")
            .bind(game_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    /// Persist a game + its seat metrics atomically; `false` = already stored.
    pub async fn insert_game(
        &self,
        run_id: &str,
        record: &GameRecord,
        metrics: &[SeatMetrics],
    ) -> StoreResult<bool> {
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO codenames_games
                (id, run_id, seed, schedule_label, winner, end_reason, turns, duration_ms, record)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (id) DO NOTHING",
        )
        .bind(&record.game_id)
        .bind(run_id)
        .bind(format!("{:016x}", record.seed))
        .bind(&record.schedule_label)
        .bind(record.winner.as_str())
        .bind(record.end_reason.as_str())
        .bind(record.turns as i32)
        .bind(record.duration_ms as i64)
        .bind(serde_json::to_value(record).expect("GameRecord serializes"))
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;

        for (seat, m) in record.seats.iter().zip(metrics.iter()) {
            sqlx::query(
                "INSERT INTO codenames_seats (game_id, seat, role, side, model_id, agent_kind,
                    scaffold_version, is_anchor, won, assassin_loss,
                    malformed, illegal, forced, transport, prompt_tokens, completion_tokens,
                    clues_given, clue_number_sum, agents_found, clue_yield_den,
                    enemy_hits_caused, bystander_hits_caused, assassin_hits_caused,
                    guesses, guess_hits, first_guesses, first_guess_hits,
                    bonus_guesses, bonus_misses, passes, pass_hazard_sum, assassin_hits)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,
                         $21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32)
                 ON CONFLICT (game_id, seat) DO NOTHING",
            )
            .bind(&record.game_id)
            .bind(seat.seat as i16)
            .bind(seat.role.as_str())
            .bind(side_name(seat.team))
            .bind(&seat.model_id)
            .bind(&seat.agent_kind)
            .bind(&seat.scaffold_version)
            .bind(seat.is_anchor)
            .bind(seat.won)
            .bind(m.assassin_loss)
            .bind(seat.reliability.malformed_outputs as i32)
            .bind(seat.reliability.illegal_moves as i32)
            .bind(seat.reliability.forced_defaults as i32)
            .bind(seat.reliability.transport_failures as i32)
            .bind(seat.usage.prompt_tokens as i64)
            .bind(seat.usage.completion_tokens as i64)
            .bind(m.clues_given as i32)
            .bind(m.clue_number_sum as i32)
            .bind(m.agents_found as i32)
            .bind(m.clue_yield_den as i32)
            .bind(m.enemy_hits_caused as i32)
            .bind(m.bystander_hits_caused as i32)
            .bind(m.assassin_hits_caused as i32)
            .bind(m.guesses as i32)
            .bind(m.guess_hits as i32)
            .bind(m.first_guesses as i32)
            .bind(m.first_guess_hits as i32)
            .bind(m.bonus_guesses as i32)
            .bind(m.bonus_misses as i32)
            .bind(m.passes as i32)
            .bind(m.pass_hazard_sum)
            .bind(m.assassin_hits as i32)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(inserted)
    }

    pub async fn list_games(&self, run_id: &str) -> StoreResult<Vec<GameSummary>> {
        let rows = sqlx::query(
            "SELECT id, run_id, schedule_label, winner, end_reason, turns, duration_ms
             FROM codenames_games WHERE run_id = $1 ORDER BY created_at",
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
                end_reason: r.get("end_reason"),
                turns: r.get("turns"),
                duration_ms: r.get("duration_ms"),
            })
            .collect())
    }

    pub async fn get_game(&self, game_id: &str) -> StoreResult<Option<GameRecord>> {
        let row = sqlx::query("SELECT record FROM codenames_games WHERE id = $1")
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
            sqlx::query("SELECT record FROM codenames_games WHERE run_id = $1 ORDER BY created_at")
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
        sqlx::query("DELETE FROM codenames_ratings WHERE run_id = $1")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        for ((model_id, role), st) in &table.entities {
            sqlx::query(
                "INSERT INTO codenames_ratings
                    (run_id, model_id, role, mu, sigma, games, wins, is_anchor)
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

    /// Load the rating entities plus the per-side diagnostic — the latter is
    /// re-aggregated from the seat rows, since side is never a rating key.
    pub async fn load_ratings(&self, run_id: &str) -> StoreResult<RatingTable> {
        let rows = sqlx::query(
            "SELECT model_id, role, mu, sigma, games, wins, is_anchor
             FROM codenames_ratings WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        let mut table = RatingTable::default();
        for r in rows {
            let Some(role) = role_from_str(r.get::<String, _>("role").as_str()) else {
                continue;
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

        let rows = sqlx::query(
            "SELECT s.model_id, s.side, count(*)::bigint AS games,
                    count(*) FILTER (WHERE s.won)::bigint AS wins
             FROM codenames_seats s JOIN codenames_games g ON g.id = s.game_id
             WHERE g.run_id = $1 GROUP BY s.model_id, s.side",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        for r in rows {
            let Some(team) = team_from_side(r.get::<String, _>("side").as_str()) else {
                continue;
            };
            table.sides.insert(
                (r.get("model_id"), team),
                SideStats {
                    games: r.get::<i64, _>("games") as u32,
                    wins: r.get::<i64, _>("wins") as u32,
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
                    count(*) FILTER (WHERE s.won)::bigint AS wins,
                    count(*) FILTER (WHERE s.assassin_loss)::bigint AS assassin_losses,
                    sum(s.clues_given)::bigint AS clues_given,
                    sum(s.clue_number_sum)::bigint AS clue_number_sum,
                    sum(s.agents_found)::bigint AS agents_found,
                    sum(s.clue_yield_den)::bigint AS clue_yield_den,
                    sum(s.enemy_hits_caused)::bigint AS enemy_hits_caused,
                    sum(s.bystander_hits_caused)::bigint AS bystander_hits_caused,
                    sum(s.assassin_hits_caused)::bigint AS assassin_hits_caused,
                    sum(s.guesses)::bigint AS guesses, sum(s.guess_hits)::bigint AS guess_hits,
                    sum(s.first_guesses)::bigint AS first_guesses,
                    sum(s.first_guess_hits)::bigint AS first_guess_hits,
                    sum(s.bonus_guesses)::bigint AS bonus_guesses,
                    sum(s.bonus_misses)::bigint AS bonus_misses,
                    sum(s.passes)::bigint AS passes, sum(s.pass_hazard_sum) AS pass_hazard_sum,
                    sum(s.assassin_hits)::bigint AS assassin_hits,
                    sum(s.malformed)::bigint AS malformed, sum(s.illegal)::bigint AS illegal,
                    sum(s.forced)::bigint AS forced, sum(s.transport)::bigint AS transport,
                    sum(s.prompt_tokens)::bigint AS prompt_tokens,
                    sum(s.completion_tokens)::bigint AS completion_tokens
             FROM codenames_seats s JOIN codenames_games g ON g.id = s.game_id
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
                wins: r.get("wins"),
                assassin_losses: r.get("assassin_losses"),
                clues_given: r.get("clues_given"),
                clue_number_sum: r.get("clue_number_sum"),
                agents_found: r.get("agents_found"),
                clue_yield_den: r.get("clue_yield_den"),
                enemy_hits_caused: r.get("enemy_hits_caused"),
                bystander_hits_caused: r.get("bystander_hits_caused"),
                assassin_hits_caused: r.get("assassin_hits_caused"),
                guesses: r.get("guesses"),
                guess_hits: r.get("guess_hits"),
                first_guesses: r.get("first_guesses"),
                first_guess_hits: r.get("first_guess_hits"),
                bonus_guesses: r.get("bonus_guesses"),
                bonus_misses: r.get("bonus_misses"),
                passes: r.get("passes"),
                pass_hazard_sum: r.get("pass_hazard_sum"),
                assassin_hits: r.get("assassin_hits"),
                malformed: r.get("malformed"),
                illegal: r.get("illegal"),
                forced: r.get("forced"),
                transport: r.get("transport"),
                prompt_tokens: r.get("prompt_tokens"),
                completion_tokens: r.get("completion_tokens"),
            })
            .collect())
    }

    /// Realized model → seat / role / side occurrence counts, from the games
    /// actually persisted (never the plan: a partial run must not over-report).
    pub async fn realized_distribution(
        &self,
        run_id: &str,
    ) -> StoreResult<BTreeMap<String, BTreeMap<String, u32>>> {
        let rows = sqlx::query(
            "SELECT s.model_id, s.seat, s.role, s.side, count(*)::bigint AS n
             FROM codenames_seats s JOIN codenames_games g ON g.id = s.game_id
             WHERE g.run_id = $1 GROUP BY s.model_id, s.seat, s.role, s.side",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        let mut dist: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
        for r in rows {
            let model_id: String = r.get("model_id");
            let seat: i16 = r.get("seat");
            let role: String = r.get("role");
            let side: String = r.get("side");
            let n = r.get::<i64, _>("n") as u32;
            let by = dist.entry(model_id).or_default();
            *by.entry(format!("seat{seat}")).or_default() += n;
            *by.entry(format!("role:{role}")).or_default() += n;
            *by.entry(format!("side:{side}")).or_default() += n;
        }
        Ok(dist)
    }
}

fn role_from_str(s: &str) -> Option<Role> {
    match s {
        "spymaster" => Some(Role::Spymaster),
        "operative" => Some(Role::Operative),
        _ => None,
    }
}

fn team_from_side(s: &str) -> Option<Team> {
    match s {
        "starting" => Some(Team::A),
        "second" => Some(Team::B),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text encodings the schema stores must round-trip, or a rating row
    /// would come back as a different entity than it went in as.
    #[test]
    fn stored_enum_encodings_round_trip() {
        for role in [Role::Spymaster, Role::Operative] {
            assert_eq!(role_from_str(role.as_str()), Some(role));
        }
        for team in [Team::A, Team::B] {
            assert_eq!(team_from_side(side_name(team)), Some(team));
        }
        assert_eq!(role_from_str("liberal"), None);
        assert_eq!(team_from_side("a"), None);
    }
}
