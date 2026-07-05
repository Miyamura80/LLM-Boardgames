---
name: onboarding
description: Interview the user, inspect this template repo, run headless onboarding, and prune unused systems so a new Rust project gets running quickly.
---

# Onboarding

Use this skill when the user wants to turn this template into a real project,
especially when they invoke `/onboarding`, ask to run onboarding, or want to
remove unused template systems.

`crates/cli/src/init/config.rs` is the source of truth for every choice,
default, and prune action. Read it before changing anything. Its enums and the
`Config` type (`Profile`, `Surface`, the `expand()` implications, and the
prune-path groups) define what can be selected and how selections imply each
other.

## Workflow

1. Inspect the repo before changing anything:
   - `AGENTS.md` / `CLAUDE.md`, the workspace `Cargo.toml`, `Makefile`, `README.md`
   - `crates/cli/src/init/config.rs` — especially the config enums, the
     per-profile presets, `expand()` (dependency implications), and the
     prune-path groups (exactly which files/crates each choice removes)
   - The surfaces themselves: the engine command registry (`crates/engine/`),
     the CLI (`crates/cli/`), the HTTP API (`shbench serve` / the `server`
     module), config (`crates/config/`), `docs/`, and relevant tests
   - Systems `make init` does NOT manage (handle these manually — see step 7):
     release CI (`.github/workflows/`). Note `frontend/`, `Dockerfile`, and the
     `docs/` site ARE pruned by init when their surface/flag is deselected — do
     not hand-delete them.

2. Interview the user briefly. Prefer grouped multi-select questions:
   - Project shape (`--profile`): `cli-only`, `server-only`, or `cli+server`
     (for a custom shape, skip `--profile` and drive `--surfaces`/`--config`)
   - Service surfaces (`--surfaces`): `cli`, `http_api` (and `mcp` once that
     transport exists)
   - Optional extras: frontend (`--frontend`, the optional Bun/React viz app),
     docs site (`--docs`), Dockerfile (`--docker`)

3. Use `make init` as the source of truth. It maps `PROFILE=`, `CONFIG=`, and
   `DRY_RUN=1` to flags and passes everything else through `ARGS=`:
   - Always start with a dry run: `make init PROFILE=<profile> DRY_RUN=1`.
   - Override individual axes via `ARGS`, e.g.
     `make init PROFILE=server-only DRY_RUN=1 ARGS="--surfaces cli,http_api --no-frontend --docker"`.
     Boolean axes use paired flags: `--frontend/--no-frontend`,
     `--docs/--no-docs`, `--docker/--no-docker`.
   - For a custom shape, write a YAML/JSON config and pass `CONFIG=<path>`
     (keys: `profile`, `surfaces`, `frontend`, `docs`, `docker`).
   - Bare `make init` (no headless flags) launches the interactive wizard
     (branding, rename, cli-name, deps, env, hooks).
   - Only run a non-dry pass after the user confirms the resolved plan printed
     by the dry run.

4. Apply dependency implications before pruning (`expand()` enforces these):
   - `frontend` implies the `http_api` surface (the frontend talks to it).
   - `cli+server` implies both the `cli` and `http_api` surfaces.
   - Dropping `http_api` also drops `frontend` (nothing for it to talk to).

5. Let onboarding prune deterministically; do not hand-delete. A non-dry run
   removes files/crates and also rewrites, in the same pass, `Cargo.toml`
   (workspace `members`, dependents' `[dependencies]`, `[[bin]]` targets — via
   `toml_edit`, format-preserving), `Makefile` targets, and `.env.example` keys
   for pruned surfaces. Pruning `http_api` drops the `server` module/routes and
   the `frontend/`; pruning `frontend` drops `frontend/` and its package
   manifests only.

6. Verify the selected shape:
   - CLI: run `shbench --help` and one simple command (e.g. `shbench call ping`).
   - API: start `shbench serve`, check `GET /healthz`, and `GET /api/v1/commands`.
   - Shared changes: run focused `cargo test` first, then `make ci` when scope
     is large.

7. Handle the systems onboarding does not touch, after confirming with the user:
   - `frontend/` branding/content (if kept) almost always needs rebranding.
   - Release workflows (`.github/workflows/`) are not wired into pruning; update
     or remove them to match the kept surfaces. (`frontend/`, `Dockerfile`, and
     the `docs/` site are pruned automatically by their surface/flag — no manual
     deletion needed.)

## Guardrails

- Do not delete the API/server, the frontend, docs, or deploy configs without
  explicit user confirmation.
- Do not push to `main`, force-push, or run destructive git commands. Onboarding
  reads `git remote` only — it never commits or pushes.
- Always dry-run and have the user confirm the plan before any mutating pass.
- Prefer `toml_edit` over regex for any `Cargo.toml` change.
- Keep the optional frontend framed as an example/visualization layer, not core
  template infrastructure.
