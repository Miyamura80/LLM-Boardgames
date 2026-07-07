# Runbook: running an arena pilot (frontier / cheap leagues)

How to take a `model_sets` league from config to a trustworthy leaderboard
without lighting money on fire. Covers id verification, a cost/latency
calibration run, the go/no-go metrics, and scaling up.

The two leagues live in `crates/config/global_config.yaml` under
`secret_hitler.model_sets` (`frontier`, `cheap`) — each is exactly 7 models so
arena fills every seat with no duplicates.

---

## 0. Prerequisites

```bash
docker compose up -d                 # Postgres eval store
export DATABASE_URL=postgres://shbench:shbench@localhost:5433/shbench
```

Routing is by litellm-style prefix (see `crates/engine/src/llm/providers.rs`):
**OpenAI / Anthropic / Gemini are first-party** (their own keys); **every other
family routes through OpenRouter** (`openrouter/<org>/<model>`, one key). The
shipped `model_sets` follow this split, so a pilot needs just these four keys:

```bash
export OPENAI_API_KEY=... ANTHROPIC_API_KEY=... GEMINI_API_KEY=...
export OPENROUTER_API_KEY=...        # covers every openrouter/… seat
```

> **Egress:** the seat calls go out to real provider APIs. In a locked-down
> sandbox they will fail at CONNECT — run this where outbound HTTPS to the
> providers is allowed.

---

## 1. Verify every model id resolves (cheap smoke test)

The model strings in `model_sets` are best-effort — AA display names are not API
ids (DeepSeek wants `deepseek-chat`/`deepseek-reasoner`, xAI `grok-*`, etc.). A
wrong id fails **loudly** as a transport error, so smoke each one before a paid
run. Seat the model in a **1-game, discussion-off, beliefs-off** arena against
six cheap bots (minimises tokens):

```bash
shbench call sh_run_match --json --args '{
  "mode":"arena",
  "models":["openai/gpt-5.5","bot:heuristic","bot:heuristic",
            "bot:bayes-history","bot:random-legal","bot:heuristic","bot:random-legal"],
  "games":1,"discussion_rounds":0,"belief_checkpoints":false,
  "run_id":"verify-gpt-5.5"
}'
```

Then read the reliability counters for that run:

```bash
shbench call sh_leaderboard --json --args '{"run_id":"verify-gpt-5.5","include_anchors":true}'
```

- **`transport` = 0** on the model's row → id resolves and auth works. ✅
- **`transport` high** (every call failed) → wrong id or missing/invalid key.
  Fix the string in `model_sets` (or repoint it at `openrouter/…`) and re-smoke.

Repeat for each distinct model. Unverified OpenRouter slugs as of writing:
`openrouter/z-ai/glm-5.2`, `openrouter/deepseek/deepseek-v4-pro`,
`openrouter/meta/muse-spark`, `openrouter/nvidia/nemotron-3-ultra`,
`openrouter/openai/gpt-oss-120b`, `openrouter/x-ai/grok-4.3`.

---

## 2. Calibration run — the real gate

Play a **small real arena** (discussion on) for the cheaper league first and
read cost/latency/reliability before committing to volume:

```bash
shbench call sh_run_match --json --args '{"mode":"arena","set":"cheap","games":10,"max_games":10}'
```

The response carries `progress.prompt_tokens` / `progress.completion_tokens`
(this call's spend) and, once all games are stored, the `leaderboard` +
`metrics`. Pull the per-model breakdown any time:

```bash
shbench call sh_leaderboard --json --args '{"run_id":"run-arena-<hash>"}'
```

### Go / no-go fields

| Field (per model, from `metrics`) | Healthy | If not |
| --- | --- | --- |
| `forced` (forced-default count) | ≈ 0 | Model can't emit legal JSON → play quality is corrupted, not measured. Investigate before scaling. |
| `malformed` / `illegal` | low | Reliability problem; the rethink loop is absorbing it but it costs tokens. |
| `completion_tokens` / games | within budget | The cost driver. Reasoning models at max effort balloon here — this is the "overthinking tax". |
| `transport` | 0 | Flaky provider or rate-limiting; retries are burning into forced-defaults. |

Cost per game ≈ `prompt_tokens × input_rate + completion_tokens × output_rate`
for each seat's provider (input and output are billed at **separate** rates).
`completion_tokens` × the output rate dominates for reasoning models.

---

## 3. Decide and scale

- `forced ≈ 0` **and** cost acceptable → bump `games` (e.g. 30–50) for a more
  stable ranking. Arena ratings need volume; expect `high_uncertainty: true`
  at low game counts (that flag is honest — see `rating.rs`).
- Run the **frontier** league the same way but with a smaller `games` count —
  Opus 4.8 + GPT-5.5 + GLM-5.2-max in a 7-seat, 3-round game is expensive.

```bash
shbench call sh_run_match --json --args '{"mode":"arena","set":"frontier","games":20,"max_games":20}'
```

Runs are **resumable**: re-invoke the same command (same spec → same `run_id`);
already-stored games are skipped and only the remainder plays. `max_games`
chunks a long run so you can inspect cost between batches.

---

## 4. Read and export results

```bash
# Ranked leaderboard (μ±σ, conservative score, per-role win rates, metrics)
shbench call sh_leaderboard --json --args '{"run_id":"run-arena-<hash>"}'

# List stored games, then export one to a self-contained HTML report
shbench call sh_list_games      --json --args '{"run_id":"run-arena-<hash>"}'
shbench call sh_export_game_report --json --args '{"game_id":"<id>","output_path":"game.html"}'
```

Interpretation:

- Rank by **`overall_conservative`** (μ − kσ), not raw μ.
- **`high_uncertainty`** rows overlap a neighbour's rating interval — don't read
  their relative order yet; play more games.
- **Per-role win rates** (`liberal`/`fascist`/`hitler`) expose faction skew a
  single number hides.

---

## Notes

- Discussion cost is bounded by `discussion_rounds` (default 3) ×
  `utterance_char_cap` (240) — tune in `global_config.yaml` if a league is too
  costly; both are part of the scaffold version, so changing them re-buckets
  ratings.
- All anchors/candidates are rated as `(model + scaffold)`; a prompt or
  temperature change moves the scaffold id automatically (`prompts.rs`), so
  don't mix results across scaffold changes.
