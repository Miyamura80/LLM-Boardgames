# shbench – CLI + HTTP API

The `shbench` binary drives the shared `engine` command registry over multiple
transports: the CLI (`call` / `probe` / `doctor` / `run-scenario`) and the axum
HTTP API (`serve`). `init` onboards the template into a real project, `new`
scaffolds a command, and `mcp` is a stub for the future MCP transport.

The `cli` and `http-api` surfaces are cargo features (both on by default), so
`shbench init` can prune one and still leave a compiling binary.

## Build

```bash
cargo build -p shbench
# Binary at target/debug/shbench (or target/release/shbench with --release)
```

## Commands

### doctor

Collect environment facts (OS, kernel, headless detection, proxy vars).

```bash
# Human-readable
shbench doctor

# JSON output
shbench doctor --json

# Write result to file
shbench doctor --json --out /tmp/env.json
```

### call

Invoke a backend command by name with JSON arguments.

```bash
# Ping (prove wiring works)
shbench call ping --json

# Read a file
shbench call read_file --args '{"path": "/etc/hostname"}' --json

# Write a file
shbench call write_file --args '{"path": "/tmp/test.txt", "content": "hello"}' --json

# With artifacts directory
shbench call ping --json --artifacts /tmp/artifacts
```

### probe

Targeted capability checks.

```bash
# Filesystem probe (create/read/write/delete in temp dir)
shbench probe filesystem --json

# Network probe (DNS resolve + HTTPS GET)
shbench probe network --json
```

### run-scenario

Execute a scripted scenario from a YAML file.

```yaml
# scenario.yaml
name: basic smoke test
steps:
  - call: "ping"
    args: {}
    expect_status: "pass"
  - call: "write_file"
    args:
      path: "/tmp/scenario_test.txt"
      content: "written by scenario"
    expect_status: "pass"
  - call: "read_file"
    args:
      path: "/tmp/scenario_test.txt"
    expect_status: "pass"
  - probe: "filesystem"
```

```bash
shbench run-scenario scenario.yaml --json
shbench run-scenario scenario.yaml --artifacts /tmp/artifacts
```

### serve

Start the axum HTTP API. Host/port default from config
(`APP__SERVER__HOST` / `APP__SERVER__PORT`) and can be overridden with flags.

```bash
shbench serve --host 0.0.0.0 --port 8080
```

Routes (versioned under `/api/v1`, auto-derived from the registry):

```
GET  /healthz                      liveness
GET  /api/v1/commands              list commands + JSON Schemas
POST /api/v1/commands/:name        run a command; body = Input, resp = bare Output
POST /api/v1/probe/:target         run a probe (filesystem | network)
GET  /api/v1/doctor                env report
GET  /api/v1/config                sanitized FrontendConfig (never secrets)
```

The `run_id` rides in the `x-run-id` response header; `CommandError`s map to HTTP
status codes with a small `{ "error": { "code", "message" } }` problem body.

### mcp

Stub for the future MCP transport — prints a notice and exits (`EX_UNAVAILABLE`).
See [`docs/mcp.md`](../../docs/mcp.md) for the adapter design.

## Output Contract

The CLI wraps command output in a diagnostic envelope with this stable JSON
schema (the HTTP API returns the **bare `Output`** instead):

```json
{
  "run_id": "uuid",
  "command": "call|probe|doctor|run-scenario",
  "target": "<cmd or probe name>",
  "status": "pass|fail|skip|error",
  "error": { "code": "ERROR_CODE", "message": "..." },
  "timing_ms": { "total": 1234, "steps": { "init": 10, "work": 1200 } },
  "artifacts": [],
  "env_summary": { "os": "linux|macos", "arch": "x86_64|aarch64", "headless": true },
  "data": {}
}
```

Error codes: `INVALID_INPUT`, `UNSUPPORTED`, `UNIMPLEMENTED`, `DEPENDENCY_MISSING`,
`PERMISSION_DENIED`, `NETWORK_ERROR`, `IO_ERROR`, `TIMEOUT`, `EXTERNAL_INTERFERENCE`,
`INTERNAL_ERROR`.

## Artifacts

When `--artifacts <dir>` is provided, the CLI writes:

```
<dir>/<run_id>/
  result.json      # Full result object
  events.jsonl     # JSON Lines log of events
```

## Exit Codes

- `0` -- pass or skip
- `1` -- fail
- `2` -- error
