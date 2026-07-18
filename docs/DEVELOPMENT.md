# Development Guide

Building, testing, and releasing easy-review. For user-facing docs see the [README](../README.md).

Cargo workspace at the repo root:

| Crate / package | Output | Role |
|-----------------|--------|------|
| `er-engine` | library | Core logic (git, AI sidecars, state) — shared by TUI and desktop |
| `er-tui` | `er` binary | Terminal UI |
| `er-desktop` | `er-desktop` binary | Tauri desktop shell |
| `desktop-ui` | Vite bundle | Svelte frontend (bundled by Tauri at build time) |

Run Rust commands from the **repo root** unless noted. Prefer scoped builds — compiling the whole workspace pulls in Tauri and can bloat `target/` to tens of GB.

## Cargo aliases

Defined in [`.cargo/config.toml`](../.cargo/config.toml). Each alias runs [`cargo er <task>`](../.cargo/bin/cargo-er), which execs the matching script. Add `.cargo/bin` to your `PATH` once (e.g. [`direnv allow`](../.envrc) if you use direnv, or `export PATH="$PWD/.cargo/bin:$PATH"`).

| Alias | Script | Use for |
|-------|--------|---------|
| `cargo tui-build` | `scripts/er-tui.sh build -p er-tui` | Dev build of terminal `er` → `target/tui/` |
| `cargo tui-release` | `scripts/er-tui.sh build --release -p er-tui` | Release build → `target/tui/release/er` |
| `cargo tui-install` | `scripts/er-tui.sh install --path crates/er-tui` | Install `er` to `~/.cargo/bin` |
| `cargo tui-test` | `scripts/er-tui.sh test -p er-engine -p er-tui` | Fast tests (no desktop) |
| `cargo tui-run` | `scripts/er-tui.sh run -p er-tui` | Run TUI from repo root |
| `cargo tui …` | `scripts/er-tui.sh …` | Passthrough to scoped cargo |
| `cargo er-dev` | `scripts/tauri-dev.sh` | Desktop dev (`target/desktop/`) |
| `cargo desktop-release` | `scripts/tauri-build.sh` | Desktop `.app` + `.dmg` bundle (ad-hoc) |
| `cargo desktop-sign-release` | `scripts/tauri-sign-release.sh` | Developer ID signed + notarized `.app` / `.dmg` |

Extra args append: `cargo tui-build --release`, `cargo er-dev --logs arena`.

## Wrapper scripts (recommended)

| Script | `CARGO_TARGET_DIR` | Purpose |
|--------|-------------------|---------|
| [`scripts/er-tui.sh`](../scripts/er-tui.sh) | `target/tui` | TUI / engine builds and tests |
| [`scripts/tauri-dev.sh`](../scripts/tauri-dev.sh) | `target/desktop` | Tauri dev server |
| [`scripts/tauri-build.sh`](../scripts/tauri-build.sh) | `target/desktop` | Desktop release bundle (`.app` + `.dmg`, ad-hoc) |
| [`scripts/tauri-sign-release.sh`](../scripts/tauri-sign-release.sh) | `target/desktop` | Signed + notarized release (Developer ID) |
| [`scripts/cargo-gc.sh`](../scripts/cargo-gc.sh) | — | Prune bloated `target/debug` (auto-run from dev scripts) |

```bash
./scripts/er-tui.sh build -p er-tui
./scripts/er-tui.sh test -p er-engine -p er-tui
./scripts/er-tui.sh run -p er-tui -- --filter '*.rs'
./scripts/tauri-dev.sh --logs arena
./scripts/cargo-gc.sh              # manual reclaim; --force also drops target/desktop
```

## Per-crate: local dev

### `er-engine` (library)

No binary — build via dependents or check directly.

```bash
# Dev
cargo check -p er-engine
cargo test -p er-engine
./scripts/er-tui.sh test -p er-engine

# Debug logging (git diffs)
ER_DEBUG=1 ./scripts/er-tui.sh run -p er-tui   # overwrites /tmp/er_debug.log per diff call
```

### `er-tui` (terminal `er`)

```bash
# Dev
cargo tui-build                                # or: ./scripts/er-tui.sh build -p er-tui
cargo tui-run -- --pr 42                       # pass CLI args after --
cargo tui-test
ER_DEBUG=1 cargo tui-run

# Install to ~/.cargo/bin
cargo install --path crates/er-tui

# Release (local)
./scripts/er-tui.sh build --release -p er-tui
# → target/tui/release/er
```

### `er-desktop` (Tauri app)

```bash
# Dev (recommended — sets ER_LOG, isolated target/desktop)
./scripts/tauri-dev.sh
./scripts/tauri-dev.sh --logs arena
./scripts/tauri-dev.sh --logs=arena,profile

# Dev (cargo alias — same as tauri-dev.sh)
cargo er-dev
cargo er-dev --logs arena

# Dev (manual)
cd crates/er-desktop && cargo tauri dev
```

**Profiling (opt-in):**

```bash
ER_DESKTOP_PROFILE_POLL=1 ER_LOG=profile ./scripts/tauri-dev.sh
```

Rust logs `er-desktop kind=…` to stderr; the webview logs `[er-profile]` to devtools console. See [`crates/er-desktop/agent.md`](../crates/er-desktop/agent.md) for log groups.

### `desktop-ui` (frontend only)

Storybook / Vite without the Rust backend:

```bash
cd desktop-ui
bun install
bun run dev          # http://localhost:5183
bun run storybook    # http://localhost:6006
bun run check        # svelte-check
bun test src         # unit tests
```

## Per-crate: release build

### `er-tui` → `er` binary

```bash
./scripts/er-tui.sh build --release -p er-tui
# Binary: target/tui/release/er

cargo build --release -p er-tui
# Binary: target/release/er
```

Release profile uses LTO + strip ([`Cargo.toml`](../Cargo.toml) `[profile.release]`). First release build is slow; incremental rebuilds are faster.

### `er-desktop` → desktop app bundle

```bash
./scripts/tauri-build.sh              # or: cargo desktop-release (target/desktop)
cargo desktop-release

# macOS output (after build):
#   target/.../bundle/macos/Easy Review.app
#   target/.../bundle/dmg/Easy Review_*.dmg  (when ER_SKIP_DMG=0)
```

Requires `cargo-tauri` and platform deps (WebKit/GTK on Linux). `tauri-build.sh` is for **local** builds (ad-hoc sign). For a Gatekeeper-clean downloadable `.dmg`, use the signed release flow below.

## macOS signed release (Developer ID + notarization)

Use this when shipping a desktop `.dmg` others can open without right-click → Open.

### One-time setup

1. Apple Developer Program membership.
2. Create a **Developer ID Application** certificate (G2 Sub-CA) at
   [Certificates](https://developer.apple.com/account/resources/certificates/list) — not Apple Development,
   not App Store Distribution, nothing under Services.
3. Install the `.cer` into your login keychain. Confirm:

   ```bash
   security find-identity -v -p codesigning
   # expect: "Developer ID Application: … (TEAMID)"
   ```

4. Create an [app-specific password](https://appleid.apple.com) for notarization.
5. Credentials file (gitignored — never commit):

   ```bash
   cp .env.signing.example .env.signing
   # edit: APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID
   ```

   `APPLE_SIGNING_IDENTITY` is optional if only one Developer ID Application identity exists.

### Build a signed + notarized release

```bash
./scripts/tauri-sign-release.sh
# or: cargo desktop-sign-release
# or: just sign-release-desktop
```

Output:

- `target/desktop/release/bundle/macos/Easy Review.app`
- `target/desktop/release/bundle/dmg/Easy Review_<version>_aarch64.dmg` (or `_x64`)

Do **not** use `./scripts/tauri-build.sh` for distribution — it ad-hoc re-signs and would wipe Developer ID.

### Verify

```bash
codesign -dv --verbose=4 "target/desktop/release/bundle/macos/Easy Review.app"
spctl -a -vv "target/desktop/release/bundle/macos/Easy Review.app"
```

### Notarization stuck on “In Progress”

First submissions (especially on a new Developer ID) can sit in Apple’s queue for **hours to a few days**. That is normal.

1. Ctrl+C if the script is waiting forever — the signed `.app` is already on disk.
2. Check status (do **not** start another upload while one is In Progress):

   ```bash
   set -a && source .env.signing && set +a
   xcrun notarytool history --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID"
   xcrun notarytool info <submission-id> \
     --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID"
   ```

3. When status is **Accepted**:

   ```bash
   xcrun stapler staple "target/desktop/release/bundle/macos/Easy Review.app"
   # then re-run ./scripts/tauri-sign-release.sh to pack/notarize the DMG,
   # or wait until a DMG-only mode exists
   ```

4. If **Invalid**, fetch the log:

   ```bash
   xcrun notarytool log <submission-id> \
     --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID"
   ```

5. Still In Progress after ~7 days → Apple Developer Forums (Notarization) with the submission id.

### Env knobs

| Variable | Default | Meaning |
|----------|---------|---------|
| `ER_SKIP_DMG` | `0` | Set `1` to skip DMG |
| `ER_SKIP_INSTALL` | `1` | Set `0` to also copy into `/Applications` (keeps Developer ID; no ad-hoc re-sign) |
| `ER_SKIP_OPEN_DMG` | `1` | Set `0` to open the DMG in Finder |
| `ER_SKIP_NOTARIZE` | `0` | Set `1` to sign only (Gatekeeper will still warn) |
| `ER_SIGNING_ENV` | `.env.signing` | Alternate credentials file path |

Reference: [Tauri macOS signing](https://v2.tauri.app/distribute/sign/macos/).

### `desktop-ui` → static assets

```bash
cd desktop-ui
bun run build        # dist/ for Vite production bundle
```

## Whole workspace

Use when touching multiple crates or before a large merge. **Slow** — compiles Tauri and all test binaries.

```bash
# Dev
cargo check --workspace
cargo test --workspace
cargo build --workspace

# Release
cargo build --workspace --release

# Lint / format
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Fast CI-style subset (no desktop):

```bash
cargo tui-test
cargo clippy -p er-engine -p er-tui --all-targets -- -D warnings
```

## Releasing the TUI (`er`)

Published releases are **terminal `er` only**, built by [`.github/workflows/release.yml`](../.github/workflows/release.yml) on tag push.

**Maintainer flow:**

```bash
# 1. Bump version in Cargo.toml ([workspace.package] version)
# 2. Commit, tag, push
git tag v0.3.0
git push origin main
git push origin v0.3.0
```

CI builds `er-tui` for `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-unknown-linux-gnu`, packages `er-<target>.tar.gz`, and creates a GitHub Release.

**Local release smoke test:**

```bash
cargo build --release -p er-tui
./target/release/er --help
```

**End-user install** of a published release:

```bash
curl -fsSL https://raw.githubusercontent.com/VilfredSikker/easy-review/main/install.sh | bash
# specific version:
curl -fsSL .../install.sh | bash -s -- --version v0.3.0
```

## `target/` hygiene

`cargo test` / `cargo build` without `-p` compile **er-desktop** into shared `target/`, which can grow to tens of GB (stale `debug/deps` files). Mitigations:

- TUI work → `./scripts/er-tui.sh` (`target/tui`)
- Desktop work → `./scripts/tauri-dev.sh` (`target/desktop`)
- Reclaim disk → `./scripts/cargo-gc.sh` (prunes when `target/debug` exceeds ~8 GB or 80k files)

## Repo utility scripts

Run from a **git repo** (or repo root for path-relative tools):

| Script | Purpose |
|--------|---------|
| [`scripts/er-tui.sh`](../scripts/er-tui.sh) | TUI-scoped `cargo` wrapper (`target/tui`, auto GC) |
| [`scripts/tauri-dev.sh`](../scripts/tauri-dev.sh) | Desktop dev launcher (`target/desktop`, `ER_LOG`) |
| [`scripts/cargo-gc.sh`](../scripts/cargo-gc.sh) | Prune bloated Cargo `target/` artifacts |
| [`scripts/generate-test-fixtures.sh`](../scripts/generate-test-fixtures.sh) | Create sample `.er/` AI files from the current diff (for testing overlays) |
| [`scripts/er-hash-files.sh`](../scripts/er-hash-files.sh) | Per-file SHA-256 hashes from a `git diff` (writes `.er/diff-tmp`) |
| [`scripts/er-freshness-check.sh`](../scripts/er-freshness-check.sh) | Validate base branch + diff hash (used by skills) |
| [`scripts/er-cleanup-reviews.sh`](../scripts/er-cleanup-reviews.sh) | Remove `.er/review.json`, checklist, summary, etc. |
| [`scripts/er-cleanup-questions.sh`](../scripts/er-cleanup-questions.sh) | Remove `.er/questions.json` |

Example:

```bash
cd your-repo
bash /path/to/easy-review/scripts/generate-test-fixtures.sh
```
