#!/usr/bin/env bash
#
# Verify that every LLM seat model id (arena sets + controlled anchor pools)
# resolves and authenticates against its provider. For each model it runs a
# 1-game, discussion-off arena against cheap bots (minimises tokens) and reads
# the `transport` reliability counter from sh_leaderboard.
#
#   transport == 0  → id resolves + auth works                       ✅
#   transport high  → wrong id, or a missing / invalid provider key  ❌
#
# See docs/runbooks/arena-pilot.md §1 for the manual version of this check.
#
# Prereqs (run where the keys + Docker live — NOT in an egress-blocked sandbox):
#   docker compose up -d
#   export DATABASE_URL=postgres://shbench:shbench@localhost:5433/shbench
#   export OPENAI_API_KEY=... ANTHROPIC_API_KEY=... GEMINI_API_KEY=... OPENROUTER_API_KEY=...
#   bash scripts/verify_model_slugs.sh
#
# Re-testing a slug after fixing a KEY (same slug → same run_id → cached):
#   RUN_SUFFIX=-retry bash scripts/verify_model_slugs.sh
# (Fixing a SLUG changes the run_id automatically, so no suffix is needed.)
#
# Requires: jq, cargo (only if shbench isn't already built).

set -uo pipefail

MODELS=(
  # first-party (known-good — pinged for completeness)
  "openai/gpt-5.5"
  "anthropic/claude-opus-4-8"
  "gemini/gemini-3.5-flash"
  # arena sets — OpenRouter (unverified: AI-guessed slugs)
  "openrouter/z-ai/glm-5.2"
  "openrouter/minimax/minimax-m3"
  "openrouter/deepseek/deepseek-v4-pro"
  "openrouter/x-ai/grok-4.3"
  "openrouter/nvidia/nemotron-3-ultra-550b-a55b"
  "openrouter/openai/gpt-oss-120b"
  "openrouter/moonshotai/kimi-k2.6"
  # controlled anchor-pool LLMs (pool-a / pool-b in global_config.yaml)
  # (deepseek-v4-pro is the llm-mid anchor too, already covered above)
  "openrouter/mistralai/mistral-small-3.2-24b-instruct"
  "gemini/gemini-3-flash-preview"
)

# --- locate the shbench binary ------------------------------------------------
if command -v shbench >/dev/null 2>&1; then
  SH=(shbench)
elif [[ -x target/release/shbench ]]; then
  SH=(target/release/shbench)
elif [[ -x target/debug/shbench ]]; then
  SH=(target/debug/shbench)
else
  echo "shbench not found — building (release)…" >&2
  cargo build --release --bin shbench || { echo "cargo build failed" >&2; exit 1; }
  SH=(target/release/shbench)
fi

# --- preconditions ------------------------------------------------------------
command -v jq >/dev/null 2>&1 || { echo "jq is required" >&2; exit 1; }
[[ -n "${DATABASE_URL:-}" ]] || {
  echo "DATABASE_URL is unset — start Postgres with 'docker compose up -d'" >&2; exit 1; }
for v in OPENAI_API_KEY ANTHROPIC_API_KEY GEMINI_API_KEY OPENROUTER_API_KEY; do
  [[ -n "${!v:-}" ]] || echo "⚠️  $v unset — seats on that provider will fail as transport errors" >&2
done

SFX="${RUN_SUFFIX:-}"
ERRLOG="verify_errors.log"
: > "$ERRLOG"
slugify() { printf '%s' "$1" | tr '/:.' '-'; }

# FREE=1 pings the :free OpenRouter variant where one exists (currently Nemotron
# and gpt-oss) for a $0 plumbing/auth dry-run. A :free variant is a DIFFERENT
# deployment with different limits, so it does NOT verify the exact paid slug —
# shake out the harness with FREE=1, then run once more without it.
free_slug() {
  case "$1" in
    openrouter/nvidia/nemotron-3-ultra-550b-a55b) echo "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free" ;;
    openrouter/openai/gpt-oss-120b)               echo "openrouter/openai/gpt-oss-120b:free" ;;
    *) echo "" ;;
  esac
}
[[ "${FREE:-}" == "1" ]] && \
  echo "FREE=1: pinging :free variants where available (nemotron, gpt-oss) — verifies plumbing, NOT the paid slug." >&2

printf '%-48s | %5s | %5s | %5s | %5s | %s\n' "model" "trans" "malf" "illg" "frcd" "verdict"
printf -- '%.0s-' {1..96}; printf '\n'

total_prompt=0; total_completion=0; fails=0
for model in "${MODELS[@]}"; do
  seat_model="$model"; tag=""
  if [[ "${FREE:-}" == "1" ]]; then
    fv=$(free_slug "$model"); [[ -n "$fv" ]] && { seat_model="$fv"; tag=" (free)"; }
  fi
  rid="verify-$(slugify "$seat_model")$SFX"

  run_args=$(jq -nc --arg m "$seat_model" --arg rid "$rid" '{
    mode:"arena",
    models:[$m,"bot:heuristic","bot:heuristic","bot:bayes-history",
            "bot:random-legal","bot:heuristic","bot:random-legal"],
    games:1, discussion_rounds:0, belief_checkpoints:false, run_id:$rid
  }')
  "${SH[@]}" call sh_run_match --json --args "$run_args" >/dev/null 2>>"$ERRLOG"

  lb_args=$(jq -nc --arg rid "$rid" '{run_id:$rid, include_anchors:true}')
  lb=$("${SH[@]}" call sh_leaderboard --json --args "$lb_args" 2>>"$ERRLOG")

  # The one non-bot metrics row (candidate) — robust to any output envelope and
  # to the scaffold-hash suffix on model_id.
  row=$(printf '%s' "$lb" | jq -c '
    [ .. | objects
      | select((.transport? != null) and (.model_id? != null)
               and ((.model_id | startswith("bot:")) | not)) ] | .[0]' 2>/dev/null)

  if [[ -z "$row" || "$row" == "null" ]]; then
    printf '%-48s | %5s | %5s | %5s | %5s | %s\n' "$model$tag" "?" "?" "?" "?" \
      "❌ no metrics row — see $ERRLOG"
    fails=$((fails+1)); continue
  fi

  trans=$(jq -r '.transport'        <<<"$row")
  malf=$(jq -r '.malformed'         <<<"$row")
  illg=$(jq -r '.illegal'           <<<"$row")
  frcd=$(jq -r '.forced'            <<<"$row")
  pt=$(jq -r '.prompt_tokens'       <<<"$row")
  ct=$(jq -r '.completion_tokens'   <<<"$row")
  total_prompt=$((total_prompt + pt)); total_completion=$((total_completion + ct))

  if [[ "$trans" -eq 0 ]]; then
    verdict="✅ resolves + auth ok"
  else
    verdict="❌ transport>0 — wrong id or bad/missing key (fix slug on openrouter.ai/models)"
    fails=$((fails+1))
  fi
  printf '%-48s | %5s | %5s | %5s | %5s | %s\n' "$model$tag" "$trans" "$malf" "$illg" "$frcd" "$verdict"
done

echo
echo "token spend across all verify runs: prompt=${total_prompt} completion=${total_completion}"
echo "failures: ${fails}"
[[ "$fails" -gt 0 ]] && echo "→ for each ❌: find the correct slug on the provider's model list, edit crates/config/global_config.yaml, re-run."
# Non-zero exit when any seat failed, so CI / automated roster validation halts.
exit $(( fails > 0 ? 1 : 0 ))
