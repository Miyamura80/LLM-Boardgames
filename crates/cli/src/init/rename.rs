//! Bulk sentinel replacement across the source tree.
//!
//! Walks the repo (skipping VCS/build/dependency dirs), and for every file with
//! an allow-listed extension replaces each `(from, to)` rename rule in place.
//! `str::replace` makes this naturally idempotent: a second run finds no
//! remaining sentinels and no-ops.

use super::config::Config;
use anyhow::{Context, Result};
use std::path::Path;
use walkdir::WalkDir;

/// Directories never descended into.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".next", "dist", ".turbo"];

/// File extensions eligible for in-place rename.
const ALLOWED_EXTS: &[&str] = &[
    "rs",
    "toml",
    "ts",
    "tsx",
    "js",
    "jsx",
    "json",
    "md",
    "mdx",
    "yaml",
    "yml",
    "html",
    "css",
    "txt",
    "sh",
    "dockerfile",
];

/// Files (by exact name) eligible even without an allow-listed extension.
const ALLOWED_NAMES: &[&str] = &["Dockerfile", "Makefile", ".env.example"];

/// Apply all rename rules under `root`. When `dry_run`, counts affected files
/// without writing. Returns the number of files that would change / did change.
pub fn apply(config: &Config, root: &Path, dry_run: bool) -> Result<usize> {
    let rules = config.rename_rules();
    if rules.is_empty() {
        return Ok(0);
    }

    let mut changed = 0usize;
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_skipped(e.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() || !is_eligible(entry.path()) {
            continue;
        }

        // Skip binary / non-UTF-8 files, but surface real read errors (e.g.
        // permission denied) rather than silently leaving sentinels in place.
        let original = match std::fs::read_to_string(entry.path()) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {}", entry.path().display())),
        };

        let mut updated = original.clone();
        for (from, to) in &rules {
            if updated.contains(from.as_str()) {
                updated = updated.replace(from.as_str(), to);
            }
        }

        if updated != original {
            changed += 1;
            if !dry_run {
                std::fs::write(entry.path(), updated)
                    .with_context(|| format!("writing {}", entry.path().display()))?;
            }
        }
    }
    Ok(changed)
}

fn is_skipped(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| SKIP_DIRS.contains(&n))
        .unwrap_or(false)
}

fn is_eligible(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if ALLOWED_NAMES.contains(&name) {
            return true;
        }
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| ALLOWED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::config::Profile;

    fn cfg(name: &str) -> Config {
        let mut c = Config::from_profile(Profile::CliServer);
        c.project_name = name.to_string();
        c
    }

    #[test]
    fn renames_sentinels_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("README.md"),
            "# Rust-Template\nname: rust-template",
        )
        .unwrap();

        let n = apply(&cfg("Acme"), root, false).unwrap();
        assert_eq!(n, 1);
        let content = std::fs::read_to_string(root.join("README.md")).unwrap();
        assert_eq!(content, "# Acme\nname: acme");
    }

    #[test]
    fn dry_run_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.rs"), "// Rust-Template").unwrap();

        let n = apply(&cfg("Acme"), root, true).unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            std::fs::read_to_string(root.join("a.rs")).unwrap(),
            "// Rust-Template"
        );
    }

    #[test]
    fn idempotent_second_run_noops() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.toml"), "name = \"rust-template\"").unwrap();

        assert_eq!(apply(&cfg("Acme"), root, false).unwrap(), 1);
        assert_eq!(apply(&cfg("Acme"), root, false).unwrap(), 0);
    }

    #[test]
    fn skips_target_and_binary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("target/x.rs"), "Rust-Template").unwrap();
        std::fs::write(root.join("photo.png"), [0u8, 159, 146, 150]).unwrap();

        assert_eq!(apply(&cfg("Acme"), root, false).unwrap(), 0);
    }
}
