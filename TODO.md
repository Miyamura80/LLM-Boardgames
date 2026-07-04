# TODO

## Future Improvements

- [x] Rename `src-tauri/src/config.rs` to `global_config.rs`.
- [x] Automate Knip: Run `.github/workflows/knip.yml` (create if needed) on PR and push.
- [x] Automate Link Check: Run `.github/workflows/link_check.yml` (create if needed) weekly.
- [ ] Test Organization: Separate flaky, fast, and nondeterministic tests in the Rust test suite.

## Technical Debt / Cleanup


## CLI Test Harness – Post-v1

### Windows Support

- [ ] UAC / admin detection (`IsUserAnAdmin`, elevation prompt handling)
- [ ] Registry-based protocol handler checks (`HKEY_CLASSES_ROOT`)
- [ ] Path conventions (backslashes, `%APPDATA%`, `%LOCALAPPDATA%`)
- [ ] Named pipes as alternative to Unix sockets for `appctl serve`
- [ ] Clipboard via PowerShell `Get-Clipboard`/`Set-Clipboard`
- [ ] Headless detection: check for interactive desktop session vs. service context
- [ ] Windows-specific doctor checks (Windows version, build number, WSL detection)

### Output & Reporting

- [ ] JUnit XML output for CI integration (`--junit <path>`)
- [ ] Result upload / "push mode": POST `result.json` + artifact zip to a server
- [ ] HTML report generation from artifact directories

### Emit Events (Desktop Simulation) — DROPPED

Not applicable to a server template: PRD §8b removed the desktop `emit` command
(always Unsupported on a server). Kept here only as historical context.

- [ ] ~~Implement `tray-click` via platform APIs~~
- [ ] ~~Implement `deep-link` via custom URL scheme invocation~~
- [ ] ~~Implement `file-drop` via synthetic drag-and-drop events~~
- [ ] ~~Implement `app-focus` via window manager APIs~~

### Scenario Runner Enhancements

- [ ] Retry logic per step (`retries: 3, backoff: exponential`)
- [ ] Conditional steps (`when: "{{ prev.status == 'pass' }}"`)
- [ ] Parallel step execution (`parallel: [step1, step2]`)
- [ ] Variable interpolation across steps
- [ ] Timeout enforcement via `tokio::time::timeout`

### Engine

- [x] More built-in commands (system_info, list_dir, http_request)
- [x] Async command support
- [ ] Plugin system for custom command registration

### CLI

- [ ] Shell completions generation (`appctl completions bash/zsh/fish`)
- [ ] `appctl list` to show available commands and probes
- [ ] Configuration file for CLI defaults (`~/.appctl.toml`)
