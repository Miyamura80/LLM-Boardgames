---
name: code-quality
description: Instructions for running code quality checks and maintaining standards in the Rust-Template project.
---
# Code Quality Skill

This skill provides instructions for running code quality checks and maintaining standards in the Rust-Template project.

## Commands

Use the following `make` targets to ensure code quality:

- `make fmt`: Formats code with Biome and rustfmt.
- `make lint`: Lints code with Biome and Clippy.
- `make knip`: Finds unused files, dependencies, and exports.
- `make ci`: Runs all CI checks (fmt, lint, knip, audit, link-check, test).

## Workflow

1. **Before Committing**: Always run `make fmt` and `make lint`.
2. **Major Changes**: Run `make ci` to ensure no regressions in types or dead code.
3. **Continuous Integration**: These checks are enforced in the CI pipeline. Ensure all pass before opening a PR.
