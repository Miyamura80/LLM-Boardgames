//! `appctl init` — one-time project onboarding.
//!
//! Turns this template into a real project: renames the template sentinels,
//! prunes the surfaces you don't want, and seeds `.env`. Everything routes
//! through [`config`] (the source of truth); this module only wires the flags,
//! the wizard, the dry-run plan, and the executors together.
//!
//! Flow: build a [`Config`] (from `--config`, `--profile`, or the wizard) →
//! apply per-axis overrides → `expand()` → print the plan → (dry-run stops
//! here) → confirm → rename + prune + env → print verification hints.

pub mod config;
mod env;
mod plan;
mod prune;
mod rename;
mod wizard;

use anyhow::{bail, Context, Result};
use config::{Config, Profile, Surface};
use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::PathBuf;

/// The `appctl init` subcommand flags (clap). Booleans use paired
/// `--flag`/`--no-flag` so an unspecified axis stays `None` and keeps the
/// profile default.
#[derive(Debug, clap::Args)]
pub struct InitArgs {
    /// Project profile: cli-only | server-only | cli+server.
    #[arg(long)]
    pub profile: Option<String>,
    /// Path to a YAML config file (overrides the profile).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Comma-separated surfaces to keep: cli,http_api.
    #[arg(long)]
    pub surfaces: Option<String>,
    #[arg(long)]
    pub frontend: bool,
    #[arg(long)]
    pub no_frontend: bool,
    #[arg(long)]
    pub docs: bool,
    #[arg(long)]
    pub no_docs: bool,
    #[arg(long)]
    pub docker: bool,
    #[arg(long)]
    pub no_docker: bool,
    /// Override the project name (default: prompted / template sentinel).
    #[arg(long)]
    pub name: Option<String>,
    /// Override the CLI binary name.
    #[arg(long)]
    pub cli_name: Option<String>,
    /// Override the GitHub owner/org (default: auto-detected from git remote).
    #[arg(long)]
    pub org: Option<String>,
    /// Override the one-line description.
    #[arg(long)]
    pub description: Option<String>,
    /// Print the plan without changing any files.
    #[arg(long)]
    pub dry_run: bool,
    /// Skip the confirmation prompt before applying.
    #[arg(long)]
    pub yes: bool,
    /// Repo root (defaults to the current directory).
    #[arg(long)]
    pub root: Option<PathBuf>,
}

impl From<InitArgs> for InitOptions {
    fn from(a: InitArgs) -> Self {
        let tri = |yes: bool, no: bool| {
            if no {
                Some(false)
            } else if yes {
                Some(true)
            } else {
                None
            }
        };
        InitOptions {
            profile: a.profile,
            config_path: a.config,
            surfaces: a.surfaces,
            frontend: tri(a.frontend, a.no_frontend),
            docs: tri(a.docs, a.no_docs),
            docker: tri(a.docker, a.no_docker),
            name: a.name,
            cli_name: a.cli_name,
            org: a.org,
            description: a.description,
            dry_run: a.dry_run,
            yes: a.yes,
            root: a.root,
        }
    }
}

/// Resolved options after mapping the clap flags.
#[derive(Debug, Default, Clone)]
pub struct InitOptions {
    pub profile: Option<String>,
    pub config_path: Option<PathBuf>,
    pub surfaces: Option<String>,
    pub frontend: Option<bool>,
    pub docs: Option<bool>,
    pub docker: Option<bool>,
    pub name: Option<String>,
    pub cli_name: Option<String>,
    pub org: Option<String>,
    pub description: Option<String>,
    pub dry_run: bool,
    pub yes: bool,
    pub root: Option<PathBuf>,
}

/// A `--config <yaml>` file. Every field is optional and overrides the profile.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    profile: Option<String>,
    surfaces: Option<Vec<String>>,
    frontend: Option<bool>,
    docs: Option<bool>,
    docker: Option<bool>,
    name: Option<String>,
    cli_name: Option<String>,
    org: Option<String>,
    description: Option<String>,
}

/// Entry point for the `init` subcommand.
pub fn run(args: InitArgs) -> Result<()> {
    execute(args.into())
}

fn execute(opts: InitOptions) -> Result<()> {
    let root = opts.root.clone().unwrap_or_else(|| PathBuf::from("."));
    let mut config = build_config(&opts)?;
    apply_overrides(&mut config, &opts)?;
    autodetect_org(&mut config, &root);
    config.expand();

    println!("{}", plan::render(&config, &root));

    if opts.dry_run {
        println!("Dry run — no files were changed. Re-run without --dry-run to apply.");
        return Ok(());
    }

    if !opts.yes && !wizard::confirm_apply()? {
        println!("Aborted — no files were changed.");
        return Ok(());
    }

    let renamed = rename::apply(&config, &root, false)?;
    let pruned = prune::apply(&config, &root, false)?;
    let env_seeded = env::ensure_env(&root, false)?;

    print_summary(renamed, &pruned, env_seeded);
    print_verify_hints(&config);
    Ok(())
}

/// Build the base config from `--config`, `--profile`, or the wizard.
fn build_config(opts: &InitOptions) -> Result<Config> {
    if let Some(path) = &opts.config_path {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let file: FileConfig =
            serde_yaml::from_str(&text).with_context(|| "parsing --config YAML")?;
        return config_from_file(file);
    }

    if let Some(profile_str) = &opts.profile {
        let profile = Profile::parse(profile_str)
            .with_context(|| format!("unknown profile `{profile_str}`"))?;
        return Ok(Config::from_profile(profile));
    }

    // No headless input: fall back to the interactive wizard.
    if !std::io::stdin().is_terminal() {
        bail!("no --profile or --config given and stdin is not a TTY; pass --profile <cli-only|server-only|cli+server> for non-interactive init");
    }
    let defaults = Config::from_profile(Profile::CliServer);
    wizard::run(&defaults)
}

fn config_from_file(file: FileConfig) -> Result<Config> {
    let profile = match &file.profile {
        Some(p) => Profile::parse(p).with_context(|| format!("unknown profile `{p}`"))?,
        None => Profile::CliServer,
    };
    let mut config = Config::from_profile(profile);
    if let Some(surfaces) = &file.surfaces {
        config.surfaces = parse_surfaces(surfaces)?;
        config.reconcile_extras_to_surfaces();
    }
    if let Some(v) = file.frontend {
        config.frontend = v;
    }
    if let Some(v) = file.docs {
        config.docs = v;
    }
    if let Some(v) = file.docker {
        config.docker = v;
    }
    if let Some(v) = file.name {
        config.project_name = v;
    }
    if let Some(v) = file.cli_name {
        config.cli_name = v;
    }
    if let Some(v) = file.org {
        config.org = v;
    }
    if let Some(v) = file.description {
        config.description = v;
    }
    Ok(config)
}

/// Apply per-axis CLI flag overrides on top of the base config.
fn apply_overrides(config: &mut Config, opts: &InitOptions) -> Result<()> {
    if let Some(surfaces) = &opts.surfaces {
        let list: Vec<String> = surfaces.split(',').map(|s| s.to_string()).collect();
        config.surfaces = parse_surfaces(&list)?;
        config.reconcile_extras_to_surfaces();
    }
    if let Some(v) = opts.frontend {
        config.frontend = v;
    }
    if let Some(v) = opts.docs {
        config.docs = v;
    }
    if let Some(v) = opts.docker {
        config.docker = v;
    }
    if let Some(v) = &opts.name {
        config.project_name = v.clone();
    }
    if let Some(v) = &opts.cli_name {
        config.cli_name = v.clone();
    }
    if let Some(v) = &opts.org {
        config.org = v.clone();
    }
    if let Some(v) = &opts.description {
        config.description = v.clone();
    }
    Ok(())
}

fn parse_surfaces(list: &[String]) -> Result<BTreeSet<Surface>> {
    let mut set = BTreeSet::new();
    for s in list {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        let surface =
            Surface::parse(s).with_context(|| format!("unknown surface `{s}` (cli | http_api)"))?;
        set.insert(surface);
    }
    if set.is_empty() {
        bail!("--surfaces resolved to an empty set");
    }
    Ok(set)
}

/// If the org is still the template sentinel, try to read the GitHub owner from
/// `git remote get-url origin`. Read-only; failures are ignored.
fn autodetect_org(config: &mut Config, root: &std::path::Path) {
    if config.org != config::SENTINEL_ORG {
        return; // user set it explicitly
    }
    let Ok(output) = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "origin"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let url = String::from_utf8_lossy(&output.stdout);
    if let Some(owner) = parse_owner(url.trim()) {
        config.org = owner;
    }
}

/// Extract the owner segment from a GitHub remote URL (ssh or https).
fn parse_owner(url: &str) -> Option<String> {
    let stripped = url.trim_end_matches(".git");
    // git@host:owner/repo  or  https://host/owner/repo  or proxy paths.
    let tail = stripped
        .rsplit_once(':')
        .map(|(_, t)| t)
        .unwrap_or(stripped);
    let mut segs: Vec<&str> = tail.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return None;
    }
    let _repo = segs.pop();
    segs.pop().map(|owner| owner.to_string())
}

fn print_summary(renamed: usize, pruned: &[String], env_seeded: bool) {
    println!("\nApplied:");
    println!("  renamed sentinels in {renamed} file(s)");
    if pruned.is_empty() {
        println!("  pruned nothing");
    } else {
        println!("  pruned {} item(s):", pruned.len());
        for d in pruned {
            println!("    - {d}");
        }
    }
    println!(
        "  .env {}",
        if env_seeded {
            "seeded from .env.example"
        } else {
            "left as-is"
        }
    );
}

fn print_verify_hints(config: &Config) {
    println!("\nNext steps:");
    println!("  cargo build --workspace");
    println!("  cargo test --workspace");
    if config.has_surface(Surface::Cli) {
        println!(
            "  {} --help && {} call ping",
            config.cli_name, config.cli_name
        );
    }
    if config.has_surface(Surface::HttpApi) {
        println!(
            "  {} serve   # then GET /healthz and /api/v1/commands",
            config.cli_name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_owner_handles_ssh_https_and_proxy() {
        assert_eq!(
            parse_owner("git@github.com:Acme/repo.git").as_deref(),
            Some("Acme")
        );
        assert_eq!(
            parse_owner("https://github.com/Acme/repo").as_deref(),
            Some("Acme")
        );
        assert_eq!(
            parse_owner("http://local_proxy@127.0.0.1:41729/git/Acme/Repo").as_deref(),
            Some("Acme")
        );
        assert_eq!(parse_owner("garbage"), None);
    }

    #[test]
    fn config_file_overrides_profile() {
        let file = FileConfig {
            profile: Some("cli+server".into()),
            frontend: Some(false),
            ..Default::default()
        };
        let mut c = config_from_file(file).unwrap();
        c.expand();
        assert!(!c.frontend);
        assert!(c.has_surface(Surface::HttpApi));
    }

    #[test]
    fn surfaces_override_replaces_set() {
        let mut c = Config::from_profile(Profile::CliServer);
        let opts = InitOptions {
            surfaces: Some("cli".into()),
            ..Default::default()
        };
        apply_overrides(&mut c, &opts).unwrap();
        c.expand();
        assert!(c.has_surface(Surface::Cli));
        assert!(!c.has_surface(Surface::HttpApi));
    }

    #[test]
    fn empty_surfaces_is_error() {
        assert!(parse_surfaces(&[]).is_err());
    }
}
