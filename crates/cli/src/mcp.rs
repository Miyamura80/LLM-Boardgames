//! `appctl mcp` — placeholder for the future MCP transport.
//!
//! MCP is **designed-for but not built** this iteration (see the PRD §8 and
//! `docs/mcp.md`). It needs nothing new from `engine`: the typed command
//! registry already exposes `registry.schemas()` / `registry.schema(name)`,
//! which is exactly what an adapter maps `tools/list` → schemas and
//! `tools/call` → `registry.call` onto. This stub keeps the subcommand surface
//! stable until the adapter lands, and is intentionally ungated (like `init`
//! and `new`) so it survives surface pruning.

/// Print the "not implemented" notice and exit with `EX_UNAVAILABLE` (69).
pub fn run() -> ! {
    eprintln!(
        "appctl mcp is not implemented yet.\n\
         \n\
         MCP is a planned transport that will expose the same engine command\n\
         registry as MCP tools:\n\
           tools/list  ← registry schemas  (registry.schemas())\n\
           tools/call  → registry dispatch (registry.call)\n\
         mounted in-process alongside `appctl serve`, mirroring the HTTP API.\n\
         See docs/mcp.md for the adapter design.\n\
         \n\
         For now use `appctl serve` (HTTP API) or `appctl call <cmd>` (CLI)."
    );
    std::process::exit(69);
}
