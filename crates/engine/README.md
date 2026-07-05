# engine – Shared Backend Logic

The transport-agnostic service core of the Rust server template — all real
backend logic. Driven by `shbench` (`crates/cli`) over the CLI and the HTTP API,
and (later) MCP; the same registry serves every transport.

## Design Principles

- **No transport dependency** – the engine never imports CLI, axum, or HTTP
  types, so it can run in any Rust context (CLI, HTTP API, tests, etc.).
- **Trait-based OS access** – filesystem and network operations are behind
  traits (`FilesystemOps`, `NetworkOps`). Callers inject the implementation they
  need (real platform vs. headless stubs).
- **Structured results** – every operation returns a `CommandResult` with a
  stable JSON schema including `run_id`, `status`, `error`, `timing_ms`, and
  `env_summary`.
- **No panics on missing capabilities** – headless environments get `SKIP` or
  `UNSUPPORTED` error codes instead of crashes.

## Modules

| Module | Purpose |
|--------|---------|
| `types` | Output contract: `CommandResult`, `Status`, `ErrorCode`, `EnvSummary`, scenario types |
| `traits` | OS capability traits: `FilesystemOps`, `NetworkOps` |
| `platform` | Real implementations: `StdFilesystem`, `ReqwestNetwork` |
| `context` | `AppContext` – holds trait objects and config; constructors for platform/headless |
| `commands` | `CommandRegistry` with built-in commands: `ping`, `read_file`, `write_file`, … |
| `probes` | Capability probes: `filesystem`, `network` |
| `doctor` | Environment diagnostics (OS, kernel, headless detection, proxy vars) |
| `scenario` | YAML scenario parser and async runner |

## Usage

```rust
use engine::{AppContext, CommandRegistry, Ctx};

// Shared capabilities built once; a lightweight Ctx is built per invocation.
let caps = AppContext::default();
let registry = CommandRegistry::new();
let cx = Ctx::new(&caps);

// `execute` returns the diagnostic CommandResult envelope (used by the CLI);
// `call` returns the bare typed Output (used by the HTTP API).
let result = registry.execute("ping", serde_json::json!({}), &cx).await;
assert_eq!(result.status, engine::Status::Pass);

// Run a probe
let probe_result = engine::probes::run_probe("filesystem", &caps).await;
```

## Adding Commands

Commands implement the typed, async `Command` trait and **self-register at link
time** via `register_command!` — there is no hand-maintained registration list.
The fastest path is `shbench new <name>` (or `make new name=<name>`); see the
`update-backend` skill for the full pattern.

```rust
register_command!(MyCommand); // at the bottom of commands/my_command.rs
```

## OS Traits

Implement custom capability providers by implementing the traits:

```rust
use engine::traits::{FilesystemOps, CapResult, CapError, DirEntry};
use std::path::{Path, PathBuf};

struct ReadOnlyFs;

impl FilesystemOps for ReadOnlyFs {
    fn write_file(&self, _path: &Path, _data: &[u8]) -> CapResult<()> {
        Err(CapError::PermissionDenied("read-only filesystem".into()))
    }
    // ...implement the remaining FilesystemOps methods...
}
```
