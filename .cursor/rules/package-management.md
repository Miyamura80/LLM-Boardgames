This repository no longer relies on Python tooling. Bun is the only supported package manager for frontend/watch scripts and documentation builds, and any Rust helpers run via `cargo`.

## Workflow
- **Install deps**: `bun install` at the repo root to hydrate `bun.lock`/`bun.lockb` and keep `node_modules` aligned.
- **Run scripts**: Always use `bun run <script>` (or `bunx` when a tool isn’t installed globally) instead of `npm run`/`yarn`/`pnpm`.
- **Docs workspace**: The `docs/` site ships with its own `bun.lock`. Run `cd docs && bun install` whenever you sync the workspace.
- **Rust helpers**: Use `cargo` for backend tooling (`cargo test`, `cargo run --bin asset-gen -- banner|logo`, etc.); the asset generator also consults `APP__GEMINI_API_KEY`.
- **Lockfile hygiene**: Treat `bun.lock` (and `docs/bun.lock`) as the single source of truth—never edit it manually; use `bun install` to update it.

## Troubleshooting

- If you see `bun: command not found`, install Bun from https://bun.sh/ and re-run `bun install`.
- To get the current Bun version, run `bun --version` so reviewers know what runtime you tested with.
- If you hit asset generation issues, ensure `cargo` is up-to-date and `APP__GEMINI_API_KEY` is set before running `cargo run --bin asset-gen`.

## Do Not

- Do not install dependencies with `npm`, `yarn`, or `pnpm`.
- Do not reintroduce `uv`, `pip`, or any Python dependency managers; the backend is Rust and all JS tooling runs through Bun.
