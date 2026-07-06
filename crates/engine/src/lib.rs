//! Engine crate – the shared game engine and eval core for the Secret Hitler
//! LLM-agent eval harness.
//!
//! This crate holds the engine-authoritative game state, eval logic, and backend
//! integrations behind traits. It has NO transport dependency (no CLI, axum, or
//! HTTP types), so the same commands run over the CLI, the HTTP API, and (later)
//! MCP.

pub mod commands;
pub mod context;
pub mod doctor;
mod env;
pub mod llm;
pub mod platform;
pub mod probes;
pub mod scenario;
pub mod secret_hitler;
pub mod traits;
pub mod types;

// Re-exports for convenience
pub use commands::{Command, CommandError, CommandRegistry, CommandSchema, ErasedCommand, Expose};
pub use context::{AppContext, Ctx};
pub use types::{CommandResult, ErrorCode, ErrorInfo, Status};
