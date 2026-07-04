# Releasing

How to cut a new release of the `appctl` binary (the CLI + HTTP API server).

## Overview

Pushing a `v*` git tag triggers the [Release workflow](.github/workflows/release.yml),
which builds `appctl` for each target platform and attaches the archives to a
GitHub Release.

| Platform | Artifact |
|----------|----------|
| macOS    | `appctl` (Intel + Apple Silicon) archive |
| Windows  | `appctl.exe` archive |
| Linux    | `appctl` archive |

The canonical release pipeline is [`cargo-dist`](https://opensource.axo.dev/cargo-dist/),
configured in `dist-workspace.toml`. The committed `release.yml` is a functional
cross-platform `cargo build` baseline; run `dist init && dist generate ci` to
regenerate the full installer-producing workflow (shell + PowerShell installers)
from that config once `cargo-dist` is available on your machine.

## Release Workflow

### Step 1 — Bump versions

```bash
make bump-version VERSION=1.2.0
```

This updates the version field in `crates/cli/Cargo.toml` and `package.json`
and refreshes `Cargo.lock`.

### Step 2 — Commit and tag

```bash
git add crates/cli/Cargo.toml package.json Cargo.lock
git commit -m "⚙️ bump version to 1.2.0"
git tag v1.2.0
git push origin main --tags
```

### Step 3 — Watch CI

The [Release workflow](.github/workflows/release.yml) triggers on the tag. Check
the **Actions** tab; all platforms build in parallel.

### Step 4 — Verify the release

Once CI completes, visit **Releases** on GitHub, confirm the per-platform
archives are attached, edit the release notes if desired, and publish.

## Code Signing (optional)

`appctl` is a headless binary, so signing is not required to run it. If you
distribute installers via `cargo-dist` and want to avoid OS warnings, configure
platform signing in `dist-workspace.toml` per the cargo-dist docs (macOS
notarization, Windows Authenticode). No signing keys are needed for the baseline
`cargo build` release.

## Pre-release / Beta

Use a pre-release version and tag, then mark the GitHub Release as a
pre-release:

```bash
make bump-version VERSION=1.3.0-beta.1
git tag v1.3.0-beta.1 && git push origin v1.3.0-beta.1
```

The release workflow matches pre-release tags via a dedicated
`v[0-9]+.[0-9]+.[0-9]+-*` trigger (alongside the stable `vX.Y.Z` pattern), so a
`-beta.N` tag builds artifacts. If you change the tag scheme, update both
patterns in `.github/workflows/release.yml` or the push will silently no-op.

After CI completes, edit the GitHub Release and check **This is a pre-release**.
