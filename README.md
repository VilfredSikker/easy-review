# easy-review (`er`)

A git diff review tool built for developers who use AI coding assistants. Ships as a terminal TUI (`er`), with a Tauri desktop app in development.

AI writes code faster than you can review it. `er` makes review fast, navigable, and live-updating.

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)

📚 **Documentation:** the full guide lives in [`docs/guide/`](docs/guide/index.html) — getting started, core concepts shared by both apps, and dedicated sections for the [terminal UI](docs/guide/tui.html) and the [desktop app](docs/guide/desktop.html). When published via GitHub Pages it is served from the site root under `/guide/`.

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

1. Open the **AI Hub** — TUI: press `a`; Desktop: <kbd>Cmd+A</kbd> (macOS) / <kbd>Ctrl+A</kbd> (Windows/Linux)
2. Run a review — TUI: select **Review work**; Desktop: select **Run review** — findings appear inline once complete
3. Press `v` / `V` in the TUI to cycle AI view modes; Desktop switches views in the sidebar
4. Leave questions (`q`) or comments (`c`). In the TUI, open the Hub and select **Answer questions** to get AI responses. On Desktop, questions are managed in the Notes panel (no Hub action for answering)
5. Watch mode is on by default — diffs refresh automatically as code changes

## Features

- **Live watch mode** — Auto-refreshes on file edits, staging, and commits. Reviewed files auto-unmark when their diff changes.
- **AI-powered review** — Open the **AI Hub** (`a` / <kbd>Cmd+A</kbd>) to run a full review (TUI: **Review work** / Desktop: **Run review**), a fast **Triage branch** scan, one of eight **Specialized review** lenses (Security, Performance, Reliability, Testing, API/Contracts, Patterns, Simplifying, Mentorship), or a **Professor** session for learning insights. Generates per-file risk levels, inline findings, and a review checklist. Four AI view modes: Default, Overlay, Side Panel, AI Review.
- **Two comment types** — Personal questions (`q`/`Q`, yellow) for your own notes, and GitHub comments (`c`/`C`, cyan) for PR discussions. Reply with `r`, delete with `d`.
- **GitHub PR sync** — Pull review comments with `G`, push yours back with `P`. Two-way sync via `gh` CLI.
- **Six diff modes** — Branch diff (`1`), unstaged (`2`), staged (`3`), commit history (`4`), merge conflicts (`5`), hidden/ignored files (`6`). Sort by recency with `Shift+R`.
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
| `5` | Merge conflicts |
| `6` | Hidden / ignored files |
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
| `review.json` | AI Hub: **Run review** | Per-file risk levels, inline findings |
| `order.json` | AI Hub: **Run review** | Suggested review order |
| `checklist.json` | AI Hub: **Run review** | Review checklist items |
| `summary.md` | AI Hub: **Run review** (TUI also has standalone **Generate summary**) | Markdown summary of changes |
| `triage.json` | AI Hub: **Triage branch** | Fast branch scan / routing |
| `professor.json` | AI Hub: **Professor** | Learning / teaching insights |
| `experts/*.json` | AI Hub: **Specialized review** | Domain-specific expert findings |
| `questions.json` | `er` (`q`/`Q`) | Personal review questions |
| `github-comments.json` | `er` (`c`/`C`) | GitHub PR comments |
| `session.json` | `er` | Session metadata |
| `reviewed` | `er` | Per-file reviewed markers |

Each JSON sidecar stores a SHA-256 `diff_hash` of the diff it was generated against. When the diff changes, AI data is dimmed with a stale warning.

The AI Hub (TUI: `a` / Desktop: <kbd>Cmd+A</kbd>) writes all review artifacts directly to managed storage — no manual steps needed. If you use external tools that write to `<repo>/.er/` in the working tree, set `ER_REPO_LOCAL=1` so `er` reads from there instead, or import via TUI `I` / Desktop **Import local review files**.

## Development

Cargo workspace at the repo root:

| Crate / package | Output | Role |
|-----------------|--------|------|
| `er-engine` | library | Core logic (git, AI sidecars, state) — shared by TUI and desktop |
| `er-tui` | `er` binary | Terminal UI |
| `er-desktop` | `er-desktop` binary | Tauri desktop shell |
| `desktop-ui` | Vite bundle | Svelte frontend (bundled by Tauri at build time) |

```bash
./scripts/er-tui.sh build -p er-tui            # build the TUI (isolated target/tui)
./scripts/er-tui.sh test -p er-engine -p er-tui # fast tests (no desktop)
./scripts/tauri-dev.sh                          # desktop dev server
```

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the full guide: cargo aliases, per-crate builds, release process, profiling, and `target/` hygiene.

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
