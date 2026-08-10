# PRD: Catan-Evals

> An LLM-agent evaluation harness for **The Settlers of Catan** (base game,
> **4 players**), built as the **second game** on the Secret-Hitler-Evals
> substrate. This document specifies **Catan v1** and the accompanying
> **`game_core` extraction** — the multi-game abstraction that
> [`PRD-secret-hitler-evals.md`](PRD-secret-hitler-evals.md) §5 deliberately
> deferred "until a second game actually exists". That game now exists, so this
> PRD **supersedes that non-goal**.
>
> Three headline decisions were confirmed explicitly by the user; the rest are
> taken by recommendation. Both kinds are flagged in
> [Resolved Decisions](#9-resolved-decisions).

## 1. Introduction / Overview

Catan is a 4-player free-for-all of resource economics and negotiation: players
place settlements on a hex board, collect resources on dice rolls, and race to
10 victory points through building, development cards, and — critically —
**trading with each other**. It complements Secret Hitler in exactly the
dimensions SH cannot measure: long-horizon economic planning, spatial
reasoning, quantitative hidden information (opponents' hands and unplayed
development cards, including hidden victory points), and **free-form
negotiation with binding, engine-verified commitments**.

As with SH, the **game outcome is objective ground truth** computed by an
authoritative Rust engine. Because Catan is a free-for-all (not two teams), the
headline metric is a **free-for-all skill rating** (Weng-Lin / OpenSkill,
multi-team form) over each game's final placement ranking, conditioned on seat
position (turn order) the way SH ratings are conditioned on role. Alongside it
we store a suite of **objective, engine-derived per-player metrics** (no LLM
judge).

### Architecture

```
                     shbench: call / serve / mcp            (unchanged)
                                  │
                    CommandRegistry (inventory)             (unchanged)
                ┌─────────────────┼──────────────────┐
          sh_* commands     catan_* commands     generic cmds
                │                 │
    ┌───────────▼─────────────────▼───────────────┐
    │  game_core  (NEW — only provably shared)     │
    │   trait Game { State, Action, Observation,   │
    │                DecisionPoint, Event, … }     │
    │   generic: rethink loop + forced defaults,   │
    │   Visibility event log, SeatAgent<G>,        │
    │   GameRecord<G>, schedules, advance_match<G> │
    └───────┬───────────────────────┬──────────────┘
    secret_hitler/              catan/                (each owns its rules,
     rules, prompts,             rules, prompts,       prompts, metrics,
     metrics, 2-team rating      metrics, FFA rating   report template)
                │                 │
          llm/ + config + store ──┴──  (shared: client, providers,
                                        retry, keys, DB pool)
```

The turn structure the engine enforces (per active player):

```
  roll dice ──► 7? ──yes──► all hands >7 discard ──► move robber + steal
     │           no                                        │
     ▼                                                     ▼
  produce resources ──────────────────────────► ACTION LOOP (atomic):
                                                  build / buy dev / play dev
                                                  bank-or-port trade
                                                  propose player trade ◄──┐
                                                  │ addressees respond    │
                                                  │ (accept/reject/counter┘
                                                  │  + free-text message)
                                                  end turn (or forced cap)
```

## 2. Goals

- A **trustworthy 4-player base-game Catan engine** (full rules: dev cards,
  ports, robber, longest road / largest army, hidden VP) with an exhaustive
  test suite, since every metric derives from engine state.
- A **`game_core` abstraction extracted from — not invented over — the SH
  code**: only machinery that is provably identical between the two games is
  generalized; SH's existing tests gate the port.
- **Free-form negotiation with structured commitment**: agents talk in natural
  language during trade windows, but only typed, engine-validated trade actions
  move resources.
- A **single comparable headline number** per model from FFA placement
  rankings, seat-position-conditioned, with uncertainty reported.
- **Variance control by construction**: candidate-independent mirrored seeds
  for board layout *and* the full dice sequence, generalizing SH's
  controlled-schedule `cell_seed` design.
- Objective, engine-derived per-player metrics; reliability counters
  (malformed / illegal / forced-default) separate from play quality.
- Bounded cost: fixed 4 seats, per-turn action cap, per-utterance char cap,
  bounded negotiation exchanges.

## 3. User Stories

### US-C01: `game_core` extraction (SH behavior-preserving)
**Description:** As a maintainer, I want the game-agnostic agent-driving
machinery in one place so a bug fixed in the loop is fixed for every game.

**Acceptance Criteria:**
- [ ] A `game_core` module defines `trait Game` with associated types
  (`State`, `Action`, `Observation`, `DecisionPoint`, `Event`) and the minimal
  operations the loop needs (`pending_decisions`, `observe`, `apply`,
  `forced_default`, terminal check).
- [ ] Generic over `Game`: the rethink loop (malformed/illegal feedback →
  bounded retries → forced legal default, logged and metric-exempt), the
  `Visibility::{Public, Private(Seat)}` event log, `SeatAgent<G>`,
  `GameRecord<G>`, `AgentFactory`/`AgentSpec`, `GamePlan`/`MatchSpec`/
  `advance_match` resumability, and token accounting.
- [ ] Explicitly **not** abstracted: prompts, metrics, rating math, report
  templates, frontend components.
- [ ] Secret Hitler is ported onto `game_core` with **zero behavior change**:
  the entire existing SH test suite (`sh_rules_core`, `sh_rules_powers`,
  `sh_property`, `sh_match_e2e`) passes unmodified, and a fixed-seed SH game
  reproduces the same `GameRecord` as before the refactor.
- [ ] Lint/Test/Build checks pass.

### US-C02: Authoritative 4-player Catan engine
**Description:** As an eval author, I need an engine that holds full base-game
state and enforces every rule.

**Acceptance Criteria:**
- [ ] Standard base board: 19 hexes (fields/forest/pasture/hills/mountains/
  desert), number tokens 2–12 (no 7, desert none), 9 ports (4 generic 3:1,
  5 resource 2:1), seeded-random layout honoring the no-adjacent-6/8
  constraint.
- [ ] Setup snake draft: seats place settlement+road in order 1-2-3-4-4-3-2-1;
  second settlement grants its adjacent hexes' resources.
- [ ] Turn loop: roll → production (bank-limited; a resource short in the bank
  yields nobody that resource that roll if more than one player is owed it) →
  on 7: every hand >7 discards half (rounded down), active player moves the
  robber to a new hex and steals one random card from an adjacent victim →
  action loop (build road/settlement/city, buy/play dev card, bank/port
  trades, player trades) → end turn.
- [ ] Building legality: distance rule (no settlement adjacent to another),
  connectivity (roads/settlements attach to own network), piece limits
  (15 roads, 5 settlements, 4 cities), exact resource costs.
- [ ] Dev cards: 25-card deck (14 Knight, 5 VP, 2 each Road Building / Year of
  Plenty / Monopoly); at most one dev card played per turn; not on the turn
  bought (VP cards exempt from both); Knights move the robber and count toward
  Largest Army.
- [ ] Longest Road (≥5, longest continuous route, broken by opponent
  settlements) and Largest Army (≥3 knights) awarded and transferred per the
  official rules.
- [ ] Win: first to **10 VP on your own turn** (settlements 1, cities 2, VP
  cards 1, Longest Road 2, Largest Army 2); hidden VP cards count on the turn
  the holder reaches 10.
- [ ] All randomness (board, dice, dev deck, robber steal, forced-default
  tiebreaks) flows from a seedable RNG; identical seed + identical agent
  decisions → identical transcript.
- [ ] Lint/Test/Build checks pass.

### US-C03: Exhaustive engine rules test suite
**Acceptance Criteria:**
- [ ] Unit tests per rule cluster: distance rule, connectivity, piece limits,
  bank exhaustion, discard-on-7 (rounding, exactly-8 hands), robber must move,
  steal from empty hand, dev-card timing (bought-this-turn, one-per-turn, VP
  exemptions), Monopoly/Year-of-Plenty/Road-Building edge cases, Longest Road
  ties + severing by settlement, Largest Army transfer, ports (2:1 vs 3:1 vs
  4:1), win only on own turn, setup snake order + second-settlement resources.
- [ ] Property tests over randomized seeded games with `RandomLegalBot`:
  resource conservation vs bank, VP accounting matches recomputation from
  state, no action ever offered to a non-pending seat, every game terminates
  under the action cap.
- [ ] A scripted-game testkit (`new_scripted`-style: fixed board, fixed dice
  sequence, fixed dev deck) mirrors SH's `testkit.rs`.
- [ ] Lint/Test/Build checks pass.

### US-C04: Per-seat observation with quantitative hiding
**Description:** As an agent, I see my own hand exactly, others' hands only as
counts, and unplayed dev cards not at all.

**Acceptance Criteria:**
- [ ] `observe(seat)` exposes: full public board, own exact hand + own unplayed
  dev cards, per-opponent **resource-card count** and **unplayed-dev-card
  count** (never contents), public VP per player (hidden VP cards excluded),
  bank counts, and the seat's `Visibility`-filtered event history.
- [ ] Robber steals are `Private` to the two parties as card identity, `Public`
  as "a card was stolen"; Monopoly reveals are public per the rules; discards
  on 7 are public as counts, private as contents.
- [ ] Tests assert: no observation leaks another hand's contents, the dev deck
  order, another seat's unplayed dev cards, or hidden VP totals.
- [ ] Lint/Test/Build checks pass.

### US-C05: Atomic act-until-pass action interface
**Description:** As the harness, I drive each turn as a sequence of single
atomic actions so the engine stays the sole legality authority.
*(User-confirmed decision.)*

**Acceptance Criteria:**
- [ ] `pending_decisions()` yields exactly one head decision at a time
  (`RollOrPlayKnight`, `DiscardHalf` (simultaneous, all over-limit seats),
  `MoveRobber`, `ChooseSteal`, `TurnAction`, `RespondToTrade`,
  `SetupPlacement`, …); one LLM call per decision via the `game_core` rethink
  loop.
- [ ] `TurnAction` repeats until the agent plays `end_turn` or hits the
  configurable per-turn action cap (default **12**), which forces `end_turn`
  (logged as a forced default).
- [ ] Forced legal defaults per decision type, deterministic and seeded:
  discard → seeded-random from largest holdings; robber → seeded-random legal
  hex, victim → seeded-random adjacent; trade response → reject; turn action →
  end_turn; setup → seeded-random legal vertex + adjacent edge.
- [ ] Malformed / illegal / forced-default counted per seat exactly as in SH,
  excluded from play-quality metrics.
- [ ] Lint/Test/Build checks pass.

### US-C06: Free-form trade negotiation with structured commitment
**Description:** As an agent, I negotiate trades in natural language, but only
typed accept/commit actions move resources. *(User-confirmed decision.)*

**Acceptance Criteria:**
- [ ] `propose_trade { give, receive, to: seat | "all", message? }` opens a
  trade window; each addressed seat gets `RespondToTrade` with
  `{ accept | reject | counter{give, receive}, message? }`.
- [ ] Free text rides **only** in optional `message` fields (plus a standalone
  `say { message }` turn action for the active player) — talk never mutates
  resources; only an accepted offer, confirmed by the proposer when multiple
  seats accept or counter, executes, and the engine validates both sides can
  pay at execution time.
- [ ] Negotiation is bounded: per-utterance char cap (reuses the
  `utterance_char_cap` pattern, default 240), max exchanges per trade window
  (default 4 rounds of counters), max trade windows per turn.
- [ ] All negotiation text is public events (Catan table talk is public);
  private `thought` fields are recorded but never re-enter any observation.
- [ ] Trading is turn-owned and sequential (no simultaneous-reveal machinery):
  only the active player opens windows; addressees respond in seat order.
  Non-active players may not initiate.
- [ ] Lint/Test/Build checks pass.

### US-C07: Baseline bots
**Acceptance Criteria:**
- [ ] `RandomLegalBot` (uniform over legal actions, never proposes trades,
  rejects all offers) finishes thousands of seeded games headlessly; a
  4-random-bot game is a deterministic sub-second CI smoke test.
- [ ] One deliberately simple `GreedyBuilderBot`: setup on highest-pip legal
  vertices, builds by fixed priority (city > settlement > dev > road toward
  best expansion), accepts only strictly-favorable trades by pip-weighted
  value, robber on the current VP leader. Documented as a weak floor, not the
  leaderboard.
- [ ] Lint/Test/Build checks pass.

### US-C08: LLM seat with compact board serialization
**Acceptance Criteria:**
- [ ] Board rendered as **structured text** (not ASCII art): stable hex/vertex/
  edge IDs with explicit adjacency, each hex's resource + number + pip count,
  occupied vertices/edges by owner, ports, robber position; plus a per-decision
  **delta** (events since this seat's last decision) rather than the full
  history verbatim.
- [ ] Prompts follow SH's shape: rules summary + role brief + output contract
  + rendered observation + decision ask + per-decision JSON schema; unknown
  fields rejected; `thought` stripped before parsing.
- [ ] A Catan `scaffold_version` hashes all Catan prompt constants + a golden
  render of every prompt-shaping function and event-render arm, so any wording
  edit invalidates the scaffold id.
- [ ] Reuses `llm/` unchanged (providers, transport retry, token accounting);
  per-seat model/persona/temperature via `AgentSpec` from config pools.
- [ ] Lint/Test/Build checks pass.

### US-C09: Match runner with mirrored board + dice seeds
**Acceptance Criteria:**
- [ ] Controlled mode: 1 candidate + a frozen 3-anchor pool, cells =
  `board_bucket (B) × seat_position (4) × reps (K)`, seeds via a
  `cell_seed(match_seed, "catan-controlled", board, seat, rep)` that is
  **candidate-independent** — every candidate sees identical boards, identical
  dice sequences, identical dev decks, from every seat.
- [ ] Arena mode: mixed candidates rotated through seat positions evenly;
  realized distribution logged against planned.
- [ ] Runs resumable at game granularity (`{run_id}-{game_id}` idempotency),
  chunkable via `max_games`, exactly like `sh_run_match`.
- [ ] Commands registered: `catan_play_game`, `catan_run_match`,
  `catan_leaderboard`, `catan_list_runs`, `catan_list_games`,
  `catan_game_replay`, `catan_export_game_report` (cli-only) — appearing on
  CLI + HTTP automatically via the registry.
- [ ] Lint/Test/Build checks pass.

### US-C10: Free-for-all rating conditioned on seat position
**Acceptance Criteria:**
- [ ] Ratings via `skillratings` **`weng_lin_multi_team`** over each game's
  final placement ranking (VP at game end; ties broken official-style: tied
  players share rank).
- [ ] Rating entities are `(model_id, seat_position)` — turn order is Catan's
  structural asymmetry, as role is SH's. Headline = seat-averaged conservative
  score μ−kσ (k=2 default); per-seat lines always reported.
- [ ] Duplicate entities in one game average their deltas (SH rule).
- [ ] Output includes game count and an uncertainty warning; FFA convergence
  is slower than two-team — expect larger K than SH's.
- [ ] Lint/Test/Build checks pass.

### US-C11: Objective per-player metric suite
**Acceptance Criteria:**
- [ ] **Final VP** (incl. hidden) and placement.
- [ ] **Production efficiency:** resources actually gained vs expectation from
  owned pips × realized dice rolls (luck-controlled by construction).
- [ ] **Placement quality:** initial-settlement pip sum as a percentile of the
  legal vertices available at that pick.
- [ ] **Trade economics:** offers made / accepted / acceptance rate both ways;
  net pip-weighted value of executed trades.
- [ ] **Robber targeting:** fraction of the seat's robber placements that hit
  the current VP leader's production.
- [ ] **Reliability:** malformed / illegal / forced-default counters, forced
  actions excluded from all play-quality metrics.
- [ ] Every metric engine-derived, no LLM judge; definitions + proxy caveats
  documented.
- [ ] Lint/Test/Build checks pass.

### US-C12: Storage in parallel `catan_*` tables
**Acceptance Criteria:**
- [ ] New sqlx migration: `catan_runs`, `catan_games` (full `GameRecord` JSONB
  as replay source of truth), `catan_seats` (seat_position, model_id,
  agent_kind, scaffold_version, placement, VP, reliability counters, tokens,
  metric columns), `catan_ratings` (`run_id, model_id, seat_position` PK).
- [ ] `sh_*` tables untouched; no shared-schema refactor until a third game
  exists.
- [ ] A combined run listing (for the frontend picker) via a `list_runs`-style
  command that unions both games with a `game` discriminator field.
- [ ] Lint/Test/Build checks pass.

### US-C13: Frontend replay + report
**Description:** As an observer, I want the replay board to look and feel like
the physical tabletop game — nice enough to *watch*, not just inspect.
*(User-confirmed: high visual fidelity is a requirement, not a nice-to-have.)*

**Acceptance Criteria:**
- [ ] Game switcher at the tab level; `client.ts` stays shared; Catan gets its
  own components: SVG hex-board renderer with step-slider replay, per-turn
  collapsible transcript with 💭 thought disclosures, negotiation text inline,
  and a **trade-flow matrix** (who traded what with whom — the Catan analogue
  of the suspicion heatmap).
- [ ] **Tabletop-faithful board:** each resource hex has a distinct
  illustrated SVG treatment (fields/forest/pasture/hills/mountains/desert as
  recognizable terrain art, not flat color swatches); circular number tokens
  with probability pip dots and red 6/8; ports drawn on the coast with their
  trade ratios; an ocean border framing the island; the robber as a
  recognizable piece on its hex.
- [ ] **Recognizable game pieces in player colors:** roads as bars along
  edges, settlements as house silhouettes, cities as larger church-like
  silhouettes — readable at a glance without a legend.
- [ ] Replay stepping visually highlights the delta (the piece just placed,
  the robber's move, resources produced on a roll); dice shown as pip faces.
- [ ] All artwork is **original SVG evoking the tabletop game** — no CATAN
  trademark, logo, or copied assets (see §6 legal note).
- [ ] Standalone offline HTML report (`catan_export_game_report`) from a
  Catan-specific template, driven like `make game-report`.
- [ ] Catan gets its own visual theme; SH's propaganda-poster brand is not
  reused.
- [ ] Lint/Test/Build checks pass.

## 4. Functional Requirements

- **FR-C1:** The engine must implement the complete 4-player base-game ruleset
  (US-C02) as the single source of truth; no expansions.
- **FR-C2:** Observations must never leak hand contents, unplayed dev cards,
  hidden VP, or deck order (US-C04) — enforced structurally via the shared
  `Visibility` event log, not prompt etiquette.
- **FR-C3:** All agent decisions are atomic single actions in strict
  per-decision JSON schemas, driven by the shared `game_core` rethink loop
  with bounded retries and deterministic forced legal defaults (US-C05).
- **FR-C4:** Player trading uses free-form messages for negotiation and typed
  actions for commitment; talk never mutates state (US-C06).
- **FR-C5:** Board layout, dice sequence, and dev deck must be derivable from
  candidate-independent cell seeds so controlled schedules mirror luck across
  candidates (US-C09).
- **FR-C6:** Ratings are Weng-Lin multi-team FFA over placement, keyed
  `(model_id, seat_position)` (US-C10); metrics per US-C11.
- **FR-C7:** Persistence in parallel `catan_*` tables (US-C12); every game
  yields a seed-reproducible, self-contained `GameRecord` JSONB.
- **FR-C8:** The `game_core` extraction must be behavior-preserving for SH,
  gated by the unmodified SH test suite (US-C01).
- **FR-C9:** Per-game cost is bounded: action cap/turn, utterance char cap,
  negotiation exchange cap, trade windows/turn cap — all in config, all
  logged.

## 5. Non-Goals (Out of Scope for Catan v1)

- **Expansions** (Seafarers, Cities & Knights, 5–6 player extension) and
  player counts other than 4.
- **General table talk outside trade windows.** Free-form text exists only as
  trade-window messages and the active player's bounded `say`. Open diplomacy
  channels (alliances, threats between arbitrary seats at arbitrary times) are
  a v2 research question.
- **Belief elicitation checkpoints** (estimating opponents' hidden VP/hands).
  SH's suspicion-Brier has no equally crisp Catan analogue; deferred rather
  than shipped weak.
- **LLM-judge grading** of negotiation quality; v1 measures negotiation only
  via engine-derived trade economics and outcomes.
- **Binding multi-turn deals** ("I'll give you wheat next turn") — the engine
  executes only immediate exchanges; promises are cheap talk by design.
- **Human-vs-model play**, hosted leaderboard, spot-decision suites.
- **Unifying `sh_*` and `catan_*` schemas or rating tables** — revisit at
  game #3.

## 6. Design Considerations

- **Abstraction boundary rule ("provably game-agnostic"):** a piece of code
  moves into `game_core` only if the SH and Catan implementations would be
  textually identical up to the `Game` associated types. Everything else stays
  in the game module, even if it looks similar. First candidates (from the
  audit): rethink loop + forced defaults, `Visibility` event log,
  `SeatAgent`, `GameRecord` envelope, `AgentFactory`, schedules +
  `advance_match`, token/reliability accounting. Known non-candidates:
  prompts, metrics, rating math, discussion (SH's simultaneous-reveal is
  SH-specific; Catan negotiation is turn-owned and sequential).
- **SH's `AgentKind` leaves `crates/config`:** bot-kind strings are validated
  by each game module at the boundary (`sh-heuristic`, `catan-greedy`, …), so
  config stops accreting per-game taxonomies. Config gains a `catan:` section
  beside `secret_hitler:` (retry_budget, action_cap_per_turn,
  utterance_char_cap, trade_exchange_cap, trade_windows_per_turn, rating_k,
  pools of 3 anchors, model_sets of 4).
- **Negotiation grounding:** because commitments are typed and validated, a
  model that talks a great game but proposes unpayable trades is caught by the
  engine (illegal move → rethink → reliability counter), keeping the metric
  layer honest without judging the prose.
- Prior art: **Catanatron** (Catan RL engine — rules-coverage reference),
  **Diplomacy/Cicero** (talk-vs-commit separation), plus the SH PRD's prior
  art for the harness patterns.
- **Legal note on the board's look:** Secret Hitler's art is CC-licensed,
  which is why the SH brand skill reproduces it closely; **Catan is not** —
  its artwork, logo, and trade dress are proprietary (Catan GmbH / Catan
  Studio). The replay board therefore uses **original illustration in the
  spirit of a wooden-tabletop hex game**: same information design (terrain
  hexes, pip tokens, colored pieces), original execution. A
  `catan-board-brand` skill (analogous to `secret-hitler-brand`) should pin
  the palette, terrain treatments, and piece silhouettes once designed, so
  every surface (frontend, offline HTML report, marketing screenshots) stays
  consistent.

## 7. Technical Considerations

- **The rules engine is the risk center.** Catan's rules surface is roughly
  3–4× SH's, and board topology (vertex/edge graph, longest-road computation)
  is the genuinely new hard part. US-C02/C03 gate everything downstream.
- **Token cost:** expect **4–8× SH per game** — ~60–120 decision points ×
  4 seats, each re-ingesting board state. Levers: delta-based observation
  rendering (US-C08), compact board serialization, action cap, cheap-model
  pools during development. Budget before frontier arena runs.
- **Variance:** dice noise survives seed mirroring (play divergence
  desynchronizes the shared dice stream's *impact*, not its values), so
  controlled-mode K must be tuned empirically and will exceed SH's. The
  rating's σ is the honest signal; do not chase point estimates.
- **Longest road** is a longest-path problem on a small graph — fine
  exhaustively at Catan's size, but memoize per rebuild; property-test it
  against brute force.
- **Turn-owned sequential negotiation** is deliberately simpler than SH's
  orderless discussion: turn order advantage is real in Catan and already the
  rating's conditioning variable, so sequential response order (seat order)
  is acceptable and canonical.

### Honest tradeoffs

- **FFA ratings converge slower than two-team** and remain relative,
  pool-dependent, and tier-resolution only.
- **Placement ranking as outcome** rewards 2nd-place-securing play; win-only
  analysis stays available from stored records.
- **Free-form negotiation makes games non-reproducible across scaffold
  versions** (any prompt change alters talk, which alters everything). Seeds
  reproduce a game only under an identical scaffold — same property SH has,
  amplified.
- **Trade metrics are proxies:** pip-weighted trade value ignores positional
  context (a "bad" trade can be strategically right). They inform; placement
  adjudicates.

## 8. Success Metrics

- `game_core` lands with the SH suite green and a fixed-seed SH
  `GameRecord` byte-identical pre/post refactor.
- Catan rules suite covers every US-C03 cluster; 4-random-bot games are a
  deterministic sub-second CI smoke test; property tests hold over thousands
  of seeded games.
- A full 4-LLM game completes end-to-end — setup draft, production, a 7 with
  discard/robber/steal, at least one negotiated multi-message trade, dev-card
  plays, Longest Road/Largest Army transfer, terminal 10-VP win — with a
  replayable transcript and offline HTML report.
- A controlled match run produces seat-conditioned FFA ratings with σ, the
  metric suite, and honest reliability counters, resumable mid-run.
- Per-game token cost logged and within the configured budget.

## 9. Resolved Decisions

**User-confirmed (2026-08-10):**

1. **Abstraction = extract everything provably game-agnostic** into
   `game_core` (hybrid); no speculative framework, no pure copy-paste fork.
2. **Trading = free-form negotiation** (natural-language messages) **with
   structured, engine-validated commitment actions**.
3. **Action granularity = atomic act-until-pass** — one LLM call per decision,
   engine as sole legality authority, per-turn action cap with forced
   `end_turn`.
3a. **Replay board = high visual fidelity** (2026-08-10): tabletop-faithful
   illustrated board that is pleasant to watch, per US-C13 — with original
   artwork only (no CATAN trademark/assets; §6 legal note).

**By recommendation (flag if you disagree):**

4. **4 players, full base game** incl. dev cards, ports, robber, Longest
   Road / Largest Army, hidden VP; no expansions.
5. **Rating = Weng-Lin multi-team FFA over placement, keyed
   (model, seat_position)**, seat-averaged μ−2σ headline.
6. **Seeded-random boards** (not the fixed beginner board), mirrored with
   dice + dev deck across candidates via candidate-independent cell seeds;
   controlled mode = 1 candidate + 3 frozen anchors.
7. **Board serialization = structured text with stable IDs + per-decision
   deltas**; no ASCII-art maps in prompts.
8. **Storage = parallel `catan_*` tables**; `sh_*` untouched; unify only at
   game #3.
9. **Frontend = per-game components** sharing only the API client and shell;
   Catan gets an SVG hex board, trade-flow matrix, and its own theme.
10. **Bot kinds move out of `crates/config`'s `AgentKind`** into per-game
    validation; anchors v1 = `RandomLegalBot` + `GreedyBuilderBot`.
11. **Negotiation bounds:** 240-char messages, 4 counter-rounds per window,
    trade windows per turn capped (default 3), talk is public.
12. **Belief elicitation deferred** (no Catan suspicion-Brier analogue worth
    shipping weak in v1).

**Tune-later:**

13. Per-turn action cap starts at **12**; controlled-mode board buckets **B**
    and reps **K** set empirically once variance is measured.

## 10. Build Order

1. **Catan rules engine, LLM-free** (`catan/` types, board, state,
   transitions, events, testkit, `RandomLegalBot`, property tests) — the risk
   center, start here.
2. **`game_core` extraction** (independent of 1; SH tests are the safety net).
3. **LLM seats:** observation rendering, prompts + scaffold hash, parsing,
   `catan_play_game`.
4. **Match layer:** schedules + mirrored seeds, FFA rating, `catan_*`
   migration, `catan_run_match` + leaderboard commands.
5. **Frontend + offline report.**
