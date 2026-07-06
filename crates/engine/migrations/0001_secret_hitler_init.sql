-- Secret Hitler eval store: runs, games (full replayable record as JSONB),
-- per-seat metric rows, and role-conditioned ratings.

CREATE TABLE IF NOT EXISTS sh_runs (
    id          TEXT PRIMARY KEY,
    mode        TEXT NOT NULL,              -- controlled | arena
    spec        JSONB NOT NULL,             -- full match spec for resume
    status      TEXT NOT NULL DEFAULT 'running',  -- running | complete | failed
    summary     JSONB,                      -- leaderboard + distribution on finalize
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS sh_games (
    id             TEXT PRIMARY KEY,
    run_id         TEXT NOT NULL REFERENCES sh_runs(id) ON DELETE CASCADE,
    -- u64 seed as zero-padded hex (BIGINT would sign-wrap high-bit seeds)
    seed           TEXT NOT NULL,
    schedule_label TEXT NOT NULL,
    winner         TEXT NOT NULL,
    win_condition  TEXT NOT NULL,
    rounds         INT NOT NULL,
    duration_ms    BIGINT NOT NULL,
    record         JSONB NOT NULL,          -- complete GameRecord (replay source)
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_sh_games_run ON sh_games(run_id);

CREATE TABLE IF NOT EXISTS sh_seats (
    game_id           TEXT NOT NULL REFERENCES sh_games(id) ON DELETE CASCADE,
    seat              SMALLINT NOT NULL,
    model_id          TEXT NOT NULL,
    agent_kind        TEXT NOT NULL,
    scaffold_version  TEXT NOT NULL,
    role              TEXT NOT NULL,        -- Liberal | Fascist | Hitler
    is_anchor         BOOLEAN NOT NULL,
    survived          BOOLEAN NOT NULL,
    won               BOOLEAN NOT NULL,
    -- reliability counters (separate from play quality)
    malformed         INT NOT NULL,
    illegal           INT NOT NULL,
    forced            INT NOT NULL,
    transport         INT NOT NULL,
    prompt_tokens     BIGINT NOT NULL,
    completion_tokens BIGINT NOT NULL,
    -- objective metric suite
    suspicion_brier        DOUBLE PRECISION,
    goal_aligned_num       INT NOT NULL,
    goal_aligned_den       INT NOT NULL,
    throughput_num         INT NOT NULL,
    throughput_den         INT NOT NULL,
    exec_hits              INT NOT NULL,
    exec_shots             INT NOT NULL,
    hitler_survived_rounds INT,
    PRIMARY KEY (game_id, seat)
);
CREATE INDEX IF NOT EXISTS idx_sh_seats_model ON sh_seats(model_id);

CREATE TABLE IF NOT EXISTS sh_ratings (
    run_id   TEXT NOT NULL REFERENCES sh_runs(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    role     TEXT NOT NULL,
    mu       DOUBLE PRECISION NOT NULL,
    sigma    DOUBLE PRECISION NOT NULL,
    games    INT NOT NULL,
    wins     INT NOT NULL,
    is_anchor BOOLEAN NOT NULL,
    PRIMARY KEY (run_id, model_id, role)
);
