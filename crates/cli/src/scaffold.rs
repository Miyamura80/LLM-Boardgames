//! `appctl new <name>` — command scaffolding.
//!
//! Generates a new engine [`Command`] by substituting into
//! `templates/command.rs.tpl` and drops it in `crates/engine/src/commands/`.
//! Because commands self-register via `inventory`, the only wiring needed is the
//! `mod <name>;` line in `commands/mod.rs`, which this inserts alphabetically —
//! no hand-editing a registration list.
//!
//! Rust has no runtime module discovery, so the `mod` line is unavoidable; the
//! generator maintains it for you, preserving the "drop a file, it's wired" UX.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

/// The command template, embedded at build time so `appctl new` works from any
/// working directory.
const TEMPLATE: &str = include_str!("../../../templates/command.rs.tpl");

/// The `appctl new` subcommand flags (clap).
#[derive(Debug, clap::Args)]
pub struct NewArgs {
    /// Command name in snake_case (e.g. `fetch_url`).
    pub name: String,
    /// One-line command description.
    #[arg(long)]
    pub description: Option<String>,
    /// Repo root (defaults to the current directory).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Overwrite an existing command file.
    #[arg(long)]
    pub force: bool,
}

impl From<NewArgs> for NewOptions {
    fn from(a: NewArgs) -> Self {
        NewOptions {
            name: a.name,
            description: a.description,
            root: a.root,
            force: a.force,
        }
    }
}

/// Resolved options after mapping the clap flags.
#[derive(Debug, Default, Clone)]
pub struct NewOptions {
    pub name: String,
    pub description: Option<String>,
    pub root: Option<PathBuf>,
    pub force: bool,
}

/// Entry point for the `new` subcommand.
pub fn run(args: NewArgs) -> Result<()> {
    execute(args.into())
}

fn execute(opts: NewOptions) -> Result<()> {
    let name = normalize_name(&opts.name)?;
    let struct_name = pascal_case(&name);
    let description = opts
        .description
        .clone()
        .unwrap_or_else(|| format!("TODO: describe the {name} command."));
    let root = opts.root.clone().unwrap_or_else(|| PathBuf::from("."));

    let commands_dir = root.join("crates/engine/src/commands");
    if !commands_dir.is_dir() {
        bail!(
            "commands directory not found at {} — run this from the repo root",
            commands_dir.display()
        );
    }

    let dest = commands_dir.join(format!("{name}.rs"));
    if dest.exists() && !opts.force {
        bail!(
            "{} already exists (use --force to overwrite)",
            dest.display()
        );
    }

    let rendered = render(&name, &struct_name, &description);
    std::fs::write(&dest, rendered).with_context(|| format!("writing {}", dest.display()))?;

    let mod_path = commands_dir.join("mod.rs");
    let mod_src = std::fs::read_to_string(&mod_path)
        .with_context(|| format!("reading {}", mod_path.display()))?;
    let (updated, inserted) = insert_mod_decl(&mod_src, &name);
    if inserted {
        std::fs::write(&mod_path, updated)?;
    }

    println!("Created {}", dest.display());
    if inserted {
        println!("Registered `mod {name};` in {}", mod_path.display());
    } else {
        println!("`mod {name};` already present in {}", mod_path.display());
    }
    println!("\nNext:");
    println!("  edit {}", dest.display());
    println!("  cargo test --workspace");
    println!("  appctl call {name} --args '{{\"message\":\"hi\"}}'");
    Ok(())
}

fn render(name: &str, struct_name: &str, description: &str) -> String {
    TEMPLATE
        .replace("{{STRUCT}}", struct_name)
        .replace("{{NAME}}", name)
        .replace("{{DESCRIPTION}}", description)
}

/// Validate/normalise a command name to a snake_case Rust identifier.
fn normalize_name(raw: &str) -> Result<String> {
    let name = raw.trim();
    if name.is_empty() {
        bail!("command name cannot be empty");
    }
    let valid = name
        .chars()
        .enumerate()
        .all(|(i, c)| c == '_' || c.is_ascii_lowercase() || (i > 0 && c.is_ascii_digit()));
    if !valid || !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        bail!("command name `{name}` must be snake_case (lowercase, digits, underscores; start with a letter)");
    }
    Ok(name.to_string())
}

fn pascal_case(snake: &str) -> String {
    snake
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Insert `mod <name>;` alphabetically among the existing simple `mod X;`
/// declarations. Returns the new source and whether a line was added
/// (idempotent: an existing declaration is left untouched).
fn insert_mod_decl(src: &str, name: &str) -> (String, bool) {
    let lines: Vec<&str> = src.lines().collect();
    let is_mod_decl = |l: &str| {
        let t = l.trim();
        t.starts_with("mod ") && t.ends_with(';') && !t.contains('{')
    };

    let indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_mod_decl(l))
        .map(|(i, _)| i)
        .collect();

    let new_line = format!("mod {name};");
    if indices.is_empty() {
        return (src.to_string(), false); // no block to extend
    }
    if lines.iter().any(|l| l.trim() == new_line) {
        return (src.to_string(), false); // already declared
    }

    let first = indices[0];
    let last = *indices.last().unwrap();
    let mut mods: Vec<String> = indices
        .iter()
        .map(|&i| lines[i].trim().to_string())
        .collect();
    mods.push(new_line);
    mods.sort();
    mods.dedup();

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    out.extend(lines[..first].iter().map(|s| s.to_string()));
    out.extend(mods);
    out.extend(lines[last + 1..].iter().map(|s| s.to_string()));

    let mut joined = out.join("\n");
    if src.ends_with('\n') {
        joined.push('\n');
    }
    (joined, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_from_snake() {
        assert_eq!(pascal_case("http_request"), "HttpRequest");
        assert_eq!(pascal_case("ping"), "Ping");
        assert_eq!(pascal_case("my_new_thing"), "MyNewThing");
    }

    #[test]
    fn rejects_bad_names() {
        assert!(normalize_name("HttpRequest").is_err());
        assert!(normalize_name("2fast").is_err());
        assert!(normalize_name("has-dash").is_err());
        assert!(normalize_name("").is_err());
        assert!(normalize_name("good_name2").is_ok());
    }

    #[test]
    fn inserts_alphabetically() {
        let src = "use x;\n\nmod alpha;\nmod zeta;\n\nfn main() {}\n";
        let (out, inserted) = insert_mod_decl(src, "middle");
        assert!(inserted);
        assert!(out.contains("mod alpha;\nmod middle;\nmod zeta;"));
        assert!(out.contains("fn main() {}"));
    }

    #[test]
    fn insert_is_idempotent() {
        let src = "mod alpha;\nmod zeta;\n";
        let (out, inserted) = insert_mod_decl(src, "alpha");
        assert!(!inserted);
        assert_eq!(out, src);
    }

    #[test]
    fn render_substitutes_all_placeholders() {
        let out = render("my_cmd", "MyCmd", "does a thing");
        assert!(out.contains("pub struct MyCmd;"));
        assert!(out.contains("\"my_cmd\""));
        assert!(out.contains("does a thing"));
        assert!(!out.contains("{{"));
    }
}
