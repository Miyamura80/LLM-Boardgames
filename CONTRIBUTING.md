# Contributing

## Getting Started

1.  **Prerequisites**:
    *   Rust >= 1.75
    *   Bun >= 1.0 (needed for `make ci` tooling — Biome, Knip, audit,
        file_len_check — as well as the optional frontend / docs site)

2.  **Setup**:
    ```bash
    cargo build --workspace   # backend (engine + appctl)
    bun install               # optional: frontend / docs
    ```

3.  **Run Tests**:
    ```bash
    make test
    ```

## Development Workflow

1.  Create a new branch for your feature/fix.
2.  Make your changes.
3.  Ensure code quality commands pass:
    ```bash
    make ci
    ```
    This runs formatting (Biome + cargo fmt), linting (Biome + Clippy), dead code detection (Knip), and tests.

## Code Style

*   **Frontend**: React + TypeScript, follow Biome's rules.
*   **Backend**: Rust, follow standard idiomatic Rust and `cargo clippy`.
*   Use Biome for formatting and linting (handled by `make fmt` and `make lint`).
*   Add tests for new features.

## Pull Requests

*   Keep PRs focused on a single change.
*   Update documentation if necessary.
