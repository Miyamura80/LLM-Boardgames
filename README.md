# Rust-Template

<p align="center">
  <img src="media/banner.png" alt="banner" width="400">
</p>

<p align="center">
<b>agent-ready Rust server + CLI template</b>
</p>

<p align="center">
  <a href="#key-features">Key Features</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#configuration">Configuration</a> •
  <a href="#agent-skills">Agent Skills</a> •
  <a href="#credits">Credits</a>
</p>

<p align="center">
  <img alt="Rust Version" src="https://img.shields.io/badge/rust-1.75%2B-blue?logo=rust">
  <img alt="GitHub repo size" src="https://img.shields.io/github/repo-size/Miyamura80/Rust-Template">
  <img alt="GitHub Actions Workflow Status" src="https://img.shields.io/github/actions/workflow/status/Miyamura80/Rust-Template/rust_checks.yaml?branch=main">
</p>

---

## Key Features

A Rust application-server template: **write your business logic once as a typed
`Command`, and expose it over multiple transports** — a CLI, an HTTP API, and
(later) MCP — all from one shared core. An optional React/Vite frontend talks to
the API over `fetch`.

| Feature | Tech Stack |
|---------|:----------:|
| **Core** | `engine` crate — typed async `Command` registry (no transport deps) |
| **CLI + API** | `appctl` binary — `call` / `serve` / `doctor` / `probe` / `run-scenario` |
| **HTTP API** | `axum` + `tower` (CORS, tracing, timeout, request-id) |
| **Contract** | `schemars` JSON Schema shared across CLI, API, and future MCP |
| **Config** | `app-config` crate (YAML + `APP__` env overrides + sanitizer) |
| **Frontend** (optional) | React + TypeScript + Vite, `fetch`-based API client |
| **Logging** | `tracing` + redaction layer |
| **Packaging** | `cargo-dist` (binaries + installers) and a server `Dockerfile` |
| **Package Manager** | Bun |
| **Formatting** | Biome + `cargo fmt` |

## Architecture

```
        ┌────────────────────────────────────────────────────────────┐
        │  TRANSPORTS  (crates/cli — one binary `appctl`, subcommands) │
        │                                                              │
        │   appctl call <cmd> --args '{...}'   one-shot JSON I/O       │
        │   appctl serve --port 8080           axum HTTP API           │
        │   appctl doctor | probe | run-scenario                       │
        │   appctl mcp                          (stub — see docs/mcp.md)│
        └───────────────┬─────────────────────────┬───────────────────┘
                        │                          │
        optional bun/React frontend               │  same registry
        ────────── HTTP/fetch ─────────▶ serve ────┤  + typed contract
                                                   │
        ┌──────────────────────────────────────────▼───────────────────┐
        │  crates/engine  — the service core (no transport deps)         │
        │    Command trait:  Input: JsonSchema + Deserialize             │
        │                    Output: JsonSchema + Serialize              │
        │    CommandRegistry (inventory self-registration) + schema()    │
        │    Ctx (per-request): fs / network capabilities, request_id    │
        └───────────────────────────┬───────────────────────────────────┘
                                     │
        ┌────────────────────────────▼──────────────────────────────────┐
        │  crates/config (app-config) — AppConfig / FrontendConfig        │
        │                 YAML + APP__ env overrides + secret sanitizer   │
        └─────────────────────────────────────────────────────────────────┘
```

- `crates/engine/` — all real logic; a typed, async `Command` registry with
  self-registration (`inventory`). No CLI/HTTP dependency.
- `crates/cli/` — the `appctl` binary. The `cli` and `http-api` surfaces are
  cargo features (both on by default) so `appctl init` can prune one.
- `crates/config/` — `AppConfig` (with secrets) vs the sanitized
  `FrontendConfig` served over HTTP. The sanitizer is a security boundary.
- `crates/assetgen/` — `asset-gen` binary for `make banner` / `make logo`.
- `frontend/` — optional React/Vite visualization app (`fetch` API client).
- `docs/` — Next.js docs site.

## Quick Start

```bash
# 1. Onboard the template into a real project (dry-run first, then apply)
make init PROFILE=cli+server DRY_RUN=1
make init PROFILE=cli+server

# 2. Build + test the workspace
cargo build --workspace
cargo test --workspace

# 3. Run the HTTP API
make run                    # = appctl serve   (GET /healthz, /api/v1/commands)

# 4. Call a command headlessly
cargo run -p appctl -- call ping --json
cargo run -p appctl -- call read_file --args '{"path": "/etc/hostname"}' --json

# 5. (optional) Run the frontend against the API
bun install
make dev                    # Vite dev server; /api is proxied to appctl serve
```

Scaffold a new command with `make new name=fetch_url` (or `appctl new
fetch_url`) — it self-registers, so it's immediately callable over the CLI and
the API.

## Asset Generation

- `make logo` / `make banner` regenerate branding assets via the Rust
  `asset-gen` CLI (requires `APP__GEMINI_API_KEY`, set via `.env`).
- Logos/icons land under `docs/public/`, the banner under `media/banner.png`.

## Configuration

Configuration is handled in Rust and exposed to the frontend over HTTP.

- **Rust**: `app_config::get_config()` (full) / `app_config::get_frontend_config()` (sanitized).
- **Frontend**: `useConfig()` hook → `GET /api/v1/config` (never carries secrets).

### Environment Variables
Prefix variables with `APP__` to override YAML settings (e.g.,
`APP__MODEL_NAME=gpt-4`, `APP__SERVER__PORT=9090`). Point a deployed binary at
its config file with `APP_CONFIG_PATH`.

## Agent Skills

Claude Code skills live in `.claude/skills/`. Invoke them with `/skill-name`.

| Skill | Description |
|-------|-------------|
| `/update-backend` | Guide for Rust backend changes — engine commands, traits, CLI/API, testing |
| `/onboarding` | Turn this template into a real project (interview → dry-run → prune) |
| `/code-quality` | Run formatting and linting checks (Biome + Clippy) |
| `/prd` | Generate a Product Requirements Document for a new feature |
| `/ralph` | Convert a PRD to `prd.json` for the Ralph autonomous agent |
| `/cleanup` | Git branch hygiene — delete merged branches, prune stale refs, sync deps |

## Credits

This software uses the following tools:
- [axum](https://github.com/tokio-rs/axum)
- [Bun](https://bun.sh/)
- [Biome](https://biomejs.dev/)
- [Rust](https://www.rust-lang.org/)

## About the Core Contributors

<a href="https://github.com/Miyamura80/Rust-Template/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=Miyamura80/Rust-Template" />
</a>

Made with [contrib.rocks](https://contrib.rocks).
