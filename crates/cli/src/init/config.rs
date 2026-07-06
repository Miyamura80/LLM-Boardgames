//! Source of truth for `shbench init` onboarding.
//!
//! Every selectable choice, its default, the implications between choices
//! ([`Config::expand`]), the rename sentinels ([`Config::rename_rules`]), and
//! the exact prune operations each choice triggers ([`Config::prune_ops`]) are
//! defined here. The wizard, plan renderer, and executors all read this module;
//! nothing decides policy on its own.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Sentinels — the template's own names, replaced during rename.
// ---------------------------------------------------------------------------

/// Lowercase/kebab project sentinel (package.json name, crate references).
pub const SENTINEL_PROJECT_KEBAB: &str = "secret-hitler-evals";
/// Title-case project sentinel (README headings, directory prose).
pub const SENTINEL_PROJECT_TITLE: &str = "Secret-Hitler-Evals";
/// The CLI binary sentinel.
pub const SENTINEL_CLI: &str = "shbench";
/// The GitHub owner sentinel (auto-detected from `git remote` when possible).
pub const SENTINEL_ORG: &str = "Miyamura80";

// ---------------------------------------------------------------------------
// Profiles & surfaces
// ---------------------------------------------------------------------------

/// High-level project shape. A profile is a preset over the finer-grained
/// [`Surface`] set plus the optional extras.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// CLI diagnostics only; no HTTP server, no frontend.
    CliOnly,
    /// HTTP API only; no interactive CLI diagnostics.
    ServerOnly,
    /// Both the CLI and the HTTP API (the template default).
    CliServer,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "cli-only" | "cli" => Some(Self::CliOnly),
            "server-only" | "server" => Some(Self::ServerOnly),
            "cli-server" | "cli+server" | "both" => Some(Self::CliServer),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CliOnly => "cli-only",
            Self::ServerOnly => "server-only",
            Self::CliServer => "cli+server",
        }
    }

    pub const ALL: [Profile; 3] = [Self::CliOnly, Self::ServerOnly, Self::CliServer];
}

/// A service surface. Maps 1:1 onto a cargo feature of the `shbench` crate, so a
/// pruned surface compiles out cleanly rather than being deleted from source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    /// CLI diagnostic subcommands — cargo feature `cli`.
    Cli,
    /// axum HTTP API (`shbench serve`) — cargo feature `http-api`.
    HttpApi,
}

impl Surface {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "cli" => Some(Self::Cli),
            "http_api" | "http" | "api" | "server" => Some(Self::HttpApi),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::HttpApi => "http_api",
        }
    }

    /// The cargo feature (in `crates/cli/Cargo.toml`'s `default` list) this
    /// surface is gated behind.
    pub fn cargo_feature(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::HttpApi => "http-api",
        }
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// A fully-resolved onboarding configuration. Build one from a [`Profile`]
/// (`from_profile`) or the wizard, apply per-axis overrides, then call
/// [`Config::expand`] before planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub project_name: String,
    pub cli_name: String,
    pub org: String,
    pub description: String,
    pub profile: Profile,
    pub surfaces: BTreeSet<Surface>,
    pub frontend: bool,
    pub docs: bool,
    pub docker: bool,
}

impl Config {
    /// The preset for a profile, with template-default names and extras.
    pub fn from_profile(profile: Profile) -> Self {
        let mut surfaces = BTreeSet::new();
        match profile {
            Profile::CliOnly => {
                surfaces.insert(Surface::Cli);
            }
            Profile::ServerOnly => {
                surfaces.insert(Surface::HttpApi);
            }
            Profile::CliServer => {
                surfaces.insert(Surface::Cli);
                surfaces.insert(Surface::HttpApi);
            }
        }
        let has_api = surfaces.contains(&Surface::HttpApi);
        Self {
            project_name: SENTINEL_PROJECT_TITLE.to_string(),
            cli_name: SENTINEL_CLI.to_string(),
            org: SENTINEL_ORG.to_string(),
            description: "A Rust CLI + HTTP API application".to_string(),
            profile,
            surfaces,
            frontend: has_api,
            docs: true,
            docker: has_api,
        }
    }

    /// Normalise the configuration so implied choices are consistent. Idempotent.
    ///
    /// - a frontend needs the HTTP API to talk to, so it implies `http_api`;
    /// - dropping `http_api` drops the frontend and the Dockerfile (both target
    ///   the server), leaving a coherent CLI-only shape.
    pub fn expand(&mut self) {
        if self.frontend {
            self.surfaces.insert(Surface::HttpApi);
        }
        if !self.surfaces.contains(&Surface::HttpApi) {
            self.frontend = false;
            self.docker = false;
        }
        // A profile with nothing selected is meaningless; fall back to CLI.
        if self.surfaces.is_empty() {
            self.surfaces.insert(Surface::Cli);
        }
    }

    pub fn has_surface(&self, s: Surface) -> bool {
        self.surfaces.contains(&s)
    }

    /// Reset the API-dependent extras to match the current surface set. Call
    /// this after *narrowing* `surfaces` explicitly so a stale preset
    /// `frontend`/`docker` doesn't linger (and get re-added by `expand`). The
    /// user can still turn them back on afterward, which re-adds the API.
    pub fn reconcile_extras_to_surfaces(&mut self) {
        if !self.has_surface(Surface::HttpApi) {
            self.frontend = false;
            self.docker = false;
        }
    }

    // -- rename ------------------------------------------------------------

    /// Sentinel → replacement pairs applied across the source tree. Longer
    /// sentinels come first so a title-case match is never clobbered by the
    /// kebab-case rule.
    pub fn rename_rules(&self) -> Vec<(String, String)> {
        let mut rules = Vec::new();
        if self.project_name != SENTINEL_PROJECT_TITLE {
            rules.push((
                SENTINEL_PROJECT_TITLE.to_string(),
                self.project_name.clone(),
            ));
            // Only rewrite the kebab sentinel when the name yields a valid
            // (non-empty) kebab; otherwise a name like "!!!" would blank out
            // `secret-hitler-evals` in the manifests and break the generated project.
            let kebab = kebab_case(&self.project_name);
            if !kebab.is_empty() {
                rules.push((SENTINEL_PROJECT_KEBAB.to_string(), kebab));
            }
        }
        if self.cli_name != SENTINEL_CLI {
            rules.push((SENTINEL_CLI.to_string(), self.cli_name.clone()));
        }
        if self.org != SENTINEL_ORG {
            rules.push((SENTINEL_ORG.to_string(), self.org.clone()));
        }
        rules
    }

    // -- prune -------------------------------------------------------------

    /// The concrete filesystem/manifest mutations implied by this config,
    /// resolved against `root`. Order is stable so plans are reproducible.
    pub fn prune_ops(&self, root: &Path) -> Vec<PruneOp> {
        let cli_manifest = root.join("crates/cli/Cargo.toml");
        let mut ops = Vec::new();

        // A surface that is off has its cargo feature dropped from `default`.
        for surface in [Surface::Cli, Surface::HttpApi] {
            if !self.has_surface(surface) {
                ops.push(PruneOp::DropCargoDefaultFeature {
                    manifest: cli_manifest.clone(),
                    feature: surface.cargo_feature().to_string(),
                });
                if surface == Surface::HttpApi {
                    ops.push(PruneOp::DeletePath(
                        root.join("crates/cli/src/serve_http.rs"),
                    ));
                }
            }
        }

        if !self.frontend {
            // The frontend's sources live in `frontend/`, but it also contributes
            // deps/scripts to the root package.json and owns the root TS + knip
            // configs. Remove all of them so the pruned project has no dangling
            // references (a bare `bun run knip` / `tsc` would otherwise fail).
            ops.push(PruneOp::DeletePath(root.join("frontend")));
            ops.push(PruneOp::DeletePath(root.join("tsconfig.json")));
            ops.push(PruneOp::DeletePath(root.join("tsconfig.node.json")));
            ops.push(PruneOp::StripFrontendPackageJson {
                manifest: root.join("package.json"),
            });
            ops.push(PruneOp::DropKnipFrontendWorkspace {
                manifest: root.join("knip.json"),
            });
        }

        if !self.docs {
            ops.push(PruneOp::DeletePath(root.join("docs")));
            ops.push(PruneOp::DropPackageJsonWorkspace {
                manifest: root.join("package.json"),
                name: "docs".to_string(),
            });
        }

        if !self.docker {
            ops.push(PruneOp::DeletePath(root.join("Dockerfile")));
            ops.push(PruneOp::DeletePath(root.join(".dockerignore")));
        }

        ops
    }
}

// ---------------------------------------------------------------------------
// Prune operations
// ---------------------------------------------------------------------------

/// A single mutating step produced by [`Config::prune_ops`]. Rendered in the
/// dry-run plan and executed by `prune.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PruneOp {
    /// Recursively delete a file or directory (existence-guarded).
    DeletePath(PathBuf),
    /// Drop a feature from `[features].default` in a Cargo manifest.
    DropCargoDefaultFeature { manifest: PathBuf, feature: String },
    /// Remove a workspace entry from package.json `workspaces`.
    DropPackageJsonWorkspace { manifest: PathBuf, name: String },
    /// Strip frontend deps/scripts from package.json.
    StripFrontendPackageJson { manifest: PathBuf },
    /// Remove the root (`"."`) frontend workspace from knip.json.
    DropKnipFrontendWorkspace { manifest: PathBuf },
}

impl PruneOp {
    /// One-line human summary for the dry-run plan table.
    pub fn describe(&self) -> String {
        match self {
            PruneOp::DeletePath(p) => format!("delete {}", p.display()),
            PruneOp::DropCargoDefaultFeature { manifest, feature } => {
                format!("{}: drop default feature `{}`", manifest.display(), feature)
            }
            PruneOp::DropPackageJsonWorkspace { manifest, name } => {
                format!("{}: drop workspace `{}`", manifest.display(), name)
            }
            PruneOp::StripFrontendPackageJson { manifest } => {
                format!("{}: strip frontend deps + scripts", manifest.display())
            }
            PruneOp::DropKnipFrontendWorkspace { manifest } => {
                format!("{}: drop frontend workspace", manifest.display())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert an arbitrary project name into a kebab-case package identifier.
pub fn kebab_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_case_normalises() {
        assert_eq!(kebab_case("My Cool App"), "my-cool-app");
        assert_eq!(kebab_case("Secret-Hitler-Evals"), "secret-hitler-evals");
        assert_eq!(kebab_case("weird__name!!"), "weird-name");
    }

    #[test]
    fn frontend_implies_http_api() {
        let mut c = Config::from_profile(Profile::CliOnly);
        c.frontend = true;
        c.expand();
        assert!(c.has_surface(Surface::HttpApi));
    }

    #[test]
    fn dropping_api_drops_frontend_and_docker() {
        let mut c = Config::from_profile(Profile::CliServer);
        c.surfaces.remove(&Surface::HttpApi);
        // Frontend requires the API, so narrowing away the API drops it too.
        c.reconcile_extras_to_surfaces();
        c.expand();
        assert!(!c.frontend);
        assert!(!c.docker);
        assert!(!c.has_surface(Surface::HttpApi));
    }

    #[test]
    fn expand_is_idempotent() {
        let mut c = Config::from_profile(Profile::ServerOnly);
        c.expand();
        let once = c.clone();
        c.expand();
        assert_eq!(once, c);
    }

    #[test]
    fn cli_only_prunes_http_api_feature_and_serve() {
        let mut c = Config::from_profile(Profile::CliOnly);
        c.expand();
        let ops = c.prune_ops(Path::new("."));
        assert!(ops.iter().any(|o| matches!(
            o,
            PruneOp::DropCargoDefaultFeature { feature, .. } if feature == "http-api"
        )));
        assert!(ops
            .iter()
            .any(|o| matches!(o, PruneOp::DeletePath(p) if p.ends_with("serve_http.rs"))));
    }

    #[test]
    fn no_frontend_prunes_dangling_ts_and_knip_configs() {
        let mut c = Config::from_profile(Profile::ServerOnly);
        c.frontend = false;
        c.expand();
        let ops = c.prune_ops(Path::new("."));
        for rel in ["frontend", "tsconfig.json", "tsconfig.node.json"] {
            assert!(
                ops.iter()
                    .any(|o| matches!(o, PruneOp::DeletePath(p) if p.ends_with(rel))),
                "expected DeletePath for {rel}"
            );
        }
        assert!(ops
            .iter()
            .any(|o| matches!(o, PruneOp::DropKnipFrontendWorkspace { .. })));
        assert!(ops
            .iter()
            .any(|o| matches!(o, PruneOp::StripFrontendPackageJson { .. })));
    }

    #[test]
    fn full_config_prunes_nothing() {
        let mut c = Config::from_profile(Profile::CliServer);
        c.docs = true;
        c.docker = true;
        c.frontend = true;
        c.expand();
        assert!(c.prune_ops(Path::new(".")).is_empty());
    }

    #[test]
    fn rename_rules_skip_unchanged_names() {
        let c = Config::from_profile(Profile::CliServer);
        assert!(c.rename_rules().is_empty());
    }

    #[test]
    fn rename_rules_emit_title_and_kebab() {
        let mut c = Config::from_profile(Profile::CliServer);
        c.project_name = "Acme App".to_string();
        let rules = c.rename_rules();
        assert!(rules
            .iter()
            .any(|(f, t)| f == "Secret-Hitler-Evals" && t == "Acme App"));
        assert!(rules
            .iter()
            .any(|(f, t)| f == "secret-hitler-evals" && t == "acme-app"));
    }
}
