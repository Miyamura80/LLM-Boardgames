---
name: update-backend
description: Guide for making changes to the Rust backend of the server template, covering the engine crate, the CLI + HTTP API transports, and testing patterns.
---

# Update Backend Skill

Use this skill whenever you are modifying Rust backend logic — adding commands, probes, traits, or configuration in `crates/engine` or `crates/cli`.

## Architecture

The backend is split into two layers:

| Layer | Path | Role |
|-------|------|------|
| **engine** | `crates/engine/` | All real backend logic. No transport dependency — runs in the CLI, the HTTP API, and tests. |
| **appctl** | `crates/cli/` | The `appctl` binary. Drives `engine` over the CLI (`call`/`probe`/`doctor`/`run-scenario`) and the axum HTTP API (`serve`), gated behind the `cli` / `http-api` cargo features. |

### Design Principles (engine)

- **No transport dependency** — never import CLI, axum, or HTTP types inside `crates/engine`.
- **Trait-based OS access** — filesystem and network go through `FilesystemOps`, `NetworkOps`. Inject real platform or headless stubs via `AppContext`.
- **Structured results** — every operation returns `CommandResult` with `run_id`, `status`, `error`, `timing_ms`, and `env_summary`.
- **No panics on missing capabilities** — headless environments get `SKIP` or `UNSUPPORTED` error codes.

## Code Style (Rust)

- `snake_case` for functions/modules/variables
- `PascalCase` for structs/enums
- Run `cargo fmt` before committing; pass `cargo clippy` with no warnings

## Adding a Backend Command

Commands implement the typed, async `Command` trait (one input struct drives the
CLI args, the HTTP body schema, and the future MCP tool schema) and
**self-register at link time** via `register_command!` — there is no
hand-maintained registration list.

The fastest path is the scaffolder: `appctl new <name>` (or `make new
name=<name>`) generates the file below from `templates/command.rs.tpl` and
inserts the `mod <name>;` line for you. To do it by hand:

1. Drop a new file `crates/engine/src/commands/my_command.rs`:

```rust
use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct MyCommand;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MyCommandInput {
    pub key: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MyCommandOutput {
    pub result: String,
}

#[async_trait]
impl Command for MyCommand {
    type Input = MyCommandInput;
    type Output = MyCommandOutput;

    fn name(&self) -> &'static str { "my_command" }
    fn description(&self) -> &'static str { "One-line description." }
    // Optional: restrict transports, e.g. `Expose::cli_only()` / `Expose::no_mcp()`.

    async fn run(&self, input: MyCommandInput, cx: &Ctx<'_>)
        -> Result<MyCommandOutput, CommandError>
    {
        // `cx.fs()`, `cx.network()` reach the capabilities;
        // `cx.request_id` / `cx.deadline` are request-scoped.
        Ok(MyCommandOutput { result: input.key })
    }
}

register_command!(MyCommand);
```

2. Declare the module in `crates/engine/src/commands/mod.rs` (`mod my_command;`).
   The `register_command!` line does the rest — `CommandRegistry::new()` collects
   it automatically. (`appctl new` inserts this line for you.)

3. No transport change is needed: the HTTP `POST /api/v1/commands/:name` route is
   auto-derived from the registry, and the CLI `call` path dispatches by name.

4. Smoke-test headlessly with `appctl`:

```bash
cargo build -p appctl
appctl call my_command --args '{"key": "value"}' --json
```

- Deserialize failures map to `INVALID_INPUT` automatically. Return typed
  `CommandError` variants (`Unsupported`, `NetworkError`, `Timeout`, …) for
  everything else; `CapError` from a capability converts via `?`.
- API/MCP receive the **bare `Output`**; the CLI/scenario runner wrap it in the
  `CommandResult` envelope. Introspect schemas via `registry.schema(name)`.

## Adding an OS Capability (Trait)

Implement the relevant trait from `crates/engine/src/traits.rs`:

```rust
use engine::traits::{NetworkOps, CapResult, CapError};

struct OfflineNetwork;

#[async_trait::async_trait]
impl NetworkOps for OfflineNetwork {
    async fn dns_resolve(&self, _host: &str) -> CapResult<Vec<String>> {
        Err(CapError::Unsupported("offline".into()))
    }
    async fn https_get(&self, _url: &str, _timeout_ms: u64) -> CapResult<(u16, String)> {
        Err(CapError::Unsupported("offline".into()))
    }
}
```

Inject via `AppContext` — `AppContext::default()` wires the real platform capabilities (used by `appctl`); pass stub implementations to `AppContext::new(fs, network)` to run a command against fakes in tests.

## Configuration

Config lives in its own crate, `crates/config` (crate name `app-config`).
Source of truth: `crates/config/global_config.yaml` (layered with
`production_config.yaml` / `.global_config.yaml` and `APP__`-prefixed env
overrides; `APP_CONFIG_PATH` points a deployed binary at its config file).

```rust
let config = app_config::get_config();
println!("Model: {}", config.default_llm.default_model);
```

`crates/engine` stays config-agnostic — do not import `app-config` there unless a
command genuinely needs config; prefer passing values in via the input struct.

## Testing with appctl

The `appctl` CLI drives `engine` without a running HTTP server:

```bash
# Build
cargo build -p appctl

# Diagnostics
appctl doctor --json

# Call a command
appctl call ping --json
appctl call read_file --args '{"path": "/etc/hostname"}' --json

# Run a capability probe
appctl probe filesystem --json
appctl probe network --json

# Run a YAML scenario
appctl run-scenario scenario.yaml --json
```

Write scenario files for regression tests:

```yaml
name: my feature smoke test
steps:
  - call: "my_command"
    args:
      key: "value"
    expect_status: "pass"
```

## Output Contract

Every command result has this stable JSON schema:

```json
{
  "run_id": "uuid",
  "command": "call|probe|doctor|run-scenario",
  "target": "<cmd or probe name>",
  "status": "pass|fail|skip|error",
  "error": { "code": "ERROR_CODE", "message": "..." },
  "timing_ms": { "total": 1234 },
  "env_summary": { "os": "linux|macos", "arch": "x86_64|aarch64", "headless": true },
  "data": {}
}
```

Error codes: `INVALID_INPUT`, `UNSUPPORTED`, `UNIMPLEMENTED`, `DEPENDENCY_MISSING`,
`PERMISSION_DENIED`, `NETWORK_ERROR`, `IO_ERROR`, `TIMEOUT`, `EXTERNAL_INTERFERENCE`, `INTERNAL_ERROR`.

## Checklist Before Committing

- [ ] `cargo fmt` applied
- [ ] `cargo clippy` passes (no warnings)
- [ ] `cargo test` passes
- [ ] New command smoke-tested with `appctl call <cmd> --json`
- [ ] `engine` crate has no transport imports (no CLI/axum/HTTP types)
