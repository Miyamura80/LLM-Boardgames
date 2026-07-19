This file provides guidance to AI agents working with code in this repository.

## Project Overview

An LLM-agent evaluation harness for **Secret Hitler**, the hidden-role
social-deduction game. The engine-authoritative game state and eval logic are
written **once** as typed async `Command`s in the `engine` crate and exposed over
multiple transports — CLI, HTTP API, and (later) MCP — through the `shbench`
binary. The `engine` core has no transport dependency; transports live in
`crates/cli` behind cargo features. An optional React/Vite frontend (`frontend/`)
visualizes games and replays over the HTTP API via `fetch`.
**Note:** migrated away from Tauri/desktop and from Python — Rust for backend,
Node/Bun for frontend/scripts.
**Before any other work in this repo, enable prek:** `bun add -g prek && prek install`. Hooks are defined in `prek.toml`.

## Common Commands

```bash
make run                # Run the HTTP API server (= shbench serve)
cargo test --workspace  # Run Rust tests
cargo clippy --workspace --all-targets -- -D warnings
shbench call ping --json # Invoke a command headlessly
make new name=fetch_url # Scaffold a new engine command
make dev                # Optional frontend: Vite dev server, /api → shbench serve
```

## Architecture

- **crates/engine/** — typed async `Command` registry with `inventory`
  self-registration; per-request `Ctx`; capability traits. No transport deps.
- **crates/cli/** — the `shbench` binary; `cli` and `http-api` are cargo features
  (both default), so a surface can be pruned and still compile.
- **crates/config/** — crate `app-config`; `AppConfig` (secrets) vs sanitized
  `FrontendConfig` (served over HTTP). The sanitizer is a security boundary.
- **frontend/** — optional React/Vite app, `fetch`-based `/api/v1` client.

> **Making backend changes?** Use the `update-backend` skill for architecture details, command patterns, trait implementations, config access, and `shbench` testing workflows.

## Code Style

Enforced by Biome (TS) and `cargo fmt` + Clippy (Rust). See `biome.json`.

## Configuration Pattern

Configuration is handled in Rust and exposed to the frontend via the sanitized
`FrontendConfig`. Source of truth: `crates/config/global_config.yaml` (`.env` /
`APP__`-prefixed env overrides; `APP_CONFIG_PATH` for a deployed binary).

```rust
let config = app_config::get_config();
println!("Model: {}", config.default_llm.default_model);
```

## Commit Message Convention

Use emoji prefixes indicating change type and magnitude (multiple emojis = 5+ files):
- 🏗️ initial implementation
- 🔨 feature changes
- 🐛 bugfix
- ✨ formatting/linting only
- ✅ feature complete with E2E tests
- ⚙️ config changes
- 💽 DB schema/migrations

## Long-Running Code Pattern

Structure as: `init()` → `continue(id)` → `cleanup(id)`
- Keep state serializable
- Use descriptive IDs (runId, taskId)
- Handle rate limits, timeouts, retries at system boundaries

## Subagents

- Folder-size CI failure → spawn subagent `.claude/agents/folder-refactor-advisor.md`.

## Git Workflow
- **Protected Branch**: `main` is protected. Do not push directly to `main`. Use PRs.
- **Merge Strategy**: Squash and merge.
- **Pre-commit CI gate**: Always run `make ci` before committing any changes. Ensure it passes with zero errors. Do not commit if `make ci` fails - fix all issues first, then commit.
- **Prek hooks**: Always run `prek install` before starting work on a new PR to ensure Git hooks are active.

## Runbooks

Operational runbooks live in `docs/runbooks/`. After resolving a difficult issue that required significant back-and-forth or investigation, ask the user: "Should I add a runbook for this?" and if yes, create a new markdown file in `docs/runbooks/` documenting the symptoms, root cause, and resolution steps.

---

## Automated Translation (Jules Sync)

Docs under `docs/content/` are auto-translated by the **Jules Translation Sync**
workflow (`.github/workflows/jules-sync-translations.yml`). Do NOT manually
translate doc files — edit the English source and the workflow will update all
locales (`zh`, `es`, `ja`).

See [`docs/translation-guide.md`](docs/translation-guide.md) for the full
glossary, file naming conventions, and translation rules.
See [`docs/ops.md`](docs/ops.md) for operational runbook and failure modes.
