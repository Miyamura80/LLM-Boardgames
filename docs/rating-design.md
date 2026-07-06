# Rating & Evaluation Design (v1.1 addendum)

This document reconciles the original PRD (`PRD-secret-hitler-evals.md`) with the
rating-design review that followed it. Where the two disagree, **this document
wins**. It covers the rating unit, schedules, anchor pools, and what v1 defers.

## 1. Rating unit: model × role

A single flat rating hides role asymmetry, so the rated entity is:

```
(model, scaffold_version) × role      role ∈ {Liberal, Fascist, Hitler}
```

- Ratings use **Weng-Lin (OpenSkill)** via the `skillratings` crate.
- Every game updates ratings as a two-team match: Liberal team (4 seats) vs
  Fascist team (3 seats, Hitler included).
- When the same rating entity occupies multiple seats in one game (duplicates
  are allowed in arena mode), each seat's update is computed independently and
  the **deltas are averaged** into the entity — self-play games therefore
  contribute little net signal, which is the correct behavior.
- Reported per model:

| Field | Definition |
| --- | --- |
| Liberal / Fascist / Hitler rating | μ ± σ per role |
| Conservative score | μ − kσ, k = 2 (configurable; use 3 for public boards) |
| Overall | Role-frequency-weighted aggregate: 4/7·Lib + 2/7·Fasc + 1/7·Hitler |
| Win rates | Per role, plus game counts |
| Uncertainty warning | Emitted when σ is too wide to separate adjacent models |

Anchors (see §3) receive ratings too — they anchor the scale — but are flagged
`anchor` and excluded from the leaderboard.

## 2. Schedules

### 2.1 Controlled mode (primary)

One **candidate** + six **anchors** per game. Per candidate the schedule
enumerates every role-seat cell:

```
3 roles × 7 seats × K repetitions  =  21K games per candidate
```

K guidance: 5 = screening, 10 = coarse ranking, 20 = serious eval, 40 = close
comparison.

**Mirrored seeds:** the deck/assignment seed is a pure function of
`(match_seed, role, seat, repetition)` — *independent of the candidate* — so two
candidates run through the same cell see the same deck order and the same anchor
arrangement. This makes cross-candidate comparisons paired.

### 2.2 Arena mode (validation)

Mixed candidate-vs-candidate games with balanced rotation through seats and
factions. Seats are always 7; when fewer than 7 distinct models are listed,
duplicates fill seats and the delta-averaging rule above applies. Arena ratings
are stored under a separate rating pool and are **not blindly merged** with
controlled ratings.

Suggested budget split (operational, not enforced in code): ~70–80% controlled,
~20–30% arena/validation.

## 3. Anchor pools

Anchor pools are frozen, versioned configurations (name, agent kind, model,
persona prompt, temperature). Two pools ship:

**Pool A — balanced (main ratings):**

| Anchor | Kind |
| --- | --- |
| `random-legal` | Bot: uniformly random legal actions, no discussion |
| `heuristic` | Bot: simple mechanical strategy, no discussion |
| `bayes-history` | Bot: tracks votes/policies, Bayes-flavored suspicion, no discussion |
| `llm-cheap` / `llm-mid` / `llm-strong` | LLM anchors with a frozen neutral prompt |

**Pool B — adversarial styles (cross-pool validation):** LLM anchors with
frozen persona prompts — aggressive accuser, quiet Bayesian, deceptive,
coalition-builder, contrarian — plus one strong neutral anchor.

Rules: anchors never appear on the leaderboard; anchor prompts/versions are
frozen and hashed into `scaffold_version`; at most one random bot per game;
every candidate sees the same anchor distribution.

> **Environment caveat:** in this repository's default config all LLM anchors
> are Gemini-family because only `GEMINI_API_KEY` is provisioned. The review's
> §6.7 (model-family collusion effects) applies — diversify anchor providers by
> adding API keys and editing the pool config before publishing results.

## 4. Auxiliary metrics

v1 keeps the PRD's objective, engine-derived suite (no LLM judge):

- suspicion calibration (Brier vs ground truth, from private belief elicitation)
- goal-aligned enactment rate (luck-controlled, per policy decision)
- faction policy throughput (outcome stat, strict attribution)
- execution accuracy
- Hitler survival (governments survived as Hitler)
- seat-conditioned win rates
- reliability counters: malformed-output, illegal-move, forced-default

Deferred to v2 (need an LLM judge or a structured claim channel): claim
consistency, contradiction rate, persuasion/deception success, coalition
influence. Transcripts store everything required to compute them later.

## 5. Scaffold versioning

The rated unit is *model + scaffold*. Every prompt template is checked into the
repo; `scaffold_version` is a short hash of the rendered template set plus the
temperature, recorded on every seat of every game.

## 6. Cost model

Planning default $0.50/game is plausible for flash-tier models but sensitive to
full-history prompting. The engine logs per-game input/output tokens and derived
cost so the review's budget tables (§18–19) can be validated empirically before
committing to a K.
