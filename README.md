# easy-review (`er`)

A terminal-based git diff review tool built for developers who use AI coding assistants.

AI writes code faster than you can review it. `er` makes review fast, navigable, and live-updating.

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)

## Install

### Quick install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/VilfredSikker/easy-review/main/install.sh | bash
```

Installs a pre-built binary to `~/.local/bin/`. Options: `--dir /usr/local/bin`.

### From source

```bash
git clone https://github.com/VilfredSikker/easy-review.git
cd easy-review
cargo install --path crates/er-tui
```

Requires Rust 1.85+. No runtime dependencies beyond git. Single binary (`er`).

## Usage

Run `er` from any git repository:

```bash
er                                        # Review current branch diff
er --pr 42                                # Open a GitHub PR
er https://github.com/owner/repo/pull/42  # Open a PR by URL
er ~/projects/api ~/projects/frontend     # Open multiple repos as tabs
er --filter '*.rs'                        # Pre-filter files
```

Base branch is auto-detected from upstream tracking, falling back to main/master/develop.

### AI Review Workflow

1. Split your terminal — Claude Code on one side, `er` on the other
2. Run `/er-review` in Claude Code to generate AI analysis
3. Press `v` in `er` to cycle AI views — findings appear inline in the diff
4. Leave questions (`q`) or comments (`c`) — AI responds on next `/er-review` run
5. Watch mode is on by default — diffs refresh automatically as code changes

## Features

- **Live watch mode** — Auto-refreshes on file edits, staging, and commits. Reviewed files auto-unmark when their diff changes.
- **AI-powered review** — `/er-review` generates per-file risk levels, inline findings, and a review checklist. Four view modes: Default, Overlay, Side Panel, AI Review.
- **Two comment types** — Personal questions (`q`/`Q`, yellow) for your own notes, and GitHub comments (`c`/`C`, cyan) for PR discussions. Reply with `r`, delete with `d`.
- **GitHub PR sync** — Pull review comments with `G`, push yours back with `P`. Two-way sync via `gh` CLI.
- **Four diff modes** — Branch diff (`1`), unstaged (`2`), staged (`3`), commit history (`4`). Sort by recency with `Shift+R`.
- **Large diff performance** — Auto-compacts lock files and generated code. Lazy-parses 5,000+ line diffs. Viewport-based rendering only builds visible lines.
- **Review tracking** — Mark files reviewed with `Space`, filter to unreviewed with `u`, jump to next unreviewed with `U`.
- **Composable filters** — `f` to filter by glob/status/size (`+*.rs,-*.lock,>50`). `F` for presets and history.
- **Multi-repo tabs** — Open multiple repos or worktrees as tabs. Switch with `]`/`[`.
- **Syntax highlighting** — TUI: syntect with content-hash caching. Desktop: Shiki in a Web Worker with per-file LRU cache.
- **Editor integration** — Jump to the current file in `$EDITOR` with `e`. Copy hunk with `y`.
- **Configurable** — Per-repo or global `.er-config.toml`. In-app settings overlay (`S`) with live preview.

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `j` / `k` | Next / prev file |
| `n` / `N` | Next / prev hunk |
| `Down` / `Up` | Next / prev line (within hunks) |
| `h` / `l` | Scroll left / right |
| `Ctrl-d` / `Ctrl-u` | Half page down / up |

### Diff Modes

| Key | Action |
|-----|--------|
| `1` | Branch diff (vs base branch) |
| `2` | Unstaged changes |
| `3` | Staged changes |
| `4` | Commit history |
| `Shift+R` | Toggle sort by recency |

### Actions

| Key | Action |
|-----|--------|
| `s` | Stage / unstage file |
| `Space` | Toggle file as reviewed |
| `u` | Filter to unreviewed files |
| `U` | Jump to next unreviewed file |
| `q` | Question on current line (personal, yellow) |
| `Q` | Question on current hunk |
| `c` | Comment on current line (GitHub, cyan) |
| `C` | Comment on current hunk |
| `y` | Copy current hunk to clipboard |
| `e` | Open file in `$EDITOR` |
| `r` | Refresh diff |
| `w` | Toggle watch mode |
| `W` | Toggle watched files section |
| `/` | Search files by name |
| `f` | Filter files (glob, status, size) |
| `F` | Filter presets & history |
| `Enter` | Expand compacted file |
| `S` | Open settings |

### Comments (when focused with `Tab`)

| Key | Action |
|-----|--------|
| `Tab` | Toggle comment focus mode |
| `Down` / `Up` | Navigate between comments |
| `r` | Reply to comment |
| `d` | Delete comment |
| `R` | Toggle resolved |

### GitHub Sync (requires `gh` CLI)

| Key | Action |
|-----|--------|
| `G` | Pull PR comments from GitHub |
| `P` | Push local comments to GitHub |

### AI Views

| Key | Action |
|-----|--------|
| `v` / `V` | Cycle AI view mode forward / backward |

### Tabs & Repos

| Key | Action |
|-----|--------|
| `]` / `[` | Next / prev tab |
| `x` | Close tab |
| `t` | Worktree picker |
| `o` | Directory browser |

### General

| Key | Action |
|-----|--------|
| `Esc` | Clear search/filter (innermost first) |
| `Ctrl+q` | Quit |

## Configuration

`er` loads settings from TOML config files:

1. `{repo}/.er-config.toml` (per-repo, highest priority)
2. `~/.config/er/config.toml` (global)
3. Built-in defaults

Press `S` to open the settings overlay. Changes apply immediately. Press `s` to save, `Esc` to revert.

```toml
[features]
view_branch = true        # branch diff mode (1)
view_unstaged = true      # unstaged mode (2)
view_staged = true        # staged mode (3)
ai_overlays = true        # AI view cycling (v/V)
blame_annotations = false # git blame on findings

[display]
tab_width = 4
line_numbers = true
wrap_lines = false

[watched]
paths = [".work/**/*.md"]
diff_mode = "content"     # "content" or "snapshot"
```

## AI Integration

Review sidecars (AI output, questions, comments, session state) live in **managed app data** by default — shared between the TUI and desktop app, per repo/branch/view. They are **not** read from `<repo>/.er/` unless you opt into repo-local mode.

### Where files live

**Default (managed storage)**

| Platform | Root |
|----------|------|
| Linux | `~/.local/share/easy-review/` |
| macOS | `~/Library/Application Support/easy-review/` |
| Windows | `%APPDATA%\easy-review\` (via `dirs`) |

**Local diff modes** (`1` branch / `2` unstaged / `3` staged / `4` history) — one directory per view bucket:

```text
<storage-root>/repos/<repo-slug>/branches/<branch-slug>/view-buckets/<bucket>/
```

`<bucket>` is `branch`, `unstaged`, `staged`, or `history`. Example (Linux, repo `easy-review`, branch `main`, branch diff):

```text
~/.local/share/easy-review/repos/easy-review/branches/main/view-buckets/branch/
```

**PR diff mode** — keyed by `owner/repo` + PR number (shared between a local clone and remote PR tabs):

```text
<storage-root>/repos/<owner-repo-slug>/prs/pr-<N>/
```

`<repo-slug>` comes from the `origin` remote name (fallback: repo folder basename). Slashes in branch names become `-`.

**Repo-local fallback** — set `ER_REPO_LOCAL=1` to read and write `<repo>/.er/` instead (useful for debugging or matching Claude Code skills that still output to `.er/` in the working tree). Add `.er/` to `.gitignore` if you use this mode.

**Tests** — `ER_STORAGE_ROOT=/tmp/...` redirects managed storage.

TUI and desktop both resolve the active tab’s path via `TabState::er_dir()`; built-in agent spawns write there directly.

### Sidecar files

Filenames are the same in managed storage and repo-local `.er/`:

| File | Written by | Purpose |
|------|------------|---------|
| `review.json` | `/er-review` | Per-file risk levels, inline findings |
| `order.json` | `/er-review` | Suggested review order |
| `checklist.json` | `/er-review` | Review checklist items |
| `summary.md` | `/er-summary` | Markdown summary of changes |
| `triage.json` | `/er-triage` | Fast branch scan / routing |
| `professor.json` | `/er-professor` | Learning / teaching insights |
| `experts/*.json` | `/er-review-*` experts | Domain-specific expert findings |
| `questions.json` | `er` (`q`/`Q`) | Personal review questions |
| `github-comments.json` | `er` (`c`/`C`) | GitHub PR comments |
| `session.json` | `er` | Session metadata |
| `reviewed` | `er` | Per-file reviewed markers |

Each JSON sidecar stores a SHA-256 `diff_hash` of the diff it was generated against. When the diff changes, AI data is dimmed with a stale warning.

Claude Code skills in this repo still document output as `.er/<file>` relative to the repo; with default managed storage, run reviews through `er`’s agent commands (desktop/TUI) or use `ER_REPO_LOCAL=1` so skill output lands where `er` reads it.

## Development

Cargo workspace at the repo root:

| Crate / package | Output | Role |
|-----------------|--------|------|
| `er-engine` | library | Core logic (git, AI sidecars, state) — shared by TUI and desktop |
| `er-tui` | `er` binary | Terminal UI |
| `er-desktop` | `er-desktop` binary | Tauri desktop shell |
| `desktop-ui` | Vite bundle | Svelte frontend (bundled by Tauri at build time) |

Run Rust commands from the **repo root** unless noted. Prefer scoped builds — compiling the whole workspace pulls in Tauri and can bloat `target/` to tens of GB.

### Cargo aliases

Defined in [`.cargo/config.toml`](.cargo/config.toml). Each alias runs [`cargo er <task>`](.cargo/bin/cargo-er), which execs the matching script. Add `.cargo/bin` to your `PATH` once (e.g. [`direnv allow`](.envrc) if you use direnv, or `export PATH="$PWD/.cargo/bin:$PATH"`).

| Alias | Script | Use for |
|-------|--------|---------|
| `cargo tui-build` | `scripts/er-tui.sh build -p er-tui` | Dev build of terminal `er` → `target/tui/` |
| `cargo tui-release` | `scripts/er-tui.sh build --release -p er-tui` | Release build → `target/tui/release/er` |
| `cargo tui-install` | `scripts/er-tui.sh install --path crates/er-tui` | Install `er` to `~/.cargo/bin` |
| `cargo tui-test` | `scripts/er-tui.sh test -p er-engine -p er-tui` | Fast tests (no desktop) |
| `cargo tui-run` | `scripts/er-tui.sh run -p er-tui` | Run TUI from repo root |
| `cargo tui …` | `scripts/er-tui.sh …` | Passthrough to scoped cargo |
| `cargo er-dev` | `scripts/tauri-dev.sh` | Desktop dev (`target/desktop/`) |
| `cargo desktop-release` | `scripts/tauri-build.sh` | Desktop `.app` + `.dmg` bundle |

Extra args append: `cargo tui-build --release`, `cargo er-dev --logs arena`.

### Wrapper scripts (recommended)

| Script | `CARGO_TARGET_DIR` | Purpose |
|--------|-------------------|---------|
| [`scripts/er-tui.sh`](scripts/er-tui.sh) | `target/tui` | TUI / engine builds and tests |
| [`scripts/tauri-dev.sh`](scripts/tauri-dev.sh) | `target/desktop` | Tauri dev server |
| [`scripts/tauri-build.sh`](scripts/tauri-build.sh) | `target/desktop` | Desktop release bundle (`.app` + `.dmg`) |
| [`scripts/cargo-gc.sh`](scripts/cargo-gc.sh) | — | Prune bloated `target/debug` (auto-run from dev scripts) |

```bash
./scripts/er-tui.sh build -p er-tui
./scripts/er-tui.sh test -p er-engine -p er-tui
./scripts/er-tui.sh run -p er-tui -- --filter '*.rs'
./scripts/tauri-dev.sh --logs arena
./scripts/cargo-gc.sh              # manual reclaim; --force also drops target/desktop
```

### Per-crate: local dev

#### `er-engine` (library)

No binary — build via dependents or check directly.

```bash
# Dev
cargo check -p er-engine
cargo test -p er-engine
./scripts/er-tui.sh test -p er-engine

# Debug logging (git diffs)
ER_DEBUG=1 ./scripts/er-tui.sh run -p er-tui   # overwrites /tmp/er_debug.log per diff call
```

#### `er-tui` (terminal `er`)

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

#### `er-desktop` (Tauri app)

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

Rust logs `er-desktop kind=…` to stderr; the webview logs `[er-profile]` to devtools console. See [`crates/er-desktop/agent.md`](crates/er-desktop/agent.md) for log groups.

#### `desktop-ui` (frontend only)

Storybook / Vite without the Rust backend:

```bash
cd desktop-ui
npm install
npm run dev          # http://localhost:5183
npm run storybook    # http://localhost:6006
npm run check        # svelte-check
```

### Per-crate: release build

#### `er-tui` → `er` binary

```bash
./scripts/er-tui.sh build --release -p er-tui
# Binary: target/tui/release/er

cargo build --release -p er-tui
# Binary: target/release/er
```

Release profile uses LTO + strip ([`Cargo.toml`](Cargo.toml) `[profile.release]`). First release build is slow; incremental rebuilds are faster.

#### `er-desktop` → desktop app bundle

```bash
./scripts/tauri-build.sh              # or: cargo desktop-release (target/desktop)
cargo desktop-release

# macOS output (after build):
#   target/.../bundle/macos/Easy Review.app
#   target/.../bundle/dmg/Easy Review_*.dmg
# The DMG opens in Finder with Easy Review.app beside an Applications shortcut — drag to install.
# Set ER_SKIP_OPEN_DMG=1 to skip auto-open (CI). Not installed automatically.
```

Requires `cargo-tauri` and platform deps (WebKit/GTK on Linux). Not part of the published GitHub Release today — only the `er` TUI binary is released.

#### `desktop-ui` → static assets

```bash
cd desktop-ui
npm run build        # dist/ for Vite production bundle
```

### Whole workspace

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

### Releasing the TUI (`er`)

Published releases are **terminal `er` only**, built by [`.github/workflows/release.yml`](.github/workflows/release.yml) on tag push.

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

### `target/` hygiene

`cargo test` / `cargo build` without `-p` compile **er-desktop** into shared `target/`, which can grow to tens of GB (stale `debug/deps` files). Mitigations:

- TUI work → `./scripts/er-tui.sh` (`target/tui`)
- Desktop work → `./scripts/tauri-dev.sh` (`target/desktop`)
- Reclaim disk → `./scripts/cargo-gc.sh` (prunes when `target/debug` exceeds ~8 GB or 80k files)

### Repo utility scripts

Run from a **git repo** (or repo root for path-relative tools):

| Script | Purpose |
|--------|---------|
| [`scripts/er-tui.sh`](scripts/er-tui.sh) | TUI-scoped `cargo` wrapper (`target/tui`, auto GC) |
| [`scripts/tauri-dev.sh`](scripts/tauri-dev.sh) | Desktop dev launcher (`target/desktop`, `ER_LOG`) |
| [`scripts/cargo-gc.sh`](scripts/cargo-gc.sh) | Prune bloated Cargo `target/` artifacts |
| [`scripts/generate-test-fixtures`](scripts/generate-test-fixtures) | Create sample `.er/` AI files from the current diff (for testing overlays) |
| [`scripts/er-hash-files`](scripts/er-hash-files) | Per-file SHA-256 hashes from a `git diff` (writes `.er/diff-tmp`) |
| [`scripts/er-freshness-check`](scripts/er-freshness-check) | Validate base branch + diff hash (used by skills) |
| [`scripts/er-cleanup-reviews`](scripts/er-cleanup-reviews) | Remove `.er/review.json`, checklist, summary, etc. |
| [`scripts/er-cleanup-questions`](scripts/er-cleanup-questions) | Remove `.er/questions.json` |

Example:

```bash
cd your-repo
bash /path/to/easy-review/scripts/generate-test-fixtures
```

## Requirements

- **git** (required) — `er` shells out to git for all diff operations
- **gh** (optional) — GitHub CLI for PR integration and comment sync. Install from [cli.github.com](https://cli.github.com)
- **Rust 1.85+** (build only) — workspace needs a recent stable toolchain; not needed if using the install script
- **Node.js** (desktop UI only) — for `desktop-ui` and Tauri dev

## AI providers & your data

`er` is a **viewer and orchestrator** for code reviews — it does not run AI models, and it does not transmit your code anywhere. There is no telemetry, no analytics, and no backend: `er` shells out to your local `git` (and optionally `gh`), and reads review artifacts that AI tools you run yourself write into `.er/`.

You bring your own AI tools and credentials (Claude Code, OpenAI Codex, Cursor, etc.). When you use them to generate reviews, **you** are the customer of those providers and are responsible for complying with each provider's terms of service and usage policies. `er` simply renders what they produce.

## License

MIT
