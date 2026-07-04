# Product Requirements Document: Python-to-Rust Tauri Migration

> **ARCHIVED (legacy).** This describes the earlier Tauri/desktop foundation,
> which has since been superseded. The template is now a Rust server + CLI/API —
> see [`docs/PRD-server-template.md`](../PRD-server-template.md). Kept for history only.


## 1. Overview
This document tracks the transition from the legacy Python-based template to a native Rust + Tauri foundation. All Python scaffolding has been removed from the root workspace, and the remaining backend/asset tooling now lives in `src-tauri` with `bun` as the single frontend package manager and `cargo` for Rust tasks.

## 2. Core Constraints
- **Runtime**: The backend is Rust-only (`src-tauri/`), and there are no Python dependencies tracked in the repo anymore.
- **Package Management**: Run frontend scripts via `bun run …` (or `bunx` for globally unavailable tools) and backend helpers via `cargo`.
- **Asset Generation**: `cargo run --bin asset-gen -- <banner|logo>` produces documentation banner/logo assets; it requires `APP__GEMINI_API_KEY` to call the Gemini image API.
- **Testing**: All validation lives under `cargo test`; there are no more pytest targets or Python test suites.

## 3. Architecture & File Structure Changes

### Legacy Layout (before migration)
```text
.
├── CLAUDE.md / AGENTS.md        # Documentation referencing Python
├── Makefile                    # Python- and uv-based asset commands
├── init/                       # Python asset generation scripts
├── python_common/              # Pydantic config models & YAMLs
├── python_utils/               # DSPy/Langfuse logic + logging
├── src_python/                 # Python backend sources
├── tests/                      # Pytest suites (Config, Env, Healthchecks)
├── src-tauri/                  # Rust/Tauri project in early migration
└── ...
```

### Current Layout (after migration)
```text
.
├── CLAUDE.md / AGENTS.md       # Updated to describe the Rust stack & Bun tooling
├── Makefile                    # Hooks into `bun`, `cargo`, and the Rust asset-gen binary
├── src-tauri/
│   ├── Cargo.toml              # Includes config, tracing, serde, and the asset-gen binary
│   ├── global_config.yaml      # Still the YAML source of truth
│   ├── src/
│   │   ├── lib.rs              # Tauri command / binding layer
│   │   ├── global_config.rs     # Serde structs & loader ported from Python
│   │   ├── logging.rs           # Tracing subscriber replacing loguru
│   │   └── bin/
│   │       └── asset_gen.rs     # Banner/logo generation binary
│   └── target/                 # Rust build output (ignored)
└── docs/                       # Bun-based documentation site
```

## 4. Implementation Phases

### Phase 1: Documentation & Standards
*Establish the “Bun only” rule first.*
- [x] Update `CLAUDE.md` and `AGENTS.md` to advertise `bun`/`cargo` tooling and drop Python references.
- [x] Verify `package.json` scripts call `bun run` so npm/rnp/yarn are not needed.

### Phase 2: Rust Foundation (Config & Logging)
*Establish Rust as the backend source of truth.*
- [x] Add `config`, `tracing`, `tracing-subscriber`, `serde`, `serde_json`, `serde_yaml`, and friends to `src-tauri/Cargo.toml`.
- [x] Port `python_common/config_models.py` to `src-tauri/src/global_config.rs` (`#[derive(Deserialize)]`).
- [x] Implement the loader in Rust that reads `global_config.yaml` and respects `APP__…` overrides.
- [x] Replace `loguru` with a `tracing` subscriber in `src-tauri/src/logging.rs`.

### Phase 3: Test Migration
*Ensure reliability before deleting Python code.*
- [x] Rebuild the critical test coverage (`config`, `env`, type coercion) as `cargo test` suites.
- [x] Delete the legacy `tests/` folder and move assertions into `src-tauri/tests/` or `#[cfg(test)]` modules.

### Phase 4: Cleanup & Removal
*Eliminate leftover Python artifacts.*
- [x] Move `python_common/global_config.yaml` into `src-tauri/`.
- [x] Delete `src_python/`, `python_common/`, and `python_utils/` from the repository.
- [x] Delete the `init/` directory and replace it with the Rust asset generator in `src-tauri/src/bin/asset_gen.rs`.
- [x] Remove `pyproject.toml`, `uv.lock`, `pytest.ini`, and other Python lockfiles/configs.
- [x] Keep the Makefile focused on `bun`, `cargo`, and `asset-gen` commands.

### Phase 5: Frontend Integration
*Connect React to Rust.*
- [x] Expose the `get_config` Tauri command in `src-tauri/src/lib.rs`.
- [x] Add a React `useConfig` hook that invokes the Tauri command via `useInvoke()`.
- [x] Verify `bun run tauri dev` starts the app with the new config pipeline.

### Phase 6: Asset Generation
- [x] Rewrite `make banner` and `make logo` to call `cargo run --bin asset-gen -- <banner|logo>`, removing the `uv` dependency.

## 5. Success Criteria
- [x] `bun run tauri dev` launches, and the config hook surfaces the Rust-loaded YAML.
- [x] `cargo test` covers the former Python config/validation logic.
- [x] `cargo run --bin asset-gen -- banner|logo` generates MText assets without any Python dependency.
- [x] No `.py` files or `uv` tooling are tracked outside of `node_modules`/`docs/node_modules`.
