-- Catan eval store: parallel to the sh_* tables (PRD-catan-evals US-C12).
-- The full GameRecord JSONB on catan_games is the replay source of truth;
-- seat columns exist for SQL aggregation only.

CREATE TABLE catan_runs (
    id         TEXT PRIMARY KEY,
    mode       TEXT NOT NULL,
    spec       JSONB NOT NULL,
    status     TEXT NOT NULL DEFAULT 'running',
    summary    JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE catan_games (
    id             TEXT PRIMARY KEY,
    run_id         TEXT NOT NULL REFERENCES catan_runs(id),
    -- Hex-encoded to dodge BIGINT sign-wrap on u64 seeds (sh_games precedent).
    seed           TEXT NOT NULL,
    schedule_label TEXT NOT NULL,
    winner         SMALLINT NOT NULL,
    turns          INTEGER NOT NULL,
    duration_ms    BIGINT NOT NULL,
    record         JSONB NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX catan_games_run_idx ON catan_games (run_id);

CREATE TABLE catan_seats (
    game_id             TEXT NOT NULL REFERENCES catan_games(id),
    seat                SMALLINT NOT NULL,
    model_id            TEXT NOT NULL,
    agent_kind          TEXT NOT NULL,
    scaffold_version    TEXT NOT NULL,
    is_anchor           BOOLEAN NOT NULL,
    final_vp            SMALLINT NOT NULL,
    public_vp           SMALLINT NOT NULL,
    placement           SMALLINT NOT NULL,
    won                 BOOLEAN NOT NULL,
    knights             SMALLINT NOT NULL,
    malformed           INTEGER NOT NULL,
    illegal             INTEGER NOT NULL,
    forced              INTEGER NOT NULL,
    transport           INTEGER NOT NULL,
    prompt_tokens       BIGINT NOT NULL,
    completion_tokens   BIGINT NOT NULL,
    production_actual   INTEGER NOT NULL,
    production_expected DOUBLE PRECISION NOT NULL,
    placement_pips_pct  DOUBLE PRECISION,
    trades_proposed     INTEGER NOT NULL,
    trades_executed     INTEGER NOT NULL,
    trade_cards_out     INTEGER NOT NULL,
    trade_cards_in      INTEGER NOT NULL,
    robber_moves        INTEGER NOT NULL,
    robber_on_leader    INTEGER NOT NULL,
    PRIMARY KEY (game_id, seat)
);

CREATE TABLE catan_ratings (
    run_id    TEXT NOT NULL,
    model_id  TEXT NOT NULL,
    seat      SMALLINT NOT NULL,
    mu        DOUBLE PRECISION NOT NULL,
    sigma     DOUBLE PRECISION NOT NULL,
    games     INTEGER NOT NULL,
    wins      INTEGER NOT NULL,
    is_anchor BOOLEAN NOT NULL,
    PRIMARY KEY (run_id, model_id, seat)
);
