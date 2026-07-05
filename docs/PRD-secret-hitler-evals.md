# PRD: Secret-Hitler-Evals

> An LLM-agent evaluation harness for the hidden-role social-deduction game
> **Secret Hitler**. This document specifies **v1** at the **7-player** ruleset.
> Decisions taken by recommendation (vs. explicit user confirmation) are flagged
> in [Open Questions](#9-open-questions).

## 1. Introduction / Overview

Secret Hitler is a hidden-role game: a hidden **Liberal** majority tries to enact
Liberal policies and identify the **Fascists** (and their secret **Hitler**),
while the Fascist minority lies, manipulates, and maneuvers to enact Fascist
policies or install Hitler as Chancellor. It exercises exactly the skills current
agent evals struggle to measure: deception, deduction, persuasion,
theory-of-mind, and multi-round narrative coherence under information asymmetry.

This project evaluates LLMs playing Secret Hitler. The **game outcome (win/loss)
is objective ground truth**, computed by an authoritative Rust engine. Because a
win is a *team* result under high variance, the headline metric is a **team skill
rating** (OpenSkill / Weng-Lin) that attributes team outcomes to individual
models and absorbs variance across many games. Alongside the single rating we
store a suite of **objective, engine-derived per-player metrics** (no LLM judge)
that describe *how* a model plays.

**v1 scope is Secret Hitler only, at the 7-player ruleset.** No multi-game
abstraction is built until a second game actually exists.

### Why 7 players

7-player is **4 Liberals / 3 Fascists** (one Fascist is Hitler) and unlocks the
richer social dynamics:
- **Hitler plays blind:** at 7+ players Hitler does *not* know the Fascists (only
  the two regular Fascists know each other and know Hitler). Liberals know nobody.
- **Full power ladder** on the 7–8-player Fascist board: Investigate Loyalty →
  Special Election → Execution → Execution + Veto — so investigation is a
  first-class mechanic and metric.

### Architecture

```
  ┌──────────────────────────────────────────────────────────────────┐
  │  engine crate  (authoritative, transport-agnostic)               │
  │                                                                  │
  │   GameState ──► role-conditioned Observation (per seat)          │
  │   (board, roles,   hides other roles + others' hidden info;      │
  │    deck, tracker)  Hitler sees NO teammates at 7 players         │
  │        ▲                   │                                     │
  │        │ validated   ┌───────────────┐  strict JSON              │
  │        │ Action      │ Agent (seat)  │  {action,target,thought}  │
  │        └── parse ◄────│  LLM or bot   │                          │
  │           +legal-check└───────────────┘                          │
  │           +rethink / forced-legal-default (logged, metric-exempt)│
  │                                                                  │
  │   Emits: win/loss  +  per-player objective signals               │
  └──────────────────────────────────────────────────────────────────┘
        │                              │
        ▼ Commands over…              ▼ rating layer (Weng-Lin / OpenSkill)
   shbench CLI (batch runs)      per-model rating + per-faction win rates
   shbench serve (HTTP API)      + granular metrics store
        │
        ▼
   React frontend (optional): game replay / who-suspected-who viz
```

## 2. Goals

- Produce a **single comparable headline number** per model (Weng-Lin/OpenSkill
  rating) from Secret Hitler games, correctly attributing team wins to
  individuals and absorbing variance + faction asymmetry.
- Always report **per-faction win rates** (Liberal, Fascist) separately, never
  hidden inside the rating.
- Store a rich set of **objective, engine-derived per-player metrics** (no
  subjective grading) that characterise play quality.
- Support two opponent modes: **deterministic rule-based baselines** (cheap,
  reproducible, CI floor) and **full LLM-vs-LLM games with orderless discussion**
  (the real eval).
- Guarantee a **trustworthy engine**: an exhaustive rules test suite is a
  first-class deliverable, because every metric derives from engine state.
- Keep cost bounded: fixed 7 seats, bounded discussion, balanced role rotation.

## 3. User Stories

### US-001: Authoritative 7-player game engine
**Description:** As an eval author, I need an engine that holds the full Secret
Hitler 7-player game state and enforces every rule, so that outcomes and metrics
are trustworthy.

**Acceptance Criteria:**
- [ ] Models 7-player setup: 4 Liberals, 3 Fascists, one Fascist is Hitler.
- [ ] Knowledge rules: the two regular Fascists know each other and know Hitler; **Hitler knows no teammates**; Liberals know nothing.
- [ ] Policy deck: 6 Liberal + 11 Fascist tiles, shuffled from a seedable RNG; reshuffle discard back in when < 3 tiles remain.
- [ ] Government round: President nominates Chancellor → all living players vote Ja/Nein → on pass, President draws 3, discards 1, Chancellor enacts 1 of 2.
- [ ] Eligibility: the last elected President **and** Chancellor are ineligible as Chancellor candidates (only the last Chancellor once ≤5 players remain); dead players excluded.
- [ ] Election tracker: 3 consecutive failed governments → top policy auto-enacted (**grants no power**), tracker resets, term-limit memory clears.
- [ ] 7–8-player Fascist board powers fire in order: 2nd Fascist policy → Investigate Loyalty; 3rd → Call Special Election; 4th → Execution; 5th → Execution and Veto unlocked.
- [ ] Investigate Loyalty reveals only a target's **party membership** (Liberal/Fascist; Hitler's card reads Fascist), never the secret role; result goes privately to the investigating President.
- [ ] Special Election lets the President appoint the next Presidential candidate; normal seat order resumes afterward.
- [ ] Win conditions: Liberals win on 5 Liberal policies **or** Hitler executed; Fascists win on 6 Fascist policies **or** Hitler elected Chancellor after ≥3 Fascist policies.
- [ ] Lint/Test/Build checks pass.

### US-002: Exhaustive engine rules test suite
**Description:** As an eval author, I need the engine's rules covered by tests so
a silent rules bug cannot corrupt every downstream metric.

**Acceptance Criteria:**
- [ ] Deterministic games are fully reproducible from a seed (same seed + same agent decisions → identical transcript).
- [ ] Tests cover each edge case: term limits (incl. the ≤5-players relaxation), election-tracker top-deck (reset + no power granted), Investigate Loyalty, Special Election (and return to normal order), Policy Peek is absent on this board, Execution (incl. executing Hitler → immediate Liberal win), Veto unlock + veto flow, deck reshuffle, both Liberal and both Fascist win conditions, and the "Hitler elected Chancellor after 3 Fascist policies" win.
- [ ] A test asserts the 7-player knowledge graph exactly: Fascists↔Fascists and Fascists→Hitler know; **Hitler→teammates does not**.
- [ ] A property test asserts invariants across randomized games (policy-tile conservation, exactly one policy enacted per government, no action offered to a dead player).
- [ ] Lint/Test/Build checks pass.

### US-003: Role-conditioned observation
**Description:** As an agent, I receive only the information my seat is entitled
to, so hidden-role asymmetry is enforced by the engine, not by prompt etiquette.

**Acceptance Criteria:**
- [ ] The engine produces a per-seat `Observation` with public state + only that seat's private knowledge.
- [ ] Tests assert: a Liberal observation contains no other player's role; a **Hitler** observation contains no teammate identities (7-player rule); a regular Fascist observation contains its teammates and Hitler; no observation leaks deck order or others' drawn tiles or another President's private investigation result.
- [ ] Observation includes the full public action/discussion history the seat legitimately witnessed.
- [ ] Lint/Test/Build checks pass.

### US-004: Strict JSON action interface with parse → legal-check → rethink
**Description:** As the harness, I take a model's action as strict JSON, validate
it, and re-prompt on illegal moves, so malformed or illegal output degrades
gracefully and is measured, not fatal.

**Acceptance Criteria:**
- [ ] Each decision point defines a strict JSON schema (`{action, target?, policy?, thought_process}` as applicable) with `additionalProperties:false`.
- [ ] Parse failure (invalid JSON / schema violation) triggers a bounded retry (configurable, default N attempts).
- [ ] A legally-parsed but **illegal** move (e.g. nominating a term-limited player) triggers a rethink re-prompt including the reason, within the same attempt budget.
- [ ] On exhausting attempts, the engine applies the defined **forced legal default** (see FR-4) and records the event.
- [ ] Metrics record **malformed-output count** and **illegal-move count** as *separate* reliability counters, distinct from play-quality metrics; forced-default moves are **excluded** from play-quality metrics.
- [ ] Lint/Test/Build checks pass.

### US-005: Deterministic rule-based baseline opponents
**Description:** As an eval author, I want scripted non-LLM agents so I have a
cheap, reproducible floor and a CI smoke test independent of any LLM.

**Acceptance Criteria:**
- [ ] At least one rule-based agent per role that plays a legal, coherent but **deliberately simple** mechanical strategy (nominate/vote/enact/use-power) with **no** discussion.
- [ ] A full 7-player all-baseline game runs headlessly and deterministically from a seed in CI in < a few seconds, asserting a terminal state is reached.
- [ ] Documentation states baselines are a weak floor/smoke test, are exploitable, exercise **no** persuasion, and are not the leaderboard.
- [ ] Lint/Test/Build checks pass.

### US-006: LLM agent + provider-agnostic model calling
**Description:** As an eval author, I want to seat LLM models via the existing
config so I can run real games.

**Acceptance Criteria:**
- [ ] An LLM agent renders the seat `Observation` into a prompt and returns a schema-valid action, reusing the project's LLM config (`app_config`, `default_llm`).
- [ ] Model calls are retry-wrapped for transient API failures (separate from the illegal-move rethink loop).
- [ ] A game can seat any mix of models and/or baselines by config.
- [ ] Lint/Test/Build checks pass.

### US-007: Orderless free-form discussion phase (simultaneous reveal)
**Description:** As an agent, I can talk before votes so the game exercises
persuasion and deduction — with a bounded, reproducible, **order-free** structure
that no seat position can exploit.

**Acceptance Criteria:**
- [ ] Before each government's vote, discussion runs for a configurable number of **simultaneous rounds** (default 2).
- [ ] Within a round, every living player produces exactly one utterance (or explicit pass) **in parallel**, each conditioned only on public state through the **end of the previous round** — no player sees another's same-round message before writing.
- [ ] All of a round's utterances are revealed together and appended to public history; the next round conditions on them.
- [ ] No fixed or implied speaking order exists within a round (verified: swapping seat indices does not change any agent's within-round input).
- [ ] Per-utterance length/token cap enforced; total discussion cost per game bounded and logged.
- [ ] Dead players do not speak.
- [ ] Lint/Test/Build checks pass.

### US-008: Match runner with balanced role/seat rotation
**Description:** As an eval author, I want many games with every model rotated
fairly through seats and factions, so no model is advantaged by assignment.

**Acceptance Criteria:**
- [ ] A `run-match` / batch command plays a configurable number of games over a set of seated models.
- [ ] Assignment rotates every model through every seat and both factions as evenly as the schedule allows; the realized distribution is logged.
- [ ] Every game persists a replayable transcript (seed, assignments, actions, discussion, powers used, outcome, per-player signals).
- [ ] Runs are resumable at match granularity (init → continue → cleanup).
- [ ] Lint/Test/Build checks pass.

### US-009: Weng-Lin/OpenSkill headline rating + per-faction win rates
**Description:** As a user, I want one comparable rating per model plus faction
win rates, so I can rank models honestly.

**Acceptance Criteria:**
- [ ] After a batch, each model has a Weng-Lin/OpenSkill rating (mean μ and uncertainty σ) computed from game win/loss, treating the two factions as the two teams.
- [ ] Output reports rating **with its uncertainty**, not a bare point estimate.
- [ ] **Liberal win rate** and **Fascist win rate** are reported separately per model.
- [ ] Rating output states game count and warns when uncertainty is too high to separate close models.
- [ ] Lint/Test/Build checks pass.

### US-010: Granular objective per-player metric suite
**Description:** As a researcher, I want objective, engine-derived signals per
model so I understand *how* it plays, without any subjective judge.

**Acceptance Criteria:**
- [ ] **Suspicion accuracy:** at defined checkpoints (see FR-13) each living player emits a structured belief distribution over others' roles; scored (Brier / log-loss) against ground-truth roles. Liberal suspicion accuracy reported.
- [ ] **Goal-aligned enactment rate** (per-decision, luck-controlled): of the policy choices a player personally made (President discard among 3, Chancellor enact among 2), the fraction that advanced the player's own faction, given the tiles held.
- [ ] **Faction policy throughput** (outcome stat): count/rate of the player's-faction policies enacted in governments where the player sat as President or Chancellor. Reported as outcome-correlated (not pure decision quality).
- [ ] **Investigation quality** (Liberal Presidents): when Investigate Loyalty is used, whether the target's true party was previously uncertain to that seat, and whether the President's subsequent public votes/nominations were consistent with the true result.
- [ ] **Execution accuracy:** when a player used Execution, whether the shot hit a Fascist/Hitler vs a Liberal, per faction.
- [ ] Every metric is computed purely from engine state + declared beliefs; none uses an LLM to grade. Each documents its exact definition and known proxy limitations. Forced-default actions are excluded.
- [ ] Metrics persisted per game and aggregated per model.
- [ ] Lint/Test/Build checks pass.

### US-011: Results surfaced over CLI + HTTP (+ optional viz)
**Description:** As a user, I want to run evals headlessly and inspect results,
optionally visually.

**Acceptance Criteria:**
- [ ] `shbench` CLI runs a batch and writes rating + faction win rates + metric summary (JSON).
- [ ] HTTP API exposes stored results (leaderboard + per-game transcript) under `/api/v1`.
- [ ] The optional React frontend can render a game replay and a per-model summary (viz is nice-to-have, not gating).
- [ ] Lint/Test/Build checks pass.

## 4. Functional Requirements

- **FR-1:** The engine must implement the complete 7-player Secret Hitler ruleset (4 Lib / 3 Fasc incl. Hitler; Hitler-blind knowledge; deck; government; term limits; election tracker; Investigate Loyalty; Special Election; Execution; Veto; all win conditions) as the single source of truth for state.
- **FR-2:** The engine must expose per-seat role-conditioned observations that never leak information a seat is not entitled to (including Hitler seeing no teammates and Presidents' private investigation results).
- **FR-3:** Agent decisions must be strict JSON validated against a per-decision schema; malformed JSON triggers bounded retries.
- **FR-4:** Legal-but-illegal moves trigger a bounded rethink with the reason; on exhaustion the engine applies a **deterministic forced legal default** per decision type — **vote → Nein; nomination → first eligible by seat; policy choice → seeded-random tile; power target → first legal target** — logs it, and **excludes it from play-quality metrics** (only reliability counters increment).
- **FR-5:** The harness must record malformed-output and illegal-move counts separately from play-quality metrics.
- **FR-6:** The harness must support deterministic, deliberately-simple rule-based baseline agents (no discussion) usable as a CI floor.
- **FR-7:** The harness must support LLM agents via the existing `app_config` LLM configuration, with transient-failure retry.
- **FR-8:** Discussion must run as **simultaneous-reveal rounds** (configurable count, default 2): within a round all living players' utterances are generated in parallel conditioned only on state through the previous round, then revealed together. No within-round ordering. Per-utterance cap enforced; costs logged.
- **FR-9:** The match runner must rotate every model through every seat and both factions as evenly as possible and log the realized distribution.
- **FR-10:** Every game must produce a seed-reproducible, replayable transcript.
- **FR-11:** The system must compute a Weng-Lin/OpenSkill rating (μ, σ) per model from win/loss, treating factions as teams. Implementation targets the Rust `skillratings` crate's `weng_lin` model; if its per-player contribution-weight support is insufficient, use/port the `openskill` crate.
- **FR-12:** The system must report Liberal and Fascist win rates per model separately, plus game count and an uncertainty warning.
- **FR-13:** The system must compute and persist the objective per-player metric suite (suspicion accuracy, goal-aligned enactment rate, faction policy throughput, investigation quality, execution accuracy) with documented definitions. Suspicion beliefs are elicited **after each enacted policy plus one end-of-game snapshot** (configurable).
- **FR-14:** Results must be retrievable via CLI and HTTP API; the frontend replay is optional.
- **FR-15:** All randomness (deck, assignment, forced-default tiebreaks) must be seedable so deterministic runs reproduce exactly.

## 5. Non-Goals (Out of Scope for v1)

- **Subjective LLM-judge grading** of deception, bluff quality, or persuasion — deferred to v2. v1 measures these only *implicitly* via wins and objective proxies.
- **Any multi-game abstraction** (no generic `Game` trait). Secret Hitler only. Avalon/Hanabi/Diplomacy remain *design references*, not build targets.
- **Player counts other than 7** (5–6 and 9–10 rulesets, and the 9–10 board's second Investigate slot) — deferred.
- **Human-vs-model play**, matchmaking, or a hosted public leaderboard service.
- **Static single-decision "spot" suites** (PokerBench Mode A style) — live play only in v1.
- **Duplicate-deck / mirror-match variance reduction** — not portable to a 7-player team game; variance is handled by rating uncertainty + balanced rotation + game volume.

## 6. Design Considerations

- Reuse the engine `Command` registry + `Ctx` pattern: `init_match` → `advance` → `finalize` follows the repo's init→continue→cleanup convention; matches carry a descriptive `runId`.
- The React frontend is an **optional visualization layer** (replay, who-suspected-who), not core infrastructure.
- Prior art to mirror: **AvalonBench** (near-isomorph; free-form discussion; rule-based baselines), **PokerBench** (strict JSON action schema; forced-default on invalid action), **Kaggle Game Arena** (external state authority; two-stage parse; rethink-on-illegal), **TrueSkill2 / OpenSkill** (team credit assignment).

## 7. Technical Considerations

- **Backend:** Rust — `engine` crate (game + eval logic, no transport deps), `shbench` binary (CLI + HTTP API). Rating via Weng-Lin/OpenSkill (`skillratings` `weng_lin`, or the `openskill` crate).
- **Engine correctness is load-bearing:** every metric derives from engine state, so US-002's test suite gates everything else. Prioritize it.
- **Cost drivers:** discussion tokens dominate (every seat re-ingests growing history). Mitigations: fixed 7 seats, bounded simultaneous rounds (default 2), per-utterance cap, parallel generation within a round (cheaper wall-clock), and zero-token baseline games for CI.
- **Belief elicitation:** suspicion accuracy needs a structured side-channel prompt at checkpoints; it must not alter game state and its cost is counted.

### Honest tradeoffs (stated, not hidden)

- **The rating is coarse, relative, and non-stationary.** Weng-Lin/OpenSkill (like TrueSkill) is accurate head-to-head but degrades in multiplayer/team/FFA; expect wide σ, slow convergence, and reliable separation only between *tiers*, not adjacent ranks. Ratings shift with the model pool and can mask non-transitive (rock-paper-scissors) metagames.
- **Win/loss is low-bandwidth** (≈1 bit/game): many games are needed and it never explains *why*. The objective metric suite is the partial remedy.
- **Per-player signals are proxies.** Goal-aligned enactment is luck-controlled (conditioned on tiles held) and is the strongest; faction throughput and investigation quality are more outcome-correlated and can reward luck — they inform, they don't adjudicate.
- **Rule-based baselines don't exercise persuasion** and are exploitable; a floor/CI anchor, not the leaderboard.

## 8. Success Metrics

- Engine test suite covers every US-002 edge case and runs green in CI; deterministic games reproduce bit-for-bit from a seed.
- A full LLM-vs-LLM 7-player game completes end-to-end (simultaneous discussion → votes → policies → powers incl. investigation/special-election/execution → terminal win) with a replayable transcript.
- A batch run produces per-model Weng-Lin/OpenSkill ratings (with σ), per-faction win rates, and the full objective metric suite.
- Baseline-only 7-player games run in CI as a sub-second deterministic smoke test.
- Malformed-output, illegal-move, and forced-default rates are reported per model and separable from play quality.
- Total token cost per LLM game is bounded and logged, within the configured discussion budget.

## 9. Open Questions

**Settled** (from discussion — recorded for traceability):

1. **Player count = 7** (4 Liberals / 3 Fascists, Hitler blind; Investigate Loyalty + Special Election + Execution×2 + Veto board).
2. **Rating = Weng-Lin / OpenSkill in Rust** (`skillratings` `weng_lin`, fallback `openskill` crate for contribution weights).
3. **Discussion = simultaneous-reveal rounds** (orderless; default 2 rounds), chosen over round-robin to remove exploitable seat order.
5. **Vote-alignment heuristic dropped**, replaced by goal-aligned enactment rate (per-decision) + faction policy throughput (outcome stat).

**Recommended defaults** (taken because the question was left open — confirm or flip):

4. **Belief-elicitation cadence = after each enacted policy + end-of-game snapshot.** Alternatives: per-government (costlier, finer) or end-only (cheaper, coarser).
6. **Forced legal default = deterministic per decision** (Nein / first-eligible / seeded-random tile / first-legal-target), metric-exempt. Alternatives: seeded-random for all, or forfeit-the-game on repeated illegality.
7. **Baseline strength = deliberately simple/weak.** Alternative: a tuned mechanical opponent (more discriminating floor, higher build/maintenance cost, overfitting risk).

**Still genuinely open:**

8. **Investigation-quality definition** needs pinning — "acted consistently with the true result" is the softest new metric; exact rule TBD.
9. **Faction policy throughput** attribution — count only governments where the player made the enacting choice, or any government they sat in?
10. **Discussion round count** default (2) — validate against cost/signal once real games run.
