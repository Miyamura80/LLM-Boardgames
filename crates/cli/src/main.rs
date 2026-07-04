//! `appctl` – the Rust server template's unified CLI + HTTP API binary.
//!
//! Runs the shared `engine` command registry over multiple transports:
//! `serve` (axum HTTP API) plus the CLI diagnostics (`call`, `probe`, `doctor`,
//! `run-scenario`). `init` onboards the template into a real project, `new`
//! scaffolds a fresh engine command, and `mcp` is a stub for the future MCP
//! transport. Transports are cargo features (`cli`, `http-api`) so `appctl
//! init` can prune a surface and still leave a compiling project.

#[cfg(feature = "cli")]
mod diagnostics;
mod init;
mod mcp;
mod scaffold;
#[cfg(feature = "http-api")]
mod serve_http;

use clap::{Parser, Subcommand};

#[cfg(any(feature = "cli", feature = "http-api"))]
use engine::{AppContext, CommandRegistry};
#[cfg(feature = "cli")]
use std::path::PathBuf;

// CLI definition

#[derive(Parser)]
#[command(
    name = "appctl",
    version,
    about = "CLI + HTTP API harness for the Rust server template"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Onboard this template into a real project (rename, prune, .env).
    Init(init::InitArgs),

    /// Scaffold a new engine command from the template.
    New(scaffold::NewArgs),

    /// (stub) Serve the registry over MCP — designed-for, not yet implemented.
    Mcp,

    /// Collect environment facts and emit an env summary.
    #[cfg(feature = "cli")]
    Doctor {
        /// Output as JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
        /// Write result JSON to this path.
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Invoke a backend command by name with JSON args.
    #[cfg(feature = "cli")]
    Call {
        /// Command name (e.g. "ping", "read_file", "write_file").
        cmd: String,
        /// JSON args to pass to the command.
        #[arg(long, default_value = "{}")]
        args: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
        /// Abort the command after this long (e.g. "30s", "5000ms", "2m").
        #[arg(long)]
        timeout: Option<String>,
        /// Directory for artifacts output.
        #[arg(long)]
        artifacts: Option<PathBuf>,
    },

    /// Targeted capability check: filesystem or network.
    #[cfg(feature = "cli")]
    Probe {
        /// Probe target: filesystem | network
        target: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
        /// Directory for artifacts output.
        #[arg(long)]
        artifacts: Option<PathBuf>,
    },

    /// Run a scripted scenario from a YAML file.
    #[cfg(feature = "cli")]
    RunScenario {
        /// Path to the scenario YAML file.
        file: PathBuf,
        /// Directory for artifacts output.
        #[arg(long)]
        artifacts: Option<PathBuf>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
        /// Run interactively with go-back navigation.
        #[arg(long)]
        interactive: bool,
    },

    /// Start the HTTP API server (axum). Host/port default from config.
    #[cfg(feature = "http-api")]
    Serve {
        /// Bind host (overrides config `server.host`).
        #[arg(long)]
        host: Option<String>,
        /// Bind port (overrides config `server.port`).
        #[arg(long)]
        port: Option<u16>,
    },
}

// Main

#[tokio::main]
async fn main() {
    // Install ring as the rustls crypto provider (reqwest needs this with rustls-no-provider)
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Initialise tracing for CLI (structured, no config dependency)
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init(args) => {
            if let Err(e) = init::run(args) {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        }
        Commands::New(args) => {
            if let Err(e) = scaffold::run(args) {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        }
        Commands::Mcp => mcp::run(),
        #[cfg(feature = "cli")]
        Commands::Doctor { json, out } => diagnostics::cmd_doctor(json, out).await,
        #[cfg(feature = "cli")]
        Commands::Call {
            cmd,
            args,
            json,
            timeout,
            artifacts,
        } => {
            let ctx = AppContext::default();
            let registry = CommandRegistry::new();
            diagnostics::cmd_call(&cmd, &args, json, timeout, artifacts, &ctx, &registry).await
        }
        #[cfg(feature = "cli")]
        Commands::Probe {
            target,
            json,
            artifacts,
        } => {
            let ctx = AppContext::default();
            diagnostics::cmd_probe(&target, json, artifacts, &ctx).await
        }
        #[cfg(feature = "cli")]
        Commands::RunScenario {
            file,
            artifacts,
            json,
            interactive,
        } => {
            let ctx = AppContext::default();
            let registry = CommandRegistry::new();
            diagnostics::cmd_run_scenario(&file, json, interactive, artifacts, &ctx, &registry)
                .await
        }
        #[cfg(feature = "http-api")]
        Commands::Serve { host, port } => {
            let ctx = AppContext::default();
            let registry = CommandRegistry::new();
            let config = match app_config::try_get_config() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: failed to load configuration: {e}");
                    eprintln!(
                        "hint: ensure global_config.yaml exists, or set APP_CONFIG_PATH \
                         to point at your config file."
                    );
                    std::process::exit(1);
                }
            };
            let cfg = &config.server;
            let host = host.unwrap_or_else(|| cfg.host.clone());
            let port = port.unwrap_or(cfg.port);
            let settings = serve_http::ServeSettings::from_config(cfg);
            serve_http::run_server(host, port, ctx, registry, settings).await
        }
    }
}
