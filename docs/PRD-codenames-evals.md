# PRD: Codenames-Evals

> An LLM-agent evaluation harness for **Codenames** (standard 2-team game,
> **4 seats**), built as the **third game** on the shared `game_core`
> substrate that [`PRD-catan-evals.md`](PRD-catan-evals.md) extracted. Unlike
> Catan, this PRD requires **no new shared-core machinery**: Codenames is a
> fully sequential game that the existing rethink loop drives as-is. The one
> multi-game debt Catan flagged for "game #3" — storage unification — is
> **acknowledged and deliberately decoupled** (see §5, §9 decision 9).
>
> All decisions below were confirmed by the user (2026-08-11) from a
> pros/cons review; they are recorded in
> [Resolved Decisions](#9-resolved-decisions).

## 1. Introduction / Overview

Codenames is a 2-team word-association game: each team has a **spymaster**
who sees a hidden key card mapping 25 word cards to teams, and gives one-word
+ number clues; the team's **operative** guesses which words the clue points
at. Revealing an own-team agent lets the operative keep guessing; a bystander
or opposing agent ends the turn; the single **assassin** loses the game
instantly. It complements Secret Hitler and Catan in exactly the dimensions
they cannot measure: **semantic abstraction and theory-of-mind over
language itself** — the spymaster must model what the *operative's* model
will associate with a word, and the operative must invert that. There is no
negotiation, no resource economy, and (in the 1-operative form) no table
talk: the entire skill surface is the clue channel.

As with SH and Catan, the **game outcome is objective ground truth** computed
by an authoritative Rust engine. Codenames is two-team with strongly
asymmetric roles, so the headline metric follows **SH's template, not
Catan's**: a Weng-Lin two-team rating keyed `(model_id, role)` where role ∈
{spymaster, operative}, with a role-averaged conservative μ−kσ headline.

### Architecture

```
                     shbench: call / serve / mcp              (unchanged)
                                  │
                    CommandRegistry (inventory)               (unchanged)
            ┌──────────────┬──────┼────────────┬──────────────┐
      sh_* commands  catan_* cmds │  codenames_* cmds    generic cmds
                                  │        (NEW)
    ┌─────────────────────────────▼─────────────────────────┐
    │  game_core  (UNCHANGED — no extraction needed)        │
    │   EngineState { Action, Observation, Decision }       │
    │   rethink loop + forced defaults, Reliability,        │
    │   Visibility::{Public, Private(Seat)} event log,      │
    │   cell_seed schedules                                 │
    └──────┬──────────────┬───────────────┬─────────────────┘
    secret_hitler/     catan/         codenames/   ◄── NEW  (owns its rules,
           │              │               │                  wordlist, prompts,
           │              │               │                  metrics, rating,
           │              │               │                  report template)
           │              │               │
      sh_* tables    catan_* tables  codenames_* tables      (0003 migration;
           │              │               │                   unification is a
           └──────────────┴───────┬───────┘                   separate PR)
                    llm/ + config + store  (shared: client, providers,
                                            retry, keys, DB pool)
```

The turn structure the engine enforces (teams alternate, starting team has 9
agents to the other's 8):

```
  spymaster: give_clue { word, number }        ── engine validates legality
     │                                            (single token, no overlap
     ▼                                             with unrevealed board words)
  operative: guess { word }  ◄────────────┐
     │                                    │
     ▼                                    │
  reveal card ──► own agent? ──yes──► guesses left (≤ number+1)? ──yes──┘
     │  │  │                                   │ no
     │  │  └─► bystander / enemy agent ──► turn passes to other team
     │  └────► assassin ──► INSTANT LOSS
     ▼
  operative may pass instead of guessing (after the mandatory first guess)

  win: all own agents revealed  |  opponent reveals the assassin
```

## 2. Goals

- A **trustworthy Codenames rules engine** (clue legality, guess/reveal
  resolution, assassin, win detection) with an exhaustive test suite — small
  compared to Catan, but still the single source of truth for every metric.
- **Zero `game_core` changes**: the existing `EngineState` contract, rethink
  loop, and `Visibility::{Public, Private}` event log fit as-is. Shared team
  knowledge (nothing in v1 — each team has one spymaster and one operative)
  and role asymmetry are handled entirely inside `observe()`, following SH's
  `known_teammates` precedent.
- **Deterministic clue legality**: the engine — not an LLM judge — is the
  sole authority on whether a clue is legal, so seeded runs stay reproducible
  and the rethink/forced-default machinery covers clue-giving.
- A **role-conditioned headline rating** per model from two-team outcomes,
  with per-role lines always reported, mirroring SH's rating design.
- **Variance control by construction**: candidate-independent mirrored board
  seeds; every candidate sees identical word grids and key cards, from every
  role and both team sides.
- An **embedding-based anchor bot** so ratings are anchored to a real
  clue-giving/guessing floor, not just random legality — the Codenames
  analogue of Catan's `GreedyBuilderBot`.
- Objective, engine-derived metrics (clue efficiency, guess precision,
  assassin discipline); reliability counters separate from play quality.
- Bounded cost: Codenames is the **cheapest game on the harness** (~20–40
  decisions per game across 4 seats, tiny observations).

## 3. User Stories

### US-CN01: Authoritative Codenames engine
**Description:** As an eval author, I need an engine that holds the full
standard-game state and enforces every rule.

**Acceptance Criteria:**
- [ ] Board: 25 word cards drawn seeded-random without replacement from the
  wordlist, laid out 5×5; key card assigns 9 to the starting team (fixed:
  team A starts), 8 to team B, 7 bystanders, 1 assassin, seeded-random.
- [ ] Four fixed seats: 0 = A spymaster, 1 = A operative, 2 = B spymaster,
  3 = B operative.
- [ ] Clue legality (deterministic, engine-enforced): a single alphabetic
  token (hyphens allowed, no spaces/digits), case-insensitive, that is not
  equal to, a substring of, or a superstring of any **unrevealed** board
  word; revealed words become legal (official rule); length-capped. Number
  ∈ 1..=remaining own agents.
- [ ] Guess resolution: revealing an own agent continues the turn (guess cap
  = number + 1); a bystander or opposing agent ends the turn (opposing
  agents still count for the opponent); the assassin ends the game as an
  immediate loss for the guessing team.
- [ ] The operative must make at least one guess per clue (official rule);
  after the first guess, `pass` is legal and ends the turn.
- [ ] Win detection: all of a team's agents revealed (win, including via the
  opponent's mis-guess) or assassin revealed (guessing team loses).
- [ ] All randomness (word draw, key layout, forced-default draws) flows from
  a seedable RNG; identical seed + identical agent decisions → identical
  transcript. A `rules_version` const gates record comparability.
- [ ] Lint/Test/Build checks pass.

### US-CN02: Engine rules test suite
**Acceptance Criteria:**
- [ ] Unit tests per rule cluster: clue token validation (case, hyphens,
  substring/superstring vs unrevealed and vs revealed words, number
  bounds), guess-cap accounting (number + 1), mandatory first guess,
  turn-end on bystander/enemy, assassin loss, win-by-opponent-mis-guess,
  win on last agent mid-guess-streak, seeded board/key determinism.
- [ ] Property tests over randomized seeded games with `RandomLegalBot`:
  every game terminates, revealed-card accounting is conserved, no decision
  is ever offered to a non-pending seat, clue legality never admits an
  unrevealed board word.
- [ ] A scripted-game testkit (fixed board, fixed key) mirrors
  `secret_hitler/testkit.rs` and `catan/testkit.rs`.
- [ ] Lint/Test/Build checks pass.

### US-CN03: Per-seat observation with role asymmetry
**Description:** As a spymaster I see the key card; as an operative I never
do — enforced structurally, not by prompt etiquette.

**Acceptance Criteria:**
- [ ] `observe(seat)` exposes to every seat: the 5×5 grid with each card's
  word and revealed state (and revealed identity), clue history with
  guesses-remaining, score, and whose turn it is. Spymaster seats
  additionally see the full key card. This is computed in `observe()` from
  state — the key card is **never an event**, so `Visibility` needs no
  team variant (SH `known_teammates` precedent; `game_core` unchanged).
- [ ] All game events are `Public` (Codenames has no private events);
  `thought` records are stored but never re-enter any observation.
- [ ] Tests assert: no operative observation contains key-card information
  beyond revealed cards; spymaster observations of the two teams are
  symmetric.
- [ ] Lint/Test/Build checks pass.

### US-CN04: Atomic decisions driven by the shared rethink loop
**Description:** As the harness, I drive every clue and every single guess as
one atomic decision so the engine stays the sole legality authority.

**Acceptance Criteria:**
- [ ] `DecisionPoint` variants: `GiveClue { seat }` and
  `GuessOrPass { seat, clue, guesses_used }` — **one decision per guess**,
  not a batched guess list, so the operative reacts to each reveal and the
  rethink loop covers every guess. Codenames is fully sequential:
  `pending_decisions()` always has exactly one entry, and the Catan-style
  single-decision game loop suffices (no simultaneous phases).
- [ ] Illegal clues (board-word overlap, bad token, number out of range) are
  `IllegalMove`s fed back verbatim through the rethink loop.
- [ ] Forced legal defaults, deterministic and seeded: clue → a
  seeded-random wordlist word legal on the current board, number 1;
  mandatory first guess → seeded-random unrevealed card; subsequent
  guesses → `pass`. All logged via `push_forced_default` and metric-exempt.
- [ ] Malformed / illegal / forced-default counted per seat exactly as in
  SH/Catan via the shared `Reliability` counters.
- [ ] Lint/Test/Build checks pass.

### US-CN05: Vendored open wordlist
**Description:** As a maintainer, I need a word pool with no IP exposure,
following the repo's Catan precedent of original assets only.

**Acceptance Criteria:**
- [ ] An **original, curated list of ~400 common English nouns** vendored in
  the engine crate (no copying of the proprietary Codenames card list —
  same legal posture as §6 of the Catan PRD). Words are single tokens,
  lowercase, deduplicated, and screened so no word is a substring of
  another (keeps clue legality crisp).
- [ ] Wordlist path overridable via config (`codenames.wordlist_path`) for
  custom pools; the effective wordlist hash is recorded in the
  `GameRecord` so records are comparable.
- [ ] Board draw + key layout derive from `cell_seed` so controlled
  schedules mirror boards across candidates.
- [ ] Lint/Test/Build checks pass.

### US-CN06: Baseline bots, including an embedding anchor
**Description:** As an eval author, I need a non-LLM anchor that gives
*meaningful* clues and guesses, because a random-only floor makes Codenames
ratings mushy — random clue-giving is not a baseline, it is noise.

**Acceptance Criteria:**
- [ ] `RandomLegalBot`: seeded-random legal clue with number 1; seeded-random
  unrevealed guess, then pass. A 4-random-bot game is a deterministic
  sub-second CI smoke test.
- [ ] `EmbeddingGreedyBot`, both roles, from a **small vendored word-vector
  subset** (pre-trained vectors filtered to the wordlist + a bounded clue
  candidate vocabulary; a few MB, checked in or fetched at build time —
  license-compatible, e.g. GloVe): spymaster greedily picks the clue
  candidate maximizing a margin between its similarity to a cluster of own
  agents and its maximum similarity to enemy/assassin/bystander words
  (assassin similarity hard-penalized); operative guesses unrevealed words
  by descending clue similarity and passes when the margin drops below a
  threshold. Deterministic given the seed. Documented as a floor, not a
  leaderboard entry.
- [ ] Bot kinds validated at the game boundary (`codenames-random`,
  `codenames-embedding`), per Catan PRD decision #10 — no bot taxonomy in
  `crates/config`.
- [ ] Anchors v1 = `RandomLegalBot` + `EmbeddingGreedyBot`.
- [ ] Lint/Test/Build checks pass.

### US-CN07: LLM seats
**Acceptance Criteria:**
- [ ] Observation rendered as compact structured text: the grid with
  revealed/unrevealed status (plus key identities for spymasters), clue
  history with outcomes, score, guesses remaining. Small enough that no
  delta rendering is needed (unlike Catan).
- [ ] Prompts follow the SH/Catan shape: rules summary + role brief + output
  contract + rendered observation + decision ask + per-decision JSON
  schema; unknown fields rejected; `thought` stripped before parsing.
- [ ] A Codenames `scaffold_version` hashes all prompt constants + golden
  renders of prompt-shaping functions, per the established pattern; a
  `PROMPT_REVISION` const is bumped on any wording change.
- [ ] Reuses `llm/` unchanged; per-seat model/persona/temperature via
  `AgentSpec` from config pools.
- [ ] Lint/Test/Build checks pass.

### US-CN08: Match runner with mirrored boards, role- and side-balanced
**Acceptance Criteria:**
- [ ] Controlled mode: 1 candidate + 3 frozen anchors (anchor teammate +
  anchor opposing team — the anchor teammate holds the partner confound
  constant so per-role skill is attributable). Cells =
  `board_bucket (B) × side (2: starting/second team) × role (2) × reps (K)`
  with seeds via `cell_seed(match_seed, "codenames-controlled", board,
  side_role, rep)`, candidate-independent: every candidate sees identical
  grids and key cards from every role on both sides.
- [ ] Arena mode: model sets of 4 rotated through seats evenly; realized
  distribution logged against planned.
- [ ] Runs resumable at game granularity (`{run_id}-{game_id}`), chunkable
  via `max_games`, exactly like `sh_run_match` / `catan_run_match`.
- [ ] Commands registered (full-name prefix, matching the `catan_*` style):
  `codenames_play_game`, `codenames_run_match`, `codenames_leaderboard`,
  `codenames_list_runs`, `codenames_list_games`, `codenames_game_replay`,
  `codenames_export_game_report` (cli-only) — appearing on CLI + HTTP
  automatically via the registry.
- [ ] Lint/Test/Build checks pass.

### US-CN09: Role-conditioned two-team rating
**Acceptance Criteria:**
- [ ] Ratings via Weng-Lin **two-team** form over win/loss, rating entities
  keyed `(model_id, role)` with role ∈ {spymaster, operative} — SH's
  role-conditioned template, not Catan's FFA placement (a 2-team win/loss
  is not a placement ranking).
- [ ] Headline = role-averaged conservative score μ−kσ (k=2 default);
  per-role lines always reported (the spymaster and operative numbers are
  the interesting split).
- [ ] Duplicate entities in one game average their deltas (SH rule); side
  (starting vs second team) is mirrored by the schedule rather than added
  to the rating key, and reported as a per-side win-rate diagnostic.
- [ ] Output includes game count and an uncertainty warning.
- [ ] Lint/Test/Build checks pass.

### US-CN10: Objective per-seat metric suite
**Acceptance Criteria:**
- [ ] **Outcome:** win, assassin-loss flag, turns to game end.
- [ ] **Spymaster:** mean clue number; agents found per clue; clue yield
  (own agents revealed ÷ intended number); assassin/enemy hits caused by
  own clues; illegal-clue attempts (from reliability, reported separately).
- [ ] **Operative:** guess precision (own-agent hit rate); first-guess
  precision; overreach rate (wrong guesses taken on the optional
  number+1th guess); pass discipline (passes with guesses remaining that
  the next reveal would have missed — computable from the key card
  post-hoc); assassin hits.
- [ ] Every metric engine-derived from the record + key card, no LLM judge;
  definitions + proxy caveats documented.
- [ ] Reliability counters (malformed / illegal / forced-default /
  transport) reported separately, forced actions excluded from
  play-quality metrics.
- [ ] Lint/Test/Build checks pass.

### US-CN11: Storage in parallel `codenames_*` tables
**Acceptance Criteria:**
- [ ] New sqlx migration `0003_codenames_init.sql`: `codenames_runs`,
  `codenames_games` (full `GameRecord` JSONB as replay source of truth,
  seed as hex text per the established u64 convention),
  `codenames_seats` (seat, role, side, model_id, agent_kind,
  scaffold_version, is_anchor, won, reliability counters, tokens, metric
  columns), `codenames_ratings` (`run_id, model_id, role` PK).
- [ ] `sh_*` and `catan_*` tables untouched. **The Catan PRD's "unify at
  game #3" flag is hereby acknowledged as due — and deliberately split
  out**: unification is a dedicated follow-up PR with its own migration
  plan, not coupled to feature delivery (§9 decision 9).
- [ ] The combined run listing for the frontend picker gains the
  `codenames` discriminator.
- [ ] Lint/Test/Build checks pass.

### US-CN12: Frontend replay + offline report
**Acceptance Criteria:**
- [ ] Game switcher gains a third tab; `client.ts` stays shared; a
  hand-written `api/codenames.ts` mirror follows `api/sh.ts` /
  `api/catan.ts`. The hardcoded two-branch `Game` union in `App.tsx`
  becomes a proper three-way switch (small refactor, in scope).
- [ ] Codenames components: a 5×5 word-grid replay with step-slider —
  unrevealed cards face-up as words, reveals flipping to team colors, the
  assassin visually unmistakable; a **spymaster key overlay toggle**
  (viewer chooses omniscient vs operative view); clue history sidebar with
  per-clue guess outcomes and 💭 thought disclosures.
- [ ] Own visual theme (word-card tabletop feel); no proprietary Codenames
  (CGE) artwork, logo, or trade dress — original execution, per the Catan
  §6 legal posture. A `codenames-brand` skill pins palette and card
  treatments once designed.
- [ ] Standalone offline HTML report (`codenames_export_game_report`) from a
  Codenames-specific template, driven like the existing game reports.
- [ ] Lint/Test/Build checks pass.

## 4. Functional Requirements

- **FR-CN1:** The engine implements the complete standard 2-team ruleset
  (US-CN01) as the single source of truth; clue legality is deterministic
  and engine-enforced — no LLM judge anywhere in the loop.
- **FR-CN2:** Observations never leak the key card to operatives (US-CN03),
  enforced structurally in `observe()`; `game_core` is not modified.
- **FR-CN3:** All agent decisions are atomic single actions (one clue, one
  guess) in strict per-decision JSON schemas, driven by the shared rethink
  loop with bounded retries and deterministic forced legal defaults
  (US-CN04).
- **FR-CN4:** Word grids and key cards derive from candidate-independent
  cell seeds; controlled schedules mirror boards across candidates, roles,
  and sides (US-CN08).
- **FR-CN5:** Ratings are Weng-Lin two-team keyed `(model_id, role)`
  (US-CN09); metrics per US-CN10.
- **FR-CN6:** Persistence in parallel `codenames_*` tables (US-CN11); every
  game yields a seed-reproducible, self-contained `GameRecord` JSONB
  carrying the wordlist hash and `rules_version`.
- **FR-CN7:** The vendored wordlist and any vendored word vectors are
  original or license-compatible assets; no proprietary Codenames content
  (US-CN05, US-CN06).
- **FR-CN8:** Per-game cost is bounded by structure (≤ 9 clues per team,
  guess cap number+1); config carries retry budget, token caps, and
  temperature per the established per-game config pattern.

## 5. Non-Goals (Out of Scope for Codenames v1)

- **Duet** (2-player co-op) and any player count other than 4 seats /
  1 operative per team.
- **Multi-operative teams and operative table talk.** The discussion
  machinery stays SH-specific; a deliberating operative team is a v2
  research question (and a cost multiplier).
- **Clue numbers 0 and "unlimited"** (official but niche); v1 numbers are
  1..=remaining agents with the number+1 guess cap.
- **Rules-as-written soft clue constraints** — rhymes, non-substring
  derivatives ("breaking" for BREAK is caught as a substring; "broke" is
  not), homonym/compound adjudication. The deterministic check is the
  documented v1 boundary; clues are stored verbatim so stricter offline
  audits remain possible. A stemmer would change legality and therefore
  `rules_version`; deferred deliberately.
- **LLM-judged clue quality** grading; v1 measures clues only via
  engine-derived outcomes.
- **Store/schema unification across the three games** — due per the Catan
  PRD's flag, but a dedicated follow-up PR, not part of this feature.
- **Human-vs-model play**, hosted leaderboard, picture variant.

## 6. Design Considerations

- **Why no `game_core` change:** the audit found two candidate pressure
  points and rejected both. (1) Shared team knowledge (the key card visible
  to both spymasters — though in v1 each sees it independently) is state
  derived in `observe()`, following SH's `known_teammates`; extending
  `Visibility` with a team variant would widen the shared core against its
  own extraction rule for zero current need. (2) The guess loop is a
  `DecisionPoint` variant (`GuessOrPass`), not a runner-side loop, so the
  rethink/forced-default machinery covers it — the lesson from Catan's
  act-until-pass `TurnAction`.
- **Clue legality is the load-bearing design point.** The whole eval
  machinery (rethink feedback, forced defaults, reliability counters,
  seeded reproducibility) hangs off `apply()` being a deterministic
  legality authority. An LLM-judged clue rule would quietly break all of it.
  The deterministic check is slightly *looser* than rules-as-written; that
  looseness is symmetric across candidates and therefore fair.
- **The embedding anchor is the biggest genuine cost** and it is worth it:
  Catan's anchor pattern (Random + Greedy) exists because random-only
  anchors made ratings mushy, and Codenames is the extreme case — a random
  clue-giver provides almost no signal for the opposing team comparison.
  Vector asset kept small by restricting to wordlist + a bounded clue
  vocabulary.
- **Config:** a new `crates/config/src/codenames.rs` (following `catan.rs`,
  not SH's older inline style): `retry_budget`, `agent_max_tokens`,
  `agent_temperature`, `rating_k`, `wordlist_path` (optional override),
  `clue_word_max_len`, `pools` (3 anchors), `model_sets` (4 seats).
- Prior art: the **Codenames AI Competition** framework and the word-vector
  Codenames-bot literature (clue generation via embedding margins) — for
  the anchor bot's design, not for harness patterns.
- **Legal note:** Codenames is a Czech Games Edition property. The vendored
  wordlist is an original curation of common English nouns; the frontend
  look is original card-table execution with the same information design.
  Same posture as Catan (§6 there), same reasoning.

## 7. Technical Considerations

- **The engine is small; the prompts are the risk center** — inverted from
  Catan. Rules fit in ~an afternoon of careful code; whether models give
  legal, well-formed clues under the output contract is where iteration
  will go. The rethink loop's feedback path (verbatim `IllegalMove` text)
  is the main tuning surface.
- **Token cost:** the cheapest game on the harness — ~20–40 decisions per
  game total across 4 seats, observations a few hundred tokens. Suitable as
  the fast-iteration game for harness changes.
- **Forced-default sharpness:** the mandatory first guess's forced default
  (seeded-random unrevealed card) can hit the assassin. That is acceptable
  and precedented (Catan's forced discard is also harmful), it is logged,
  metric-exempt, and rare by construction (only after retry-budget
  exhaustion) — and honoring the must-guess rule keeps the engine faithful.
- **Variance:** per-game variance is high (one assassin guess decides a
  game), but games are so cheap that controlled-mode K can be large.
  Mirrored boards + both sides + both roles per cell keep comparisons
  luck-controlled by construction.
- **Wordlist hygiene matters:** the no-substring-pairs screening (US-CN05)
  prevents degenerate boards where many potential clues are illegal by
  accident of the draw.

### Honest tradeoffs

- **The deterministic legality check under-enforces rules-as-written**
  (rhymes, non-substring derivatives pass). Symmetric across candidates,
  stored verbatim for offline audit, and revisitable behind a
  `rules_version` bump.
- **Anchor-partner ceiling:** in controlled mode the candidate spymaster's
  measurable skill is capped by the anchor operative's guessing (and vice
  versa). Mitigated by reporting per-role lines and using the embedding
  anchor rather than random; arena mode covers the LLM-teammate regime.
- **A same-model spymaster/operative pair may share associations**
  (self-play affinity). Controlled mode deliberately breaks this with anchor
  teammates; arena mode measures the mixed regime; interpret accordingly.
- **Two ratings per model** (spymaster, operative) is a more complex
  headline than one number; the role-averaged μ−kσ exists for ranking, but
  the per-role split is the honest story.

## 8. Success Metrics

- Codenames rules suite covers every US-CN02 cluster; 4-random-bot games
  are a deterministic sub-second CI smoke test; property tests hold over
  thousands of seeded games.
- A full 4-LLM game completes end-to-end — legal clues, guess streaks, a
  turn lost to a bystander, win or assassin ending — with a replayable
  transcript and offline HTML report.
- The `EmbeddingGreedyBot` pair reliably beats the `RandomLegalBot` pair
  (sanity floor ordering) over a seeded batch.
- A controlled match run produces role-conditioned ratings with σ, the
  metric suite, and reliability counters, resumable mid-run.
- `game_core`, `sh_*`, and `catan_*` code and tests are untouched by the
  entire feature.

## 9. Resolved Decisions

**User-confirmed (2026-08-11)** — all ten taken from the pros/cons review:

1. **Variant = standard 2-team, 4 fixed seats** (1 spymaster + 1 operative
   per team); no Duet, no multi-operative discussion teams.
2. **Decision granularity = atomic**: `give_clue`, then one
   `guess`-or-`pass` per decision (mandatory first guess); no batched guess
   lists.
3. **Clue legality = deterministic engine checks** (single token,
   case-insensitive, no equality/substring/superstring overlap with
   unrevealed board words); no LLM judge, no stemmer in v1 (would bump
   `rules_version`); gray areas logged verbatim for offline audit.
4. **Wordlist = vendored original ~400-word open curation**, seeded 25-draw
   + 9/8/7/1 key via `cell_seed`, mirrored across candidates; path
   override in config; wordlist hash recorded per game.
5. **Rating = Weng-Lin two-team keyed (model_id, role)**, role-averaged
   μ−2σ headline, per-role lines always reported — SH's template, not
   Catan's FFA.
6. **Controlled mode = candidate + frozen anchor teammate/opponents**,
   candidate rotated through both roles and both sides on mirrored boards;
   no self-play teams in controlled mode.
7. **Anchors v1 = `RandomLegalBot` + `EmbeddingGreedyBot`** (small vendored
   vector subset); bot kinds validated at the game boundary.
8. **Shared knowledge stays out of `game_core`**: key card derived in
   `observe()`; `Visibility::{Public, Private}` unchanged.
9. **Storage = parallel `codenames_*` tables** (`0003` migration); the
   Catan PRD's "unify at game #3" flag is due but **decoupled into its own
   follow-up PR** — never coupled to feature delivery.
10. **Frontend = per-game components + offline report**: word-grid replay
    with key overlay toggle, `api/codenames.ts`, third game tab (the
    `App.tsx` union refactor is in scope), own theme, original artwork
    only.

**Convention (locked here to avoid churn):**

11. Command prefix **`codenames_*`** (full-name style like `catan_*`);
    config in a new **`crates/config/src/codenames.rs`** following the
    `catan.rs` split-file pattern.

**Tune-later:**

12. Clue-word length cap, embedding-bot pass-margin threshold, and
    controlled-mode board buckets **B** / reps **K** set empirically once
    variance is measured.

## 10. Build Order

1. **Codenames rules engine, LLM-free** (`codenames/` types, wordlist +
   board, state, transitions, events, testkit, `RandomLegalBot`, property
   tests) — small, but everything downstream trusts it.
2. **LLM seats:** observation rendering, prompts + scaffold hash, parsing,
   `codenames_play_game`. (No `game_core` work — go straight here.)
3. **`EmbeddingGreedyBot`** + vendored vector subset (needed before ratings
   mean anything).
4. **Match layer:** schedules + mirrored seeds, role-conditioned rating,
   `codenames_*` migration, `codenames_run_match` + leaderboard commands.
5. **Frontend + offline report** (third tab, grid replay, key overlay,
   report template).
6. *(Separate PR, after v1 lands)*: the store-unification follow-up the
   Catan PRD deferred to game #3.
