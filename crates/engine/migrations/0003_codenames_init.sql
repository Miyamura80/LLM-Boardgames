-- Codenames eval store: parallel to the sh_* and catan_* tables
-- (PRD-codenames-evals US-CN11 / §9 decision 9). Unifying the three schemas is
-- acknowledged as due and deliberately deferred to its own follow-up PR, so
-- nothing here touches the existing tables.
--
-- The full GameRecord JSONB on codenames_games is the replay source of truth;
-- seat columns exist for SQL aggregation only. The key card is NOT stored: it
-- is engine state, reconstructed from (seed, wordlist) by
-- codenames::metrics::key_card_for, and codenames_games.record carries the
-- wordlist hash that pins which pool that must be.

CREATE TABLE codenames_runs (
    id         TEXT PRIMARY KEY,
    mode       TEXT NOT NULL,
    spec       JSONB NOT NULL,
    status     TEXT NOT NULL DEFAULT 'running',
    summary    JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE codenames_games (
    id             TEXT PRIMARY KEY,
    run_id         TEXT NOT NULL REFERENCES codenames_runs(id),
    -- Hex-encoded to dodge BIGINT sign-wrap on u64 seeds (sh_games precedent).
    seed           TEXT NOT NULL,
    schedule_label TEXT NOT NULL,
    -- 'a' (the starting, nine-agent team) or 'b'.
    winner         TEXT NOT NULL,
    -- 'agents-found' or 'assassin'.
    end_reason     TEXT NOT NULL,
    turns          INTEGER NOT NULL,
    duration_ms    BIGINT NOT NULL,
    record         JSONB NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX codenames_games_run_idx ON codenames_games (run_id);

CREATE TABLE codenames_seats (
    game_id             TEXT NOT NULL REFERENCES codenames_games(id),
    -- 0 = A spymaster, 1 = A operative, 2 = B spymaster, 3 = B operative.
    seat                SMALLINT NOT NULL,
    role                TEXT NOT NULL,
    -- The mirrored schedule dimension: 'starting' or 'second'.
    side                TEXT NOT NULL,
    model_id            TEXT NOT NULL,
    agent_kind          TEXT NOT NULL,
    scaffold_version    TEXT NOT NULL,
    is_anchor           BOOLEAN NOT NULL,
    won                 BOOLEAN NOT NULL,
    assassin_loss       BOOLEAN NOT NULL,
    -- Reliability counters: never folded into play quality.
    malformed           INTEGER NOT NULL,
    illegal             INTEGER NOT NULL,
    forced              INTEGER NOT NULL,
    transport           INTEGER NOT NULL,
    prompt_tokens       BIGINT NOT NULL,
    completion_tokens   BIGINT NOT NULL,
    -- Spymaster metrics (US-CN10).
    clues_given         INTEGER NOT NULL,
    clue_number_sum     INTEGER NOT NULL,
    agents_found        INTEGER NOT NULL,
    clue_yield_den      INTEGER NOT NULL,
    enemy_hits_caused   INTEGER NOT NULL,
    bystander_hits_caused INTEGER NOT NULL,
    assassin_hits_caused  INTEGER NOT NULL,
    -- Operative metrics (US-CN10).
    guesses             INTEGER NOT NULL,
    guess_hits          INTEGER NOT NULL,
    first_guesses       INTEGER NOT NULL,
    first_guess_hits    INTEGER NOT NULL,
    bonus_guesses       INTEGER NOT NULL,
    bonus_misses        INTEGER NOT NULL,
    passes              INTEGER NOT NULL,
    pass_hazard_sum     DOUBLE PRECISION NOT NULL,
    assassin_hits       INTEGER NOT NULL,
    PRIMARY KEY (game_id, seat)
);

-- Rating entities are (model_id, role); side is a diagnostic aggregated from
-- codenames_seats, never a rating key (US-CN09).
CREATE TABLE codenames_ratings (
    run_id    TEXT NOT NULL,
    model_id  TEXT NOT NULL,
    role      TEXT NOT NULL,
    mu        DOUBLE PRECISION NOT NULL,
    sigma     DOUBLE PRECISION NOT NULL,
    games     INTEGER NOT NULL,
    wins      INTEGER NOT NULL,
    is_anchor BOOLEAN NOT NULL,
    PRIMARY KEY (run_id, model_id, role)
);
