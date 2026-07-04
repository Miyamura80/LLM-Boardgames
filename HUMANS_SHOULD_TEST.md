# Humans Should Test

Manual test cases that require real infrastructure, release artifacts, or human
judgement and cannot be automated in CI.

---

## Release binaries (cross-platform)

**Reference:** [`RELEASING.md`](RELEASING.md), `.github/workflows/release.yml`.

The release workflow builds `appctl` per platform; the produced binaries can't
be fully exercised in CI.

- [ ] **Downloaded binary runs** — On each target OS (macOS, Windows, Linux),
  download the release archive, extract `appctl`, and run `appctl --help`,
  `appctl call ping --json`, and `appctl doctor --json`. Also run `appctl mcp`
  and confirm the documented stub behaviour: a stderr "not implemented" notice
  and exit code 69.
- [ ] **`serve` binds and shuts down** — Run `appctl serve`, hit
  `GET /healthz`, then send SIGINT/SIGTERM (Ctrl-C) and confirm it shuts down
  gracefully without a panic or hung socket.
- [ ] **Config via env** — Start with `APP__SERVER__PORT=9090` and confirm the
  server binds the overridden port; point `APP_CONFIG_PATH` at a file and
  confirm it loads.

## Frontend end-to-end (optional)

The optional frontend talks to `appctl serve` over `fetch`; the dev proxy and
live wiring are best verified in a browser.

- [ ] **Connectivity dot** — Run `appctl serve`, then `make dev`. Load the app:
  the header dot is green (`ping` succeeded) and the model name comes from
  `GET /api/v1/config`. Stop the server and reload — the dot goes red.
- [ ] **Settings panel** — Open settings; confirm the model/LLM/feature-flag
  values match `crates/config/global_config.yaml` (and that no secret/API-key
  value ever appears — it must be stripped by `FrontendConfig`).
- [ ] **Non-default port** — Run the server on a non-default port and start the
  dev server with `VITE_API_PROXY=http://127.0.0.1:<port>`; confirm the proxy
  still reaches the API.

## Onboarding prune (`appctl init`)

- [ ] **Pruned project builds** — In a scratch copy, run `appctl init` for each
  profile (`cli-only`, `server-only`, `cli+server`) and separately with the
  `--no-frontend` flag, then run `cargo build --workspace` and `make ci` in the
  initialized project and confirm it is green.
