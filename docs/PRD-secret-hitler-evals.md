# PRD: Secret-Hitler-Evals

> An LLM-agent evaluation harness for the hidden-role social-deduction game
> **Secret Hitler**. This document specifies **v1**. Design decisions were
> settled in prior discussion; where a decision was made by recommendation
> (not explicit user confirmation) it is flagged in [Open Questions](#9-open-questions).

## 1. Introduction / Overview

Secret Hitler is a 5–10 player hidden-role game: a hidden **Liberal** majority
tries to enact Liberal policies and identify the **Fascists** (and their secret
**Hitler**), while the Fascist minority lies, manipulates, and maneuvers to
enact Fascist policies or install Hitler as Chancellor. It exercises exactly the
skills current agent evals struggle to measure: deception, deduction, persuasion,
theory-of-mind, and multi-round narrative coherence under information asymmetry.

This project evaluates LLMs playing Secret Hitler. The **game outcome (win/loss)
is objective ground truth**, computed by an authoritative Rust engine. Because a
win is a *team* result under high variance, the headline metric is a **team skill
rating** (TrueSkill) that attributes team outcomes to individual models and
absorbs variance across many games. Alongside the single rating we store a suite
of **objective, engine-derived per-player metrics** (no LLM judge) that describe
*how* a model plays.

**v1 scope is Secret Hitler only** at the **5-player** ruleset. No multi-game
abstraction is built until a second game actually exists.

### Architecture

```
  ┌──────────────────────────────────────────────────────────────────┐
  │  engine crate  (authoritative, transport-agnostic)               │
  │                                                                  │
  │   GameState ──► role-conditioned Observation (per seat)          │
  │   (board, roles,   hides other roles + others' hidden info       │
  │    deck, tracker)          │                                     │
  │        ▲                   ▼                                     │
  │        │ validated   ┌───────────────┐  strict JSON              │
  │        │ Action      │ Agent (seat)  │  {action,target,thought}  │
  │        └── parse ◄────│  LLM or bot   │                          │
  │           +legal-check└───────────────┘                          │
  │           +rethink / forced-legal-default                        │
  │                                                                  │
  │   Emits: win/loss  +  per-player objective signals               │
  └──────────────────────────────────────────────────────────────────┘
        │                              │
        ▼ Commands over…              ▼ rating layer (skillratings::TrueSkill)
   shbench CLI (batch runs)      per-model rating + per-faction win rates
   shbench serve (HTTP API)      + granular metrics store
        │
        ▼
   React frontend (optional): game replay / who-suspected-who viz
```

## 2. Goals

- Produce a **single comparable headline number** per model (TrueSkill rating)
  from Secret Hitler games, correctly attributing team wins to individuals and
  absorbing variance + faction asymmetry.
- Always report **per-faction win rates** (Liberal, Fascist) separately, never
  hidden inside the rating.
- Store a rich set of **objective, engine-derived per-player metrics** (no
  subjective grading) that characterise play quality.
- Support two opponent modes: **deterministic rule-based baselines** (cheap,
  reproducible, CI floor) and **full LLM-vs-LLM games with free-form discussion**
  (the real eval).
- Guarantee a **trustworthy engine**: an exhaustive rules test suite is a
  first-class deliverable, because every metric derives from engine state.
- Keep cost bounded: fixed 5 seats, bounded discussion, balanced role rotation.

## 3. User Stories

### US-001: Authoritative 5-player game engine
**Description:** As an eval author, I need an engine that holds the full Secret
Hitler 5-player game state and enforces every rule, so that outcomes and metrics
are trustworthy.

**Acceptance Criteria:**
- [ ] Models 5-player setup: 3 Liberals, 2 Fascists, one Fascist is Hitler; Fascists (incl. Hitler) know each other's identities; Liberals know nothing.
- [ ] Policy deck: 6 Liberal + 11 Fascist tiles, shuffled from a seedable RNG; reshuffle discard when < 3 remain.
- [ ] Government round: President nominates Chancellor → all living players vote Ja/Nein → on pass, President draws 3, discards 1, Chancellor enacts 1 of 2.
- [ ] Enforces eligibility: term-limited last-elected President/Chancellor cannot be nominated; dead players excluded.
- [ ] Election tracker: 3 consecutive failed governments → top policy auto-enacted, tracker resets, powers/eligibility rules applied.
- [ ] 5–6 player Fascist board powers fire correctly: 3rd Fascist policy → Policy Peek; 4th & 5th → Execution; Veto unlocked after 5 Fascist policies.
- [ ] Win conditions: Liberals win on 5 Liberal policies **or** Hitler executed; Fascists win on 6 Fascist policies **or** Hitler elected Chancellor after ≥3 Fascist policies.
- [ ] Lint/Test/Build checks pass.

### US-002: Exhaustive engine rules test suite
**Description:** As an eval author, I need the engine's rules covered by tests so
a silent rules bug cannot corrupt every downstream metric.

**Acceptance Criteria:**
- [ ] Deterministic games are fully reproducible from a seed (same seed + same agent decisions → identical transcript).
- [ ] Tests cover each edge case: term limits, election-tracker top-deck (incl. tracker reset and that top-decked policies grant **no** power), Policy Peek, Execution (including executing Hitler → immediate Liberal win), veto unlock + veto flow, deck reshuffle, both Liberal and both Fascist win conditions, and the "Hitler elected Chancellor after 3 Fascist policies" Fascist win.
- [ ] A property test asserts invariants hold across randomized games (policy-tile conservation, exactly-one-policy-enacted-per-government, no action available to a dead player).
- [ ] Lint/Test/Build checks pass.

### US-003: Role-conditioned observation
**Description:** As an agent, I receive only the information my seat is entitled
to, so hidden-role asymmetry is enforced by the engine, not by prompt etiquette.

**Acceptance Criteria:**
- [ ] The engine produces a per-seat `Observation` containing public state + only that seat's private knowledge (own role; if Fascist/Hitler, the Fascist team identities).
- [ ] A test asserts a Liberal observation never contains any other player's role, and that no observation leaks the policy deck order or others' drawn tiles.
- [ ] Observation includes the full public action/discussion history the seat would legitimately have witnessed.
- [ ] Lint/Test/Build checks pass.

### US-004: Strict JSON action interface with parse → legal-check → rethink
**Description:** As the harness, I take a model's action as strict JSON, validate
it, and re-prompt on illegal moves, so malformed or illegal output degrades
gracefully and is measured, not fatal.

**Acceptance Criteria:**
- [ ] Each decision point defines a strict JSON schema (`{action, target?, policy?, thought_process}` as applicable) with `additionalProperties:false`.
- [ ] Parse failure (invalid JSON / schema violation) triggers a bounded retry (configurable, default up to N attempts).
- [ ] A legally-parsed but **illegal** move (e.g. nominating a term-limited player) triggers a rethink re-prompt that includes the reason, bounded by the same attempt budget.
- [ ] On exhausting attempts, the engine applies a defined **forced legal default** (e.g. forced vote / forced minimal-impact action) and records the event.
- [ ] Metrics record **malformed-output count** and **illegal-move count** as *separate* counters, distinct from legal-but-suboptimal play.
- [ ] Lint/Test/Build checks pass.

### US-005: Deterministic rule-based baseline opponents
**Description:** As an eval author, I want scripted non-LLM agents so I have a
cheap, reproducible floor and a CI smoke test independent of any LLM.

**Acceptance Criteria:**
- [ ] At least one rule-based agent per role that plays a legal, coherent mechanical strategy (nominate/vote/enact/use-power) with **no** free-form discussion.
- [ ] A full 5-player all-baseline game runs headlessly and deterministically from a seed in CI in < a few seconds, asserting the engine reaches a terminal state.
- [ ] Documentation explicitly states baselines are a floor/smoke test, are exploitable, and do **not** exercise persuasion — they are not the leaderboard.
- [ ] Lint/Test/Build checks pass.

### US-006: LLM agent + provider-agnostic model calling
**Description:** As an eval author, I want to seat LLM models via the existing
config so I can run real games.

**Acceptance Criteria:**
- [ ] An LLM agent renders the seat `Observation` into a prompt and returns a schema-valid action, reusing the project's existing LLM config (`app_config`, `default_llm`).
- [ ] Model calls are retry-wrapped for transient API failures (separate from the illegal-move rethink loop).
- [ ] A game can seat a mix of models and/or baselines by config.
- [ ] Lint/Test/Build checks pass.

### US-007: Free-form discussion phase (fixed round-robin)
**Description:** As an agent, I can talk before votes so the game exercises
persuasion and deduction, with a bounded, reproducible structure.

**Acceptance Criteria:**
- [ ] Before each government's vote, discussion runs as **fixed round-robin in seat order** for a configurable number of rounds (default small, e.g. 1–2); each living player emits one utterance (or an explicit pass) per round.
- [ ] Per-utterance token/length cap enforced; total discussion cost per game is bounded and logged.
- [ ] Utterances are appended to the public history all seats observe.
- [ ] Dead players do not speak.
- [ ] Lint/Test/Build checks pass.

### US-008: Match runner with balanced role/seat rotation
**Description:** As an eval author, I want to run many games with every model
rotated fairly through seats and factions, so no model is advantaged by
assignment.

**Acceptance Criteria:**
- [ ] A `run-match` / batch command plays a configurable number of games over a set of seated models.
- [ ] Assignment rotates every model through every seat position and both factions as evenly as the schedule allows; the realized distribution is logged.
- [ ] Every game persists a replayable transcript (seed, assignments, actions, discussion, outcome, per-player signals).
- [ ] Runs are resumable at match granularity (init → continue → cleanup pattern).
- [ ] Lint/Test/Build checks pass.

### US-009: TrueSkill headline rating + per-faction win rates
**Description:** As a user, I want one comparable rating per model plus faction
win rates, so I can rank models honestly.

**Acceptance Criteria:**
- [ ] After a batch, each model has a TrueSkill rating (mean μ and uncertainty σ) computed via the Rust `skillratings` crate from game win/loss, treating the two factions as the two teams.
- [ ] Output reports rating **with its uncertainty** (not a bare point estimate).
- [ ] **Liberal win rate** and **Fascist win rate** are reported separately per model.
- [ ] Rating output states the game count and warns when uncertainty is too high to separate close models.
- [ ] Lint/Test/Build checks pass.

### US-010: Granular objective per-player metric suite
**Description:** As a researcher, I want objective, engine-derived signals per
model so I understand *how* it plays, without any subjective judge.

**Acceptance Criteria:**
- [ ] **Suspicion accuracy:** at defined checkpoints, each player emits a structured belief distribution over others' roles; scored (Brier / log-loss) against ground-truth roles. Liberal suspicion accuracy is reported.
- [ ] **Goal-aligned policy rate:** for each government choice a player made (President discard, Chancellor enact), fraction that advanced the player's own faction goal, given the tiles they held.
- [ ] **Execution accuracy:** when a player used the Execution power, whether the shot hit a Fascist/Hitler (Liberal-beneficial) vs a Liberal, per faction.
- [ ] **Vote alignment:** fraction of Ja/Nein votes consistent with the player's faction interest under the information then available (documented heuristic definition).
- [ ] Every metric is computed purely from engine state + declared beliefs; none uses an LLM to grade. Each metric documents its exact definition and its known proxy limitations.
- [ ] Metrics are persisted per game and aggregated per model.
- [ ] Lint/Test/Build checks pass.

### US-011: Results surfaced over CLI + HTTP (+ optional viz)
**Description:** As a user, I want to run evals headlessly and inspect results,
optionally visually.

**Acceptance Criteria:**
- [ ] `shbench` CLI runs a batch and prints/writes the rating + faction win rates + metric summary (JSON).
- [ ] HTTP API exposes stored results (leaderboard + per-game transcript) under `/api/v1`.
- [ ] The optional React frontend can render a game replay and a per-model summary (viz is nice-to-have, not gating).
- [ ] Lint/Test/Build checks pass.

## 4. Functional Requirements

- **FR-1:** The engine must implement the complete 5-player Secret Hitler ruleset (roles, deck, government, term limits, election tracker, Policy Peek, Execution, veto, all win conditions) as the single source of truth for state.
- **FR-2:** The engine must expose per-seat role-conditioned observations that never leak information a seat is not entitled to.
- **FR-3:** Agent decisions must be strict JSON, validated against a per-decision schema; malformed JSON triggers bounded retries.
- **FR-4:** Legal-but-illegal moves must trigger a bounded rethink with the reason; exhaustion applies a defined forced legal default.
- **FR-5:** The harness must record malformed-output and illegal-move counts separately from play-quality metrics.
- **FR-6:** The harness must support deterministic rule-based baseline agents (no discussion) usable as a CI floor.
- **FR-7:** The harness must support LLM agents via the existing `app_config` LLM configuration, with transient-failure retry.
- **FR-8:** Discussion must run as bounded fixed round-robin (configurable rounds; per-utterance length cap); all costs logged.
- **FR-9:** The match runner must rotate every model through every seat and both factions as evenly as possible and log the realized distribution.
- **FR-10:** Every game must produce a seed-reproducible, replayable transcript.
- **FR-11:** The system must compute a TrueSkill rating (μ, σ) per model from win/loss via the `skillratings` crate, treating factions as teams.
- **FR-12:** The system must report Liberal and Fascist win rates per model separately, plus the game count and an uncertainty warning.
- **FR-13:** The system must compute and persist the objective per-player metric suite (suspicion accuracy, goal-aligned policy rate, execution accuracy, vote alignment) with documented definitions.
- **FR-14:** Results must be retrievable via CLI and HTTP API; the frontend replay is optional.
- **FR-15:** All randomness (deck, assignment) must be seedable so deterministic runs reproduce exactly.

## 5. Non-Goals (Out of Scope for v1)

- **Subjective LLM-judge grading** of deception, bluff quality, or persuasion — deferred to v2. v1 measures these only *implicitly* via wins and objective proxies.
- **Any multi-game abstraction** (no generic `Game` trait). Secret Hitler only. Avalon/Hanabi/Diplomacy remain *design references*, not build targets.
- **6–10 player rulesets** and their extra powers, notably **Investigate Loyalty** (a 7–10 player power). The suspicion-accuracy metric substitutes for the missing investigate signal at 5 players.
- **Human-vs-model play**, matchmaking, or a hosted public leaderboard service.
- **Static single-decision "spot" suites** (PokerBench Mode A style) — live play only in v1.
- **Duplicate-deck / mirror-match variance reduction** — not portable to a 5-player team game; variance is instead handled by TrueSkill uncertainty + balanced rotation + game volume.

## 6. Design Considerations

- Reuse the engine `Command` registry + `Ctx` pattern: `init_match` → `advance` → `finalize` follows the repo's long-running init→continue→cleanup convention; matches carry a descriptive `runId`.
- The React frontend is framed as an **optional visualization layer** (game replay, who-suspected-who), not core infrastructure.
- Prior art to mirror: **AvalonBench** (near-isomorph; free-form discussion handling; rule-based baselines), **PokerBench** (strict JSON action schema; forced-default on invalid action), **Kaggle Game Arena** (external state authority; two-stage parse; rethink-on-illegal), **TrueSkill2 / OpenSkill** (team credit assignment).

## 7. Technical Considerations

- **Backend:** Rust — `engine` crate (game + eval logic, no transport deps), `shbench` binary (CLI + HTTP API). Rating via the `skillratings` crate (TrueSkill).
- **Engine correctness is load-bearing:** every metric derives from engine state, so US-002's test suite gates everything else. Prioritize it.
- **Cost drivers:** discussion tokens dominate (every seat re-ingests growing history each turn). Mitigations: fixed 5 seats, bounded discussion rounds, per-utterance cap, and the ability to run baseline (zero-token) games for CI.
- **Belief elicitation:** the suspicion-accuracy metric requires a structured side-channel prompt at checkpoints; it must not alter game state and its cost is counted.

### Honest tradeoffs (stated, not hidden)

- **The rating is coarse, relative, and non-stationary.** TrueSkill/OpenSkill accuracy degrades in multiplayer/team/FFA vs head-to-head; expect wide σ, slow convergence, and reliable separation only between *tiers*, not adjacent ranks. Ratings shift when the model pool changes and can mask non-transitive (rock-paper-scissors) metagames.
- **Win/loss is low-bandwidth** (≈1 bit/game): many games are needed and it never explains *why* a model wins. The objective metric suite is the partial remedy.
- **Per-player signals are proxies** that can reward lucky outcomes over good decisions (e.g. a vote that happened to align). Definitions are documented with their limitations; they inform, they don't adjudicate.
- **Rule-based baselines don't exercise persuasion** and are exploitable; they are a floor/CI anchor, explicitly not the leaderboard.

## 8. Success Metrics

- Engine test suite covers every edge case in US-002 and runs green in CI; deterministic games reproduce bit-for-bit from a seed.
- A full LLM-vs-LLM 5-player game completes end-to-end (discussion → votes → policies → powers → terminal win condition) with a replayable transcript.
- A batch run produces per-model TrueSkill ratings (with σ), per-faction win rates, and the full objective metric suite.
- Baseline-only games run in CI as a sub-second deterministic smoke test.
- Malformed-output and illegal-move rates are reported per model and separable from play quality.
- Total token cost per LLM game is bounded and logged, staying within the configured discussion budget.

## 9. Open Questions

Decisions taken by **recommendation** (not explicit confirmation) — confirm or redirect:

1. **Seat count = fixed 5-player for v1.** Alternatives considered: 5–6, or full 5–10 with a seat cap. 5-player is simplest/cheapest and gives cleanest variance; note it omits Investigate Loyalty (7–10 power).
2. **Rating = Rust `skillratings` TrueSkill.** Alternatives: an OpenSkill (Weng-Lin) port with per-player contribution weights, or a simpler faction-adjusted Elo first.
3. **Discussion = fixed round-robin, N rounds.** Alternatives: free speaking-order with a token budget (more natural, harder to bound/reproduce), or a mechanics-only first milestone with discussion added later.

Still genuinely open:

4. **Belief-elicitation cadence:** at which checkpoints do we ask players for role suspicion distributions (every government? every policy enacted? end only?) — affects both cost and the granularity of suspicion accuracy.
5. **Vote-alignment definition:** the exact objective heuristic for "vote consistent with faction interest" needs pinning; it is the softest of the objective metrics.
6. **Forced legal default policy:** the precise minimal-impact fallback action per decision type when the rethink budget is exhausted.
7. **Baseline strategy strength:** how strong the rule-based baselines should be (a weak floor vs a tuned mechanical opponent) — affects how discriminating the CI floor is.
