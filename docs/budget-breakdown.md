# Budget Breakdown — model roster & program cost

Fills the gap left open by [`rating-design.md`](rating-design.md) §6, which
pegs a planning default of **$0.50/game** for flash-tier models and defers to
"budget tables (§18–19)" that never landed in the repo. This doc costs the
**combined `frontier` + `cheap` arena roster** against empirical run costs and
derives per-game and whole-program budgets.

> **Status: modeled estimate, not measured.** Every dollar figure below is a
> first-order projection anchored to the repo's own $0.50/flash-game assumption
> and Artificial Analysis's "cost to run the Intelligence Index" (which already
> bakes in input, cache, reasoning, and answer tokens). Validate against real
> per-turn token logs before committing to a K — see [Action items](#action-items).
> Prices checked **8 July 2026**.

## 1. Combined roster (frontier ∪ cheap, deduped)

The two shipped leagues in `crates/config/global_config.yaml`
(`secret_hitler.model_sets`) are 7 models each; four overlap
(`deepseek-v4-pro`, `minimax-m3`, `glm-5.2`, `grok-4.3`), leaving **10 distinct
models**.
"Effective cost" is ranked by Artificial Analysis's *cost to run the
Intelligence Index* — the honest cross-model comparator, because reasoning-heavy
models burn hidden thinking tokens that a sticker output price hides.

| Rank | Model | API slug | In $/1M | Out $/1M | AA run cost | Reasoning? | ×Flash |
| ---: | --- | --- | ---: | ---: | ---: | --- | ---: |
| 1 | GPT-OSS 120B | `openrouter/openai/gpt-oss-120b` | 0.03 | 0.15 | $96 | yes (cfg) | 0.09× |
| 2 | DeepSeek V4 Pro | `openrouter/deepseek/deepseek-v4-pro` | 0.44 | 0.87 | $176 | yes | 0.17× |
| 3 | MiniMax M3 | `openrouter/minimax/minimax-m3` | 0.30 | 1.20 | $204 | unclear | 0.20× |
| 4 | Grok 4.3 | `openrouter/x-ai/grok-4.3` | 1.25 | 2.50 | $288 | yes (low dflt) | 0.28× |
| 5 | Nemotron 3 Ultra | `openrouter/nvidia/nemotron-3-ultra-550b-a55b` | 0.50 | 2.20 | $443 | yes | 0.43× |
| 6 | GLM-5.2 | `openrouter/z-ai/glm-5.2` | 0.93 | 3.00 | $820 | yes | 0.79× |
| 7 | Kimi K2.6 | `openrouter/moonshotai/kimi-k2.6` | 0.66 | 3.41 | $852 | yes | 0.82× |
| 8 | Gemini 3.5 Flash | `gemini/gemini-3.5-flash` | 1.50 | 9.00 | $1,041 | yes | 1.00× |
| 9 | GPT-5.5 | `openai/gpt-5.5` | 5.00 | 30.00 | $2,630 | yes (med dflt) | 2.53× |
| 10 | Claude Opus 4.8 | `anthropic/claude-opus-4-8` | 5.00 | 25.00 | $3,753 | yes (high dflt) | 3.61× |

Slug fixes vs the config's original best-guesses (both **applied** to
`global_config.yaml`):
- **`nvidia/nemotron-3-ultra` → `nvidia/nemotron-3-ultra-550b-a55b`** — the short
  slug does not resolve on OpenRouter.
- **`meta/muse-spark` has no usable public route** — Meta ships it on Meta AI
  with API in private preview; AA's "$0" is placeholder, not a real price. It is
  **replaced in `frontier` by Grok 4.3** (`openrouter/x-ai/grok-4.3`): a frontier
  flagship with a confirmed slug that restores provider-family diversity (a 7th
  distinct family) and is the cheapest frontier-tier option.
- All other slugs resolve first-party (`openai/`, `anthropic/`, `gemini/`) or on
  OpenRouter.

## 2. Cost model

The rated unit costs derive from one anchor and one comparator:

```
  anchor:  all-7-flash-seat game ≈ $0.50   (rating-design.md §6)
             │
             ▼
  per-flash-seat  = $0.50 / 7 seats ≈ $0.0714 / seat / game
             │
             │   scale by empirical reasoning-inclusive cost ratio
             ▼
  r(model) = AA_run_cost(model) / AA_run_cost(Flash)      [×Flash column]
             │
             ▼
  per-seat(model) ≈ $0.0714 × r(model)
             │
   ┌─────────┴───────────────────────────────────────────┐
   ▼                                                       ▼
  CONTROLLED game (primary)                        ARENA game (validation)
  1 candidate + 6 anchors                          7 candidate seats, no bots
  (3 bots = $0, 3 LLM anchors ≈ $0.085)            cost = Σ per-seat(7 models)
  cost = per-seat(cand) + $0.085
```

Why scale by AA run cost rather than sticker output price: this harness is
**output-volume-bound** (7 seats each re-ingest the growing public history every
turn, and reasoning tokens bill as output). AA's run cost is the only figure
that captures how token-hungry each model *actually* is — DeepSeek V4 Pro emits
~180M tokens/Index run yet still ranks #2 because its tokens are near-free, while
Kimi/GLM are "cheap-until-they-talk."

**Anchor block (controlled mode):** pool-a is 3 bots (`random`, `heuristic`,
`bayes` — zero tokens) + 3 LLM anchors (`mistral-small-3.2-24b-instruct`,
`deepseek-v4-pro`, `gemini-3-flash`). Modeled at **≈ $0.085/game** (V4 Pro
reasons, but its tokens are cheap enough to stay within rounding), dominated by
the flash-tier strong anchor. Note `deepseek-v4-pro` is also an arena candidate,
so in its own controlled cells it partly anchors itself — a known, accepted
wrinkle.

## 3. Per-game cost

### Controlled mode (1 candidate + anchors) — the primary schedule

| Candidate | per-seat | + anchors | **$/game** |
| --- | ---: | ---: | ---: |
| GPT-OSS 120B | 0.007 | 0.085 | **0.092** |
| DeepSeek V4 Pro | 0.012 | 0.085 | **0.097** |
| MiniMax M3 | 0.014 | 0.085 | **0.099** |
| Grok 4.3 | 0.020 | 0.085 | **0.105** |
| Nemotron 3 Ultra | 0.030 | 0.085 | **0.116** |
| GLM-5.2 | 0.056 | 0.085 | **0.142** |
| Kimi K2.6 | 0.058 | 0.085 | **0.143** |
| Gemini 3.5 Flash | 0.071 | 0.085 | **0.156** |
| GPT-5.5 | 0.180 | 0.085 | **0.266** |
| Claude Opus 4.8 | 0.257 | 0.085 | **0.343** |

### Arena mode (7 candidate seats) — validation only

| Set | Seats | **$/game** |
| --- | --- | ---: |
| `cheap` (7 seats) | DeepSeek, GPT-OSS, MiniMax, Grok, Nemotron, Kimi, GLM | **≈ 0.20** |
| `frontier` (7 seats) | GPT-5.5, Opus 4.8, Flash, GLM, MiniMax, DeepSeek, Grok 4.3 | **≈ 0.61** |

## 4. Program budget

Controlled schedule is **21·K games per candidate** (3 roles × 7 seats × K reps;
`rating-design.md` §2.1). Totals below are the **whole 10-model roster**, summed
across per-candidate game costs.

| K (purpose) | games/candidate | roster controlled total |
| --- | ---: | ---: |
| 5 — screening | 105 | **≈ $164** |
| 10 — coarse ranking | 210 | **≈ $327** |
| 20 — serious eval | 420 | **≈ $655** |
| 40 — close comparison | 840 | **≈ $1,310** |

Arena validation is cheap because game counts are small (tens):
`cheap` set × 30 games ≈ **$6**; `frontier` set × 20 games ≈ **$12**.

**Headline scenarios** (controlled + arena, with a ~25% buffer for retries,
belief-elicitation side calls, and forced-default rethinks):

```
  screening   (K=5)   :  $164 + ~$18 arena  →  ~$230  incl. buffer
  serious     (K=20)  :  $655 + ~$18 arena  →  ~$840  incl. buffer
  close-comp  (K=40)  : $1,310 + ~$18 arena → ~$1,660 incl. buffer
```

The program is dominated by controlled runs of the two costly frontier
candidates: **Opus 4.8 ($144/candidate @ K=20) and GPT-5.5 ($112)** together are
~40% of the serious-tier bill; the other eight candidates are $39–$60 each.

## 5. Findings

**Cost traps** (cheap sticker, expensive in practice — they reason a lot):
- **Kimi K2.6** and **GLM-5.2** — low OpenRouter output price, but ~140–170M
  tokens/Index run puts real cost at $820–852 (8× GPT-OSS).
- **Nemotron 3 Ultra** — mid sticker, verbosity pushes it to $443.
- **DeepSeek V4 Pro** — the *exception*: most verbose (~180M tokens) yet #2,
  because output is ~$0.87/1M. A trap only if your provider/cache path is worse
  than OpenRouter sticker.

**Genuinely cheap** (low verbosity *and* low price):
- **GPT-OSS 120B** — best empirical cost by far ($96/run); the budget workhorse.
- **MiniMax M3** — low sticker, mid verbosity, $204/run.
- **Grok 4.3** — default-low reasoning keeps it reasonable at $288.

**Just plainly expensive** (not traps, honestly priced): **GPT-5.5**,
**Claude Opus 4.8** — reserve these for small-K controlled runs or short arena
games.

## 6. Action items

1. ~~**Muse Spark**: no public API route~~ — **resolved**: `frontier` now seats
   **Grok 4.3** (`openrouter/x-ai/grok-4.3`) in its place.
2. ~~**Nemotron slug**~~ — **resolved**: `global_config.yaml` now uses
   `nvidia/nemotron-3-ultra-550b-a55b`.
3. **Instrument before trusting these numbers.** Log per-turn `output_tokens`,
   `reasoning_tokens`, `cache_read_tokens`, `cache_write_tokens` per seat. The
   whole model above is a proxy; real cost lives in those counters.
4. **Prompt caching is unmodeled and material** — re-ingesting growing history
   every turn is the input-cost driver, and Anthropic/OpenAI/Gemini cached-input
   tiers (Opus cache-read ≈ 10% of input, GPT-5.5 cached input $0.50/1M) could
   cut the two expensive candidates' bills substantially. Model it once the token
   counters land.

## Sources

- Artificial Analysis — cost-to-run-Intelligence-Index per model (blended
  7:2:1 cache:input:output weighting): https://artificialanalysis.ai
- OpenAI GPT-5.5: https://developers.openai.com/api/docs/models/gpt-5.5
- Anthropic Claude Opus 4.8: https://platform.claude.com/docs/en/about-claude/models/overview
- Google Gemini 3.5 Flash: https://ai.google.dev/gemini-api/docs/pricing
- OpenRouter model pages (GLM-5.2, MiniMax-M3, DeepSeek-V4-Pro, GPT-OSS-120B,
  Grok-4.3, Nemotron-3-Ultra, Kimi-K2.6): https://openrouter.ai/models
- Meta Muse Spark (private-preview API): https://ai.meta.com/blog/introducing-muse-spark-msl/
