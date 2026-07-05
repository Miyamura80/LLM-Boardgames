# PRD: Tauri → Rust Server + CLI/API Template

## 1. Overview

Convert this Tauri desktop template into a **Rust application-server template**
with a unified CLI & HTTP API interface over a single shared core, plus an
**optional** Bun/React frontend for visualization. Inspired by
[`Miyamura80/MCP-Template`](https://github.com/Miyamura80/MCP-Template):
*write the business logic + its typed I/O contract once*, and **auto-derive the
API (and, later, MCP) from that contract** — with an MCP transport designed-for
but **not built** in this iteration.

### How the reference actually works (verified against source)

MCP-Template does NOT auto-generate everything from one registry. There are two
distinct mechanisms, and the split is intentional:

- A `@service(name, description, input_model, output_model)` decorator registers
  a `ServiceEntry{func, InputModel, OutputModel}` in a list. `discover_services()`
  imports every `services.*` module so the decorators fire.
- **API and MCP are auto-generated** from that registry: the API loops the
  registry and mounts one `POST /api/v1/services/{name}` per service
  (`input_model` = request body, `output_model` = response); MCP loops the same
  registry and synthesizes a tool whose JSON schema comes from `input_model`.
- **The CLI is hand-written per command** (`src/cli/commands/*.py`, its own
  `discover_commands()` scan). Each command is a bespoke Typer function with its
  own flags, `--dry-run`/`--verbose`, interactive fallback, and rendering — it
  *imports and calls* the service. The CLI is NOT derived from the registry.
- A per-service **exclusion set** hides CLI-only services (e.g. `greet`,
  `doctor`, `config_*`) from the MCP tool surface. Transport visibility is
  per-service.
- **Cross-cutting concerns live in the transport wrappers, not the service**:
  the API wrapper enforces auth scopes + daily quota and injects `user_id`; MCP
  has an analogous guard. Services stay pure. API + MCP run in one process
  (MCP mounts at `/mcp`).

**Implication for this template:** the typed contract buys auto-derived API/MCP
for free, but CLI ergonomics are worth hand-authoring. We mirror that split.

All Tauri/desktop scaffolding is removed.

## 2. Goals / Non-Goals

### Goals
- One service core (`engine`) exposed through multiple transports.
- A typed command contract (schemars JSON Schema) shared across CLI + API (+ future MCP).
- An `axum`-based HTTP API.
- A single CLI binary with subcommands (`call`, `serve`, `doctor`, `probe`, `run-scenario`).
- Optional React/Vite frontend that talks to the HTTP API via `fetch`.
- Config relocated out of the (deleted) Tauri crate into a standalone crate.
- The core compiles and tests pass **standalone at every phase** (no broken intermediate states).

### Non-Goals (this iteration)
- Building the MCP server (designed-for only; left as a documented stub).
- Authentication/authorization beyond a pluggable middleware seam.
- Database / persistence layer.
- Desktop features (tray, deep links, auto-updater, native bundling).

## 3. Target Architecture

```
        ┌──────────────────────────────────────────────────────────┐
        │  TRANSPORTS  (crates/cli — one binary, subcommands)        │
        │                                                            │
        │   shbench call <cmd> --args '{...}'   one-shot JSON I/O     │
        │   shbench serve --http :8080          axum HTTP API         │
        │   shbench doctor | probe | run-scenario                     │
        │   shbench mcp                         (LATER — stub)        │
        └───────────────┬─────────────────────────┬──────────────────┘
                        │                          │
          optional ─────┘                          │  same registry
          bun/React frontend ──HTTP/fetch──▶ serve  │  + typed contract
                                                    │
        ┌───────────────────────────────────────────▼───────────────┐
        │  crates/engine  — the service core (no transport deps)      │
        │                                                             │
        │    Command trait:  Input: JsonSchema + Deserialize          │
        │                    Output: JsonSchema + Serialize           │
        │                    fn run(input, &AppContext) -> Result     │
        │    CommandRegistry: name → boxed typed command              │
        │                     + schema(name) introspection            │
        │    AppContext:      fs / network / clipboard (traits)       │
        │    types:           CommandResult (stable JSON contract)    │
        └───────────────────────────┬─────────────────────────────────┘
                                     │
        ┌────────────────────────────▼────────────────────────────────┐
        │  crates/config  — AppConfig / FrontendConfig (moved here)     │
        │                   YAML + APP__ env overrides + sanitizer       │
        └────────────────────────────────────────────────────────────────┘
```

### 3.1 Crate layout (after)

```
Cargo.toml                 # workspace: engine, config, cli
crates/
  engine/                  # service core + typed Command registry
  config/                  # AppConfig/FrontendConfig + loader (moved from src-tauri)
  cli/                     # `shbench` binary: call / serve / doctor / probe / scenario / (mcp stub)
    src/
      main.rs              # clap entrypoint
      serve_http.rs        # axum app (replaces serve.rs UDS daemon)
      ...
frontend/   (optional)     # React/Vite app, fetch()-based API client (moved from src/)
docs/                      # docs site + this PRD
```

`src-tauri/`, `src/` (Tauri frontend), and `crates/onboard/` (orphan, no
Cargo.toml) are removed.

## 4. The Typed Command Contract (core decision)

Replace the untyped `fn(Value, &AppContext) -> Result<Value, CommandError>`
with a typed trait so every transport gets schemas for free.

```
#[async_trait]
trait Command {
    // ONE struct drives the API request body and the MCP schema. CLI arg-parsing
    // is layered on separately so `engine` stays free of `clap` (see the note below).
    type Input:  DeserializeOwned + JsonSchema + Send;
    type Output: Serialize + JsonSchema + Send;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn expose(&self) -> Expose { Expose::all() }   // per-transport visibility
    async fn run(&self, input: Self::Input, cx: &Ctx)
        -> Result<Self::Output, CommandError>;
}
```

- **Async** (`async-trait`): the registry, probes, and any network/DB/LLM work
  are async; the HTTP server is async end-to-end. The current sync
  `fn(Value,&AppContext)->Result` handler type is replaced.
- **One input struct, three consumers**: `#[derive(clap::Args, Deserialize,
  JsonSchema)]` on the input type means CLI flags, the API body schema, and the
  MCP tool schema all come from a single definition (no drift). Commands needing
  bespoke CLI UX can still hand-write a clap command and call the service.
  - **Implementation note (Phase 2):** the trait bounds `Input` on
    `DeserializeOwned + JsonSchema + Send` only — it deliberately does **not**
    require `clap::Args`, so `engine` stays free of any CLI dependency (matching
    §4.1's "CLI is hand-written, not derived from the registry"). Input structs
    add `#[derive(clap::Args)]` opt-in when a hand-written/generated CLI
    subcommand wants to reuse them; that lands with the CLI scaffolding
    (Phase 5). The current CLI `call` path feeds JSON straight through
    `Deserialize`, so nothing needs `clap::Args` yet.

- The registry stores **type-erased** entries (an object-safe inner trait that
  takes/returns `serde_json::Value`, with deserialize→run→serialize wrapped
  inside) **plus** the input/output `schemars::schema_for!` outputs. This is the
  Rust analog of MCP-Template's `ServiceEntry{func, InputModel, OutputModel}`.
- Each entry carries **per-transport visibility** flags (mirrors the reference's
  MCP exclusion set), e.g. `expose: { api: true, mcp: false }`, so CLI-only
  utility commands don't leak into the API/MCP tool surface.
- **Registration via `inventory`/`linkme`** (not a hand-maintained `register()`
  list): a command self-registers at link time, giving the "drop a file, it's
  wired" UX that makes `shbench new` (§8c) a pure file generator. Decide this here
  in Phase 2 since it shapes the registry type.
- New introspection: `registry.schema(name) -> { input_schema, output_schema }`,
  consumed by the API (`GET /commands`, OpenAPI) and the future MCP `tools/list`.

### Output contract: bare output for API/MCP, envelope for CLI

The HTTP/MCP body is the **bare `Output`** type (clean, idiomatic schemas the
LLM/OpenAPI can consume directly). The rich `CommandResult` envelope (run_id,
status, timing, error, env_summary) is retained for the **CLI and scenario
runner**, where the diagnostics are the point. `run_id`/timing ride along on the
HTTP path as response headers (e.g. `x-run-id`), not in the body.

```
CLI  : shbench call greet … → CommandResult { run_id, status, timing, data:{…}, … }
HTTP : POST /api/v1/commands/greet → 200 { "message": "...", "times": 1 }   (bare Output)
       errors → HTTP status (from CommandError::error_code()) + problem body
```

### Per-request context (`Ctx`), no identity yet

Drop the process-global context singleton. Capabilities (fs/net) and config are
shared (`Arc`); a lightweight **`Ctx` is constructed per request/invocation**
carrying `request_id` and a deadline. This is the seam for future auth — but we
add **no `user_id`/identity field now** (auth is explicitly undecided). Adding it
later is a field on `Ctx` + middleware, not a signature break.

```
   shared (Arc):   capabilities (fs/net) + config        ← built once
   per request:    Ctx { request_id, deadline }          ← built per call, passed to run()
```

### 4.1 Transport split (mirrors the reference)

- **API + MCP are auto-derived** by looping the registry — no per-command
  boilerplate. Adding a `Command` makes it callable over HTTP (and MCP) for free.
- **CLI subcommands are hand-written** (clap), importing and calling the same
  command/service, so each gets first-class flags and output. The CLI is not
  generated from the registry — only the *core logic + schema* is shared.
- **Cross-cutting concerns (auth, rate limit, logging) go in the transport
  layer** (tower middleware for HTTP), never in `engine`. The core stays pure.

New deps: `schemars` (engine), `clap` already present.

## 5. HTTP API (axum)

Replace the Unix-socket daemon (`crates/cli/src/serve.rs`) with axum routes
that loop the registry (auto-derived, mirroring the reference). **Versioned from
day one** under `/api/v1`:

```
GET  /healthz                      → liveness (unversioned)
GET  /api/v1/commands              → list + JSON Schemas (introspection)
POST /api/v1/commands/:name        → run command; body = Input JSON; resp = bare Output
POST /api/v1/probe/:target         → run probe (network | filesystem)
GET  /api/v1/doctor                → env report
       (future)  /mcp              → mounted MCP sub-router (rmcp), same process
```

- Response body is the **bare `Output`**; `run_id`/timing in `x-run-id` etc.
- tower middleware: CORS (frontend), tracing, request-id, timeout. This is the
  seam where auth/rate-limit slot in later — never in `engine`.
- Error mapping: `CommandError::error_code()` → HTTP status (InvalidInput→400,
  PermissionDenied→403, NetworkError→502, IoError→500, …) + a small problem body.
- `serve` is structured so a `/mcp` sub-router can be mounted later without
  reshaping the app (reference mounts FastMCP on FastAPI the same way).
- Bind host/port from config; graceful shutdown on SIGTERM/SIGINT (tokio signal).
- Retire `DaemonRequest`/`DaemonResponse` (UDS daemon) unless a non-HTTP
  transport is still wanted.

New deps (cli): `axum`, `tower`, `tower-http` (cors, trace), `async-trait`.

### 5.1 API integration tests

Add an HTTP-level test layer using axum's `tower::ServiceExt::oneshot` (in-process,
no socket): assert status codes, the bare-output schema, error mapping, and that
no secret config field ever serializes over the wire. The existing `run-scenario`
YAML harness can additionally be pointed at the HTTP API for end-to-end checks.

## 6. Config relocation

Move `src-tauri/src/global_config.rs` + `global_config.yaml` into
`crates/config`:
- Keep the `AppConfig` (full, with secret API keys) vs `FrontendConfig`
  (sanitized) split — the sanitizer is reused for any payload the API exposes
  to the frontend. **This is now a security boundary**: it used to feed a
  bundled webview over IPC; it will now serve over HTTP to a browser. Add a test
  asserting no secret field ever serializes, and exercise it in 5.1.
- Fix the loader's path logic: it currently falls back to a hard-coded
  `src-tauri/` prefix. Re-anchor to the config crate's `CARGO_MANIFEST_DIR` /
  a configurable base path / `APP_CONFIG_PATH` env.
- Port the existing config tests (env override precedence, type coercion,
  sanitization) verbatim into the new crate.

## 7. Frontend (optional, HTTP client)

- Move `src/` → `frontend/`. Remove `@tauri-apps/api`, `-plugin-opener`,
  `-plugin-updater`, `-plugin-process`; delete `UpdateNotification` and the
  tauri update hook.
- Replace `invoke('engine_call', …)` with `fetch('/api/v1/commands/:name', …)`;
  add a tiny typed API client. `useConfig()` calls `GET` a config endpoint
  instead of the tauri `get_app_config` command.
- Vite dev server proxies `/api` → `shbench serve`. Frontend is fully optional:
  the template is useful headless with just the CLI + API.

## 8. MCP (designed-for, not built)

No MCP server this iteration. The typed registry + `schema(name)` introspection
is precisely what an MCP transport needs:
- Future `shbench mcp` adapter maps `tools/list` → registry schemas and
  `tools/call` → `registry.execute`. Leave a stub subcommand returning
  "unimplemented" and a `docs/` note describing the adapter.

## 8b. Example command surface

Swap the desktop-flavored examples for server-relevant ones so the template's
sample commands are coherent headless:
- Drop `clipboard` probe and the `emit` desktop-event command (always
  Unsupported on a server).
- Keep `ping`, `read_file`, `write_file`, `list_dir`, `system_info`, `doctor`,
  and the `network`/`filesystem` probes.
- Add an `http_request` command (already on the repo TODO) as the canonical
  async example that exercises the `Ctx`, the typed contract, and a real await.

## 8c. Project onboarding + command scaffolding

Verified from mcp-template source: it has **two unrelated systems**, and we
mirror the split. Today's `make init` is only a thin Tauri rename — we replace
it with the richer model.

```
                     Secret-Hitler-Evals repo
                            │
      ┌──────────────────────┴───────────────────────┐
   PROJECT ONBOARDING                          COMMAND SCAFFOLDING
   (one-time, mutates repo)                    (recurring, adds files)
      │                                               │
  make init ──► shbench init                   shbench new <name>
      │  (wizard | --profile/--config/--dry-run)      │  templates/command.rs.tpl
   rename · brand · prune surfaces ·          ──► generates an engine Command
   .env setup · prek hooks                        (+ optional CLI subcommand)
```

### A. Project onboarding — `make init` → `shbench init`

A Rust onboarding subcommand mirroring mcp-template's `init/onboard.py`, living
in `crates/cli/src/init/`:

```
crates/cli/src/init/
  config.rs   profile/surface enums + Config + expand()   ← source of truth
  wizard.rs   dialoguer interactive multi-step flow
  rename.rs   walkdir bulk str::replace of sentinels
  prune.rs    delete crate dirs + toml_edit Cargo.toml/workspace rewrites
  env.rs      .env.example → .env (grouped, masked secrets via dialoguer)
  plan.rs     dry-run plan (comfy-table), printed before any mutation
```

Adopt the reference's proven patterns:
- **Profiles + per-axis overrides.** For this template: `cli-only`,
  `server-only`, `cli+server` (± optional `frontend`). `--profile`, `--config
  <yaml>`, `ARGS=` overrides, all behind one `Config` with an `expand()` that
  encodes implications (e.g. `frontend ⇒ server`).
- **Dry-run first, always.** `make init … DRY_RUN=1` prints the plan; mutation
  is gated. The onboarding skill mandates a dry run before applying.
- **Idempotent.** Rename/prune self-detect completion and no-op; deletes are
  existence-guarded.
- **`toml_edit`, not regex, for `Cargo.toml`.** Pruning a surface = remove the
  crate dir + drop it from `[workspace].members` and dependents'
  `[dependencies]` — format-preserving and robust (the reference uses brittle
  regex on `pyproject.toml`; we do better).
- **Rename sentinels** (`secret-hitler-evals`/`shbench`/`myorg`) replaced across an
  extension allowlist via `walkdir`, skipping `.git`/`target`/`node_modules`;
  GitHub owner/repo auto-detected from `git remote`. Read-only on git — never
  commit/push.

`make init` wraps it:
```make
init:
	cargo run -p shbench -- init $(if $(PROFILE),--profile $(PROFILE),) \
	  $(if $(CONFIG),--config $(CONFIG),) $(if $(DRY_RUN),--dry-run,) $(ARGS)
```

### B. Command scaffolding — `shbench new <name>`

A `string`-substitution generator over `templates/command.rs.tpl` (no
cookiecutter/Jinja needed) that creates a new **engine `Command`** (input/output
structs + impl) and optionally a CLI subcommand wrapper.

- **Auto-registration:** clap/Rust have no runtime module discovery like
  Python's. To keep the "drop a file, it's registered" UX, register engine
  commands with **`inventory`** (or `linkme`) so a generated command
  self-registers at link time — no hand-editing a `mod.rs` registration list.
  This is a small but high-value decision for the registry design (Phase 2).

### C. Onboarding skill

Port `.agents/skills/onboarding/SKILL.md` nearly verbatim: inspect → interview →
dry-run → confirm → apply → verify → handle-untouched-systems. Swap `make
onboard`→`make init`, `pyproject.toml`→`Cargo.toml`, verify via `cargo
build`/`cargo test`/`shbench --help` + `/healthz`. Keep the skill pointing at one
declared source-of-truth file (`crates/cli/src/init/config.rs`).

> Note: `inventory`-based auto-registration (B) feeds back into Phase 2 — if we
> want it, the typed `Command` registry should collect entries via `inventory`
> rather than a hand-maintained `register()` list.

### D. Implementation notes (Phase 5)

- **Surface pruning is cargo-feature based, not source surgery.** The two Rust
  surfaces are gated behind `shbench` crate features `cli` (diagnostic
  subcommands) and `http-api` (axum `serve` + tower stack), both in `default`.
  Pruning a surface drops its feature from the `default` list (via `toml_edit`,
  format-preserving) and deletes the now-unreferenced files (e.g.
  `serve_http.rs`). Because the code is `#[cfg(...)]`-gated, every combination
  compiles cleanly under `clippy -D warnings` — no dead code, no fragile match
  editing. `Init`/`New` stay ungated so onboarding/scaffolding always work.
- **Prune scope this iteration:** the `cli`/`http-api` surface features,
  `frontend` (`src/`, `index.html`, `vite.config.ts`, package.json frontend
  deps/scripts), `docs` (`docs/` + package.json workspace entry), and `docker`
  (`Dockerfile`, `.dockerignore`). `expand()` encodes the implications
  (`frontend ⇒ http_api`; dropping `http_api` drops `frontend`+`docker`).
- **`command.rs.tpl` is embedded** via `include_str!`, so `shbench new` works
  from any cwd. It writes `crates/engine/src/commands/<name>.rs` and inserts a
  sorted `mod <name>;` line — the only wiring `inventory` can't do at link time.
- **`.env` bootstrap** copies `.env.example → .env` (existence-guarded);
  interactive per-secret masking (reference's `env.rs`) is left for a later pass.
- Every mutator (rename/prune/env) is idempotent and dry-run-first; the wizard
  gates any real apply behind a confirmation.

## 9. Teardown checklist (Tauri/desktop removal)

- Delete `src-tauri/` (lib.rs, main.rs, logging.rs, global_config, capabilities,
  icons, tauri.conf.json, build.rs). **First relocate `asset_gen.rs`** — it is
  kept (see below), so move it out before deleting the crate.
- Remove `src-tauri` from workspace members; add `crates/config`.
- `package.json`: drop `@tauri-apps/*`, `tauri` script; rename app; move to
  `frontend/`.
- `Makefile`: replace `tauri-dev`/`tauri-build` with `run` (`cargo run -p cli --
  serve`) / `cargo build`; fix `test` to `cargo test --workspace` (currently
  `cd src-tauri && cargo test`).
- CI: `rust_checks.yaml` — drop GTK/WebKit apt deps; `build_verification.yaml`
  — replace `tauri build` with `cargo build --workspace`; **replace `release.yml`
  with `cargo-dist`** (§13) for cross-platform binary releases + installers.
- **Keep asset-gen**, but relocate it out of `src-tauri` into its own crate
  (e.g. `crates/assetgen`, a `[[bin]]`). Keep `make banner`/`logo` (still needs
  `APP__GEMINI_API_KEY`). Repoint output paths (logos → `docs/public/`, banner →
  `media/`) since they no longer serve desktop icons.
- **Keep the `docs/` Next.js site** and its Jules translation workflow — it's
  backend-independent. Reframe its content from Tauri to server/CLI.
- Add a **`Dockerfile`** for the server (§13).
- Clean orphan `crates/onboard/` (delete, or promote to a real crate).
- Docs: rewrite `README.md`, `CLAUDE.md` (Tauri → server framing), update
  `update-backend`/`code-quality` skills, archive the old `docs/PRD.md`.

## 10. Phased plan (each phase leaves the tree green)

```
Phase 0  Baseline:  cargo test --workspace passes; branch ready.
Phase 1  Config:    extract crates/config from src-tauri; port tests +
                    sanitizer security test; src-tauri temporarily depends on
                    it. Tree green.
Phase 2  Contract:  async Command trait + schemars; one input struct derives
                    clap::Args + Deserialize + JsonSchema; per-request Ctx (no
                    identity); per-transport expose flags; registry gains
                    schema(). Port example commands. Tree green.
Phase 3  HTTP API:  axum `serve` reusing registry; /api/v1 routes; bare-output
                    body + run_id header; CORS/trace/request-id/timeout;
                    graceful shutdown. Add 5.1 integration tests. Retire UDS
                    daemon. Tree green.
Phase 4  Teardown:  relocate asset-gen → crates/assetgen; delete src-tauri +
                    tauri frontend deps; swap example commands (8b); fix
                    workspace, Makefile, CI; add cargo-dist + Dockerfile (§13);
                    clean orphan crate. Pure server+CLI. Green.
Phase 5  Scaffold:  rebuild `make init` + onboarding (8c, per subagent report).
Phase 6  Frontend:  move src→frontend, convert invoke()→fetch(); optional.
Phase 7  MCP stub + docs/README/CLAUDE rewrite; final CI + prek pass.
```

Rationale for order: config and the typed contract must exist **before** Tauri
is deleted, so the core always compiles standalone.

## 11. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| Config loader path assumptions break when src-tauri is gone | Re-anchor to config-crate manifest dir + `APP_CONFIG_PATH`; do it in Phase 1 while src-tauri still builds. |
| Typed-registry type erasure is fiddly in Rust | Object-safe inner trait doing Value↔typed conversion; commands implement the typed trait only. |
| Frontend rewrite scope creep | Frontend is optional and last; ship CLI+API value before touching it. |
| Loss of desktop features later regretted | Documented explicitly as a non-goal; Tauri layer was thin and re-addable atop `engine`. |
| `clipboard`/`emit` commands meaningless server-side | Swap for server-relevant examples; add async `http_request` (see 8b). |
| `async-trait` + type erasure interacts awkwardly | Inner object-safe trait is `async` too; box futures at the erasure boundary. Validated by Phase 2 before any transport depends on it. |

## 12. Resolved decisions

All prior open questions are now decided (see §13 for the packaging detail):

- **Release:** `cargo-dist` — cross-platform binaries + installers, replacing
  `release.yml`.
- **asset-gen:** kept, relocated out of `src-tauri` into its own crate.
- **docs site:** kept (reframed Tauri → server); Jules translation workflow stays.
- **Dockerfile:** added, for the server.
- **Frontend:** convert the existing React/Vite app to a `fetch`-based `/api/v1`
  client (Phase 6) — not slimmed, not removed.
- **Auth/identity:** deliberately deferred — `Ctx` carries no `user_id` yet; the
  middleware seam is left open (see §4.1).

## 13. Packaging & deployment

- **`cargo-dist`** (`dist-workspace.toml` / `cargo dist init`) generates the
  release CI: per-OS binary builds (linux/macos/windows), shell + PowerShell
  installers, and GitHub Release artifacts. Replaces the Tauri `release.yml`.
  The released artifact is the `shbench` binary (CLI + `serve`).
  - **Implementation note (Phase 4):** `dist-workspace.toml` is committed as the
    cargo-dist source of truth. The generated pipeline could not be produced in
    the build container (`dist init` unavailable), so `release.yml` currently
    ships as a functional cross-platform `cargo build` → GitHub Release baseline;
    running `dist init && dist generate ci` regenerates the canonical
    installer-producing workflow from the config.
  - **Frontend deps (Phase 4):** the runtime `@tauri-apps/*` packages stay in
    `package.json` for now because the React `src/` still imports them; they are
    removed together with the `invoke()`→`fetch()` conversion in Phase 6. Only
    the `tauri` npm script and `@tauri-apps/cli` were dropped here.
- **`Dockerfile`** — multi-stage: `cargo build --release -p shbench` in a builder
  stage, copy the binary into a slim runtime base (distroless/debian-slim),
  `EXPOSE` the configured port, `ENTRYPOINT ["shbench", "serve"]`. Host/port and
  config via env (`APP__…`, `APP_CONFIG_PATH`). `.dockerignore` excludes
  `target/`, `node_modules/`, `frontend/`.
- Both are **onboarding-prunable** and land in the surface config so
  `server-only` vs `cli-only` projects keep only what applies.
```
