//! Executes the [`PruneOp`]s a [`Config`] implies: deleting pruned surfaces and
//! rewriting the manifests that reference them. Cargo edits use `toml_edit`
//! (format-preserving); package.json edits use `serde_json` (key order
//! preserved via the `preserve_order` feature). Every op is existence-guarded
//! and idempotent, so re-running after a partial pass is safe.

use super::config::{Config, PruneOp};
use anyhow::{Context, Result};
use std::path::Path;

/// Frontend dependency keys removed by [`PruneOp::StripFrontendPackageJson`].
/// These are the only deps the frontend contributes to the root manifest;
/// stripping them (plus deleting `frontend/` and the TS/knip configs) leaves a
/// clean, lint-green project with no dangling references.
const FRONTEND_DEPS: &[&str] = &[
    "react",
    "react-dom",
    "@vitejs/plugin-react",
    "@types/react",
    "@types/react-dom",
    "vite",
    "typescript",
];

/// Frontend npm scripts removed alongside the deps.
const FRONTEND_SCRIPTS: &[&str] = &["dev", "build", "preview"];

/// Apply every prune op for `config` under `root`. When `dry_run`, nothing is
/// written. Returns the human-readable descriptions of ops that had an effect.
pub fn apply(config: &Config, root: &Path, dry_run: bool) -> Result<Vec<String>> {
    let mut done = Vec::new();
    for op in config.prune_ops(root) {
        let effective = if dry_run {
            true
        } else {
            execute(&op).with_context(|| op.describe())?
        };
        if effective {
            done.push(op.describe());
        }
    }
    Ok(done)
}

/// Run one op. Returns whether it changed anything (false = already absent).
fn execute(op: &PruneOp) -> Result<bool> {
    match op {
        PruneOp::DeletePath(path) => {
            if !path.exists() {
                return Ok(false);
            }
            if path.is_dir() {
                std::fs::remove_dir_all(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
            Ok(true)
        }
        PruneOp::DropCargoDefaultFeature { manifest, feature } => {
            drop_cargo_default_feature(manifest, feature)
        }
        PruneOp::DropPackageJsonWorkspace { manifest, name } => {
            drop_package_json_workspace(manifest, name)
        }
        PruneOp::StripFrontendPackageJson { manifest } => strip_frontend_package_json(manifest),
        PruneOp::DropKnipFrontendWorkspace { manifest } => drop_knip_frontend_workspace(manifest),
    }
}

/// Remove the root (`"."`) frontend workspace from `knip.json` so a
/// frontend-pruned project's `bun run knip` doesn't point at deleted files.
fn drop_knip_frontend_workspace(manifest: &Path) -> Result<bool> {
    if !manifest.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(manifest)?;
    let mut json: serde_json::Value = serde_json::from_str(&text)?;
    let removed = json
        .get_mut("workspaces")
        .and_then(|w| w.as_object_mut())
        .map(|ws| ws.remove(".").is_some())
        .unwrap_or(false);
    if removed {
        write_json(manifest, &json)?;
    }
    Ok(removed)
}

fn drop_cargo_default_feature(manifest: &Path, feature: &str) -> Result<bool> {
    if !manifest.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(manifest)?;
    let mut doc = text.parse::<toml_edit::DocumentMut>()?;
    let Some(arr) = doc
        .get_mut("features")
        .and_then(|f| f.get_mut("default"))
        .and_then(|d| d.as_array_mut())
    else {
        return Ok(false);
    };
    let before = arr.len();
    arr.retain(|v| v.as_str() != Some(feature));
    if arr.len() == before {
        return Ok(false);
    }
    std::fs::write(manifest, doc.to_string())?;
    Ok(true)
}

fn drop_package_json_workspace(manifest: &Path, name: &str) -> Result<bool> {
    if !manifest.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(manifest)?;
    let mut json: serde_json::Value = serde_json::from_str(&text)?;
    let Some(arr) = json.get_mut("workspaces").and_then(|w| w.as_array_mut()) else {
        return Ok(false);
    };
    let before = arr.len();
    arr.retain(|v| v.as_str() != Some(name));
    if arr.len() == before {
        return Ok(false);
    }
    write_json(manifest, &json)?;
    Ok(true)
}

fn strip_frontend_package_json(manifest: &Path) -> Result<bool> {
    if !manifest.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(manifest)?;
    let mut json: serde_json::Value = serde_json::from_str(&text)?;
    let mut changed = false;

    for section in ["dependencies", "devDependencies"] {
        if let Some(obj) = json.get_mut(section).and_then(|s| s.as_object_mut()) {
            for dep in FRONTEND_DEPS {
                changed |= obj.remove(*dep).is_some();
            }
        }
    }
    if let Some(scripts) = json.get_mut("scripts").and_then(|s| s.as_object_mut()) {
        for script in FRONTEND_SCRIPTS {
            changed |= scripts.remove(*script).is_some();
        }
    }

    if changed {
        write_json(manifest, &json)?;
    }
    Ok(changed)
}

fn write_json(path: &Path, json: &serde_json::Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(json)?;
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::config::Profile;

    #[test]
    fn drops_http_api_feature_from_cargo() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[features]\ndefault = [\"cli\", \"http-api\"]\ncli = []\n",
        )
        .unwrap();

        assert!(drop_cargo_default_feature(&manifest, "http-api").unwrap());
        let out = std::fs::read_to_string(&manifest).unwrap();
        assert!(out.contains("default = [\"cli\"]"));
        assert!(!out.contains("http-api\"]"));
        // Idempotent second run.
        assert!(!drop_cargo_default_feature(&manifest, "http-api").unwrap());
    }

    #[test]
    fn strips_frontend_deps_but_keeps_others() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("package.json");
        std::fs::write(
            &manifest,
            r#"{"scripts":{"dev":"vite","knip":"knip"},"dependencies":{"react":"19","zod":"3"}}"#,
        )
        .unwrap();

        assert!(strip_frontend_package_json(&manifest).unwrap());
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
        assert!(json["dependencies"].get("react").is_none());
        assert!(json["dependencies"].get("zod").is_some());
        assert!(json["scripts"].get("dev").is_none());
        assert!(json["scripts"].get("knip").is_some());
    }

    #[test]
    fn drops_knip_frontend_workspace_keeps_docs() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("knip.json");
        std::fs::write(
            &manifest,
            r#"{"workspaces":{".":{"entry":["frontend/src/main.tsx"]},"docs":{"entry":["app/page.tsx"]}}}"#,
        )
        .unwrap();

        assert!(drop_knip_frontend_workspace(&manifest).unwrap());
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
        assert!(json["workspaces"].get(".").is_none());
        assert!(json["workspaces"].get("docs").is_some());
        // Idempotent second run.
        assert!(!drop_knip_frontend_workspace(&manifest).unwrap());
    }

    #[test]
    fn delete_path_is_existence_guarded() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert!(!execute(&PruneOp::DeletePath(missing)).unwrap());
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("crates/cli/src")).unwrap();
        std::fs::write(root.join("crates/cli/src/serve_http.rs"), "// serve").unwrap();
        std::fs::write(
            root.join("crates/cli/Cargo.toml"),
            "[features]\ndefault = [\"cli\", \"http-api\"]\n",
        )
        .unwrap();

        let mut c = Config::from_profile(Profile::CliOnly);
        c.expand();
        let done = apply(&c, root, true).unwrap();
        assert!(!done.is_empty());
        // Nothing actually removed.
        assert!(root.join("crates/cli/src/serve_http.rs").exists());
    }
}
