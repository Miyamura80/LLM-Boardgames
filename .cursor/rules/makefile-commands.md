Some helpful commands are defined in `Makefile` to match the Rust + Bun workflow.

- `make dev`          - `bun run dev` (frontend development server)
- `make build`        - `bun run build` (frontend production bundle)
- `make tauri-dev`    - `bun run tauri dev` (Tauri development runner)
- `make banner`       - `cargo run --bin asset-gen -- banner` (Regenerates the docs banner)
- `make logo`         - `cargo run --bin asset-gen -- logo` (Regenerates logos/icons)
- `make test`         - `cargo test` (Runs the Rust unit and integration suites)
- `make fmt`          - `bunx @biomejs/biome check --write --unsafe .` + `cargo fmt`
- `make lint`         - `bunx @biomejs/biome check .` + `cargo clippy -- -D warnings`
