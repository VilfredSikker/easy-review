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
cargo install --path .
```

Requires Rust 1.70+. No runtime dependencies beyond git. Single binary.

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
- **Syntax highlighting** — Language-aware coloring via syntect with content-hash caching.
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

`er` reads `.er/` sidecar files written by Claude Code skills:

| File | Written by | Purpose |
|------|------------|---------|
| `.er/review.json` | `/er-review` | Per-file risk levels, inline findings |
| `.er/checklist.json` | `/er-review` | Review checklist items |
| `.er/summary.md` | `/er-summary` | Markdown summary of changes |
| `.er/questions.json` | `er` | Your personal review questions (`q`/`Q`) |
| `.er/github-comments.json` | `er` | GitHub PR comments (`c`/`C`) |

Each file stores a SHA-256 hash of the diff it was generated against. When the diff changes, AI data is dimmed with a stale warning.

Add `.er/` to your `.gitignore`.

## Requirements

- **git** (required) — `er` shells out to git for all diff operations
- **gh** (optional) — GitHub CLI for PR integration and comment sync. Install from [cli.github.com](https://cli.github.com)
- **Rust 1.70+** (build only) — not needed if using the install script

## License

MIT
