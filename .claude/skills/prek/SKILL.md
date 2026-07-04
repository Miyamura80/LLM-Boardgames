---
name: prek
description: Instructions for using prek as a replacement for pre-commit.
---
# Prek Skill
This skill provides instructions for using `prek` in the Rust-Template project. `prek` replaces `pre-commit` for managing Git hooks and running checks.
## Commands
- `prek run`: Run hooks on changed files.
- `prek run --all-files`: Run hooks on all files in the repository.
- `prek install`: Install Git hooks to run automatically on commit.
- `prek uninstall`: Remove Git hooks.
## Configuration
The configuration is located in `prek.toml`.
## Workflow
1.  **Before Committing**: Run `prek run` to check your changes.
2.  **CI**: The CI pipeline runs `make ci` directly. Running `prek run --all-files` locally is a good pre-push sanity check, but the authoritative gate is `make ci`.
