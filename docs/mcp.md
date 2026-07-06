# MCP transport (designed-for, not built)

`shbench mcp` is a **stub**. It prints a "not implemented" notice and exits with
`EX_UNAVAILABLE` (69). This document records the intended design so the adapter
can be added later without reshaping `engine`.

## Why it's cheap to add

The typed command registry already exposes everything an MCP server needs. Each
registered `Command` carries its input/output JSON Schemas (via `schemars`) and
per-transport visibility flags (`Expose`). The HTTP API in `serve_http.rs` is
built by looping that same registry; MCP is the same loop with a different
protocol envelope.

```
                       crates/engine (unchanged)
                    ┌──────────────────────────────┐
                    │  CommandRegistry              │
                    │   • schemas()  → [ {name,     │
                    │       input_schema,           │
                    │       output_schema, expose} ]│
                    │   • schema(name)              │
                    │   • call(name, json, ctx)     │
                    └───────────┬──────────────────┘
                                │  (same registry, two transports)
                 ┌──────────────┴───────────────┐
                 ▼                               ▼
        serve_http.rs (built)           mcp.rs adapter (future)
        HTTP  /api/v1/commands/:name    MCP   tools/list  ← schemas()
        POST body = Input               MCP   tools/call  → call(name, args)
        resp = bare Output              tool schema = input_schema
```

## Adapter sketch

1. Add an MCP server dep (e.g. `rmcp`) behind a new `mcp` cargo feature, so it
   is prunable like `cli` / `http-api`.
2. `tools/list`: map `registry.schemas().filter(|s| s.expose.mcp)` to MCP tool
   definitions — `name`, `description`, and `inputSchema` = the command's
   `input_schema`. (The `Expose::mcp` flag already exists to hide CLI-only
   utility commands from the tool surface, mirroring the reference template's
   exclusion set.)
3. `tools/call`: deserialize the tool arguments as the command `Input`, run
   `registry.call(name, args, &ctx)`, and return the bare `Output` as the tool
   result. Errors map from `CommandError::error_code()` to MCP error responses,
   the same mapping `serve_http.rs` uses for HTTP status codes.
4. Mount it in the same process as `shbench serve` (the HTTP router is already
   shaped to nest a `/mcp` sub-router), or run it standalone over stdio for
   local tool clients.

## Cross-cutting concerns

Auth, rate limiting, and logging live in the transport layer — a tower-style
guard for MCP, never in `engine`. The core stays pure, exactly as it does for
the HTTP path.
