//! The typed command contract and registry.
//!
//! A [`Command`] is the unit of backend logic. It declares a typed `Input`
//! (deserialized from the request body / CLI args) and a typed `Output`
//! (serialized back to the caller). Both derive [`schemars::JsonSchema`], so
//! every transport gets a JSON Schema for free — the HTTP `GET /commands`
//! introspection endpoint and the future MCP `tools/list` both read it.
//!
//! Commands **self-register** at link time via [`inventory`]: dropping a new
//! command file that ends in `register_command!(MyCommand);` makes it appear in
//! [`CommandRegistry::new`] with no hand-edited registration list.
//!
//! ```ignore
//! #[derive(Default)]
//! pub struct Ping;
//!
//! #[derive(Deserialize, JsonSchema)]
//! pub struct PingInput {}
//!
//! #[derive(Serialize, JsonSchema)]
//! pub struct PingOutput { pub pong: bool }
//!
//! #[async_trait]
//! impl Command for Ping {
//!     type Input = PingInput;
//!     type Output = PingOutput;
//!     fn name(&self) -> &'static str { "ping" }
//!     async fn run(&self, _in: PingInput, _cx: &Ctx<'_>) -> Result<PingOutput, CommandError> {
//!         Ok(PingOutput { pong: true })
//!     }
//! }
//! register_command!(Ping);
//! ```

use crate::context::Ctx;
use crate::types::*;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Instant;

mod catan_game;
mod catan_match;
mod catan_report;
mod codenames_game;
mod codenames_match;
mod codenames_match_spec;
mod codenames_report;
mod http_request;
mod list_dir;
mod ping;
mod read_file;
mod sh_game;
mod sh_match;
mod sh_report;
mod system_info;
mod write_file;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("timeout")]
    Timeout,
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("unimplemented: {0}")]
    Unimplemented(String),
    #[error("{0}")]
    Other(String),
}

impl CommandError {
    pub fn error_code(&self) -> ErrorCode {
        match self {
            CommandError::InvalidInput(_) => ErrorCode::InvalidInput,
            CommandError::Io(_) => ErrorCode::IoError,
            CommandError::PermissionDenied(_) => ErrorCode::PermissionDenied,
            CommandError::NetworkError(_) => ErrorCode::NetworkError,
            CommandError::Timeout => ErrorCode::Timeout,
            CommandError::Unsupported(_) => ErrorCode::Unsupported,
            CommandError::Unimplemented(_) => ErrorCode::Unimplemented,
            CommandError::Other(_) => ErrorCode::InternalError,
        }
    }
}

/// Map a capability-layer error onto a command error, preserving the code.
impl From<crate::traits::CapError> for CommandError {
    fn from(e: crate::traits::CapError) -> Self {
        use crate::traits::CapError;
        match e {
            CapError::Unsupported(m) => CommandError::Unsupported(m),
            CapError::DependencyMissing(m) => CommandError::Unsupported(m),
            CapError::PermissionDenied(m) => CommandError::PermissionDenied(m),
            CapError::Io(io) => CommandError::Io(io),
            CapError::Network(m) => CommandError::NetworkError(m),
            CapError::Timeout => CommandError::Timeout,
            CapError::Other(m) => CommandError::Other(m),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-transport visibility
// ---------------------------------------------------------------------------

/// Which transports may invoke a command. Mirrors MCP-Template's per-service
/// exclusion set: a CLI-only utility can stay off the HTTP/MCP surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Expose {
    pub cli: bool,
    pub api: bool,
    pub mcp: bool,
}

impl Expose {
    pub const fn all() -> Self {
        Self {
            cli: true,
            api: true,
            mcp: true,
        }
    }
    pub const fn cli_only() -> Self {
        Self {
            cli: true,
            api: false,
            mcp: false,
        }
    }
    /// Everywhere except MCP (the reference's common case for utility commands).
    pub const fn no_mcp() -> Self {
        Self {
            cli: true,
            api: true,
            mcp: false,
        }
    }
}

impl Default for Expose {
    fn default() -> Self {
        Self::all()
    }
}

// ---------------------------------------------------------------------------
// The typed Command trait
// ---------------------------------------------------------------------------

/// A unit of backend logic with a typed input/output contract.
///
/// The trait deliberately does **not** bound `Input` on `clap::Args` — the
/// engine stays free of CLI concerns. Individual input structs may add a
/// `#[derive(clap::Args)]` when a hand-written/generated CLI subcommand wants
/// to reuse them (see the CLI scaffolding phase).
#[async_trait]
pub trait Command: Send + Sync + 'static {
    type Input: DeserializeOwned + JsonSchema + Send;
    type Output: Serialize + JsonSchema + Send;

    fn name(&self) -> &'static str;

    fn description(&self) -> &'static str {
        ""
    }

    /// Per-transport visibility. Defaults to everywhere.
    fn expose(&self) -> Expose {
        Expose::all()
    }

    async fn run(&self, input: Self::Input, cx: &Ctx<'_>) -> Result<Self::Output, CommandError>;
}

// ---------------------------------------------------------------------------
// Type erasure – object-safe inner trait doing Value<->typed conversion
// ---------------------------------------------------------------------------

/// Object-safe form of [`Command`]. Callers never implement this directly; it
/// is blanket-implemented for every `Command` and stored in the registry.
#[async_trait]
pub trait ErasedCommand: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn expose(&self) -> Expose;
    fn input_schema(&self) -> Value;
    fn output_schema(&self) -> Value;
    async fn run_json(&self, input: Value, cx: &Ctx<'_>) -> Result<Value, CommandError>;
}

#[async_trait]
impl<C: Command> ErasedCommand for C {
    fn name(&self) -> &'static str {
        Command::name(self)
    }
    fn description(&self) -> &'static str {
        Command::description(self)
    }
    fn expose(&self) -> Expose {
        Command::expose(self)
    }
    fn input_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(C::Input)).unwrap_or(Value::Null)
    }
    fn output_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(C::Output)).unwrap_or(Value::Null)
    }
    async fn run_json(&self, input: Value, cx: &Ctx<'_>) -> Result<Value, CommandError> {
        // An absent body is treated as an empty object so commands with no
        // required fields (e.g. `ping`) can be called with `{}` or nothing.
        let input = if input.is_null() {
            Value::Object(Default::default())
        } else {
            input
        };
        let typed: C::Input =
            serde_json::from_value(input).map_err(|e| CommandError::InvalidInput(e.to_string()))?;
        let out = Command::run(self, typed, cx).await?;
        serde_json::to_value(out).map_err(|e| CommandError::Other(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Link-time self-registration (inventory)
// ---------------------------------------------------------------------------

/// One collected registration entry. Each `register_command!` submits one.
pub struct CommandRegistration {
    pub make: fn() -> Box<dyn ErasedCommand>,
}

inventory::collect!(CommandRegistration);

/// Register a `Command` type so it self-installs into `CommandRegistry::new`.
/// The type must implement `Default`.
#[macro_export]
macro_rules! register_command {
    ($ty:ty) => {
        inventory::submit! {
            $crate::commands::CommandRegistration {
                make: || ::std::boxed::Box::new(<$ty as ::std::default::Default>::default()),
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Schema introspection
// ---------------------------------------------------------------------------

/// A command's public contract, consumed by `GET /commands` and future MCP.
#[derive(Debug, Clone, Serialize)]
pub struct CommandSchema {
    pub name: String,
    pub description: String,
    pub expose: Expose,
    pub input_schema: Value,
    pub output_schema: Value,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct CommandRegistry {
    commands: BTreeMap<&'static str, Box<dyn ErasedCommand>>,
}

impl CommandRegistry {
    /// Build the registry from every `register_command!`-registered command.
    pub fn new() -> Self {
        let mut commands: BTreeMap<&'static str, Box<dyn ErasedCommand>> = BTreeMap::new();
        for reg in inventory::iter::<CommandRegistration> {
            let cmd = (reg.make)();
            let name = cmd.name();
            if commands.insert(name, cmd).is_some() {
                // Two commands claimed the same name — a scaffolding mistake.
                panic!("duplicate command registered: {name}");
            }
        }
        Self { commands }
    }

    /// All registered command names, sorted.
    pub fn list(&self) -> Vec<&str> {
        self.commands.keys().copied().collect()
    }

    fn get(&self, name: &str) -> Option<&dyn ErasedCommand> {
        self.commands.get(name).map(|b| b.as_ref())
    }

    /// The schema for a single command, if present.
    pub fn schema(&self, name: &str) -> Option<CommandSchema> {
        self.get(name).map(|c| CommandSchema {
            name: c.name().to_string(),
            description: c.description().to_string(),
            expose: c.expose(),
            input_schema: c.input_schema(),
            output_schema: c.output_schema(),
        })
    }

    /// Schemas for every command, sorted by name.
    pub fn schemas(&self) -> Vec<CommandSchema> {
        self.list()
            .into_iter()
            .filter_map(|n| self.schema(n))
            .collect()
    }

    /// Bare-output path (HTTP API / MCP): deserialize `args`, run, serialize the
    /// typed `Output`. Returns the raw output value or a typed error the
    /// transport maps onto an HTTP status.
    pub async fn call(&self, name: &str, args: Value, cx: &Ctx<'_>) -> Result<Value, CommandError> {
        match self.get(name) {
            Some(cmd) => cmd.run_json(args, cx).await,
            None => Err(CommandError::InvalidInput(format!(
                "unknown command: {name}"
            ))),
        }
    }

    /// Envelope path (CLI / scenario runner): run a command and wrap the result
    /// in the diagnostic [`CommandResult`] contract (run_id, status, timing).
    pub async fn execute(&self, name: &str, args: Value, cx: &Ctx<'_>) -> CommandResult {
        let start = Instant::now();
        let run_id = cx.request_id.clone();

        match self.call(name, args, cx).await {
            Ok(data) => {
                let mut r = result_ok("call", name, &run_id, start.elapsed().as_millis() as u64);
                r.data = Some(data);
                r
            }
            Err(e) => result_err(
                "call",
                name,
                &run_id,
                start.elapsed().as_millis() as u64,
                e.error_code(),
                e.to_string(),
            ),
        }
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{AppContext, Ctx};

    async fn run(name: &str, args: Value) -> CommandResult {
        let ctx = AppContext::default();
        let cx = Ctx::new(&ctx);
        CommandRegistry::new().execute(name, args, &cx).await
    }

    #[tokio::test]
    async fn test_ping_command() {
        let result = run("ping", serde_json::json!({})).await;
        assert_eq!(result.status, Status::Pass);
        assert_eq!(result.data.unwrap()["pong"], true);
    }

    #[tokio::test]
    async fn test_ping_accepts_null_args() {
        // A null/absent body should be treated as `{}` for no-field inputs.
        let result = run("ping", Value::Null).await;
        assert_eq!(result.status, Status::Pass);
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let result = run("nonexistent", serde_json::json!({})).await;
        assert_eq!(result.status, Status::Error);
        assert_eq!(result.error.unwrap().code, ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn test_invalid_input_maps_to_error() {
        // read_file requires a 'path' string; omitting it is InvalidInput.
        let result = run("read_file", serde_json::json!({})).await;
        assert_eq!(result.status, Status::Error);
        assert_eq!(result.error.unwrap().code, ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn test_read_write_file() {
        let ctx = AppContext::default();
        let tmp = std::env::temp_dir().join("engine_test_rw.txt");
        let path_str = tmp.to_str().unwrap();

        let cx = Ctx::new(&ctx);
        let reg = CommandRegistry::new();
        let w = reg
            .execute(
                "write_file",
                serde_json::json!({ "path": path_str, "content": "hello engine" }),
                &cx,
            )
            .await;
        assert_eq!(w.status, Status::Pass);

        let cx = Ctx::new(&ctx);
        let r = reg
            .execute("read_file", serde_json::json!({ "path": path_str }), &cx)
            .await;
        assert_eq!(r.status, Status::Pass);
        assert_eq!(r.data.unwrap()["content"], "hello engine");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_list_commands() {
        let reg = CommandRegistry::new();
        let names = reg.list();
        for expected in [
            "ping",
            "read_file",
            "write_file",
            "system_info",
            "list_dir",
            "http_request",
        ] {
            assert!(names.contains(&expected), "missing command: {expected}");
        }
    }

    #[test]
    fn test_schema_introspection() {
        let reg = CommandRegistry::new();
        let schema = reg.schema("read_file").expect("read_file schema");
        assert_eq!(schema.name, "read_file");
        // read_file reads a caller-supplied path with no sandbox, so it is
        // CLI-only — reachable via the CLI, never over the unauthenticated API.
        assert!(schema.expose.cli);
        assert!(!schema.expose.api);
        // The input schema should describe the required `path` field.
        let s = serde_json::to_string(&schema.input_schema).unwrap();
        assert!(s.contains("path"), "input schema missing path: {s}");
    }

    #[tokio::test]
    async fn test_system_info_command() {
        let result = run("system_info", serde_json::json!({})).await;
        assert_eq!(result.status, Status::Pass);
        let data = result.data.unwrap();
        assert!(data["os"].is_string());
        assert!(data["arch"].is_string());
        assert!(data["hostname"].is_string());
    }

    #[tokio::test]
    async fn test_list_dir_command() {
        let tmp = std::env::temp_dir();
        let path_str = tmp.to_str().unwrap();
        let result = run("list_dir", serde_json::json!({ "path": path_str })).await;
        assert_eq!(result.status, Status::Pass);
        assert!(result.data.unwrap()["entries"].is_array());
    }

    #[tokio::test]
    async fn test_list_dir_not_a_directory() {
        let result = run(
            "list_dir",
            serde_json::json!({ "path": "/nonexistent_dir_12345" }),
        )
        .await;
        assert_eq!(result.status, Status::Error);
    }
}
