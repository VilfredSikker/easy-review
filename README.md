# easy-review (`er`)

A terminal-based git diff review tool with AI-powered analysis, built for developers who work with AI coding assistants.

Reviewing is the bottleneck — not coding. `er` makes review fast, visual, and AI-assisted.

## The Problem

When working with Claude Code (or similar AI tools), code gets written faster than you can review it. You need to see what changed across the whole branch, navigate between files and hunks quickly, get AI risk analysis on the changes, leave comments, and follow changes in real-time as the AI writes code.

## Install

```bash
cargo install --path .
```

Requires Rust 1.70+. The binary is called `er`.

## Usage

Run `er` from any git repository. It auto-detects the base branch from upstream tracking, falling back to main/master/develop.

```bash
# In any git repo
er

# Open a GitHub PR directly
er https://github.com/owner/repo/pull/42
er --pr 42

# Open multiple repos/worktrees as tabs
er ~/projects/api ~/projects/frontend
```

### AI Review Workflow

1. Split your terminal with Claude Code on one side and `er` on the other
2. Run `/er-review` in Claude Code to generate AI analysis
3. Press `v` in `er` to toggle AI overlay — findings appear inline in the diff
4. Press `c` to comment on findings — AI responds on next `/er-review` run
5. Press `w` to enable watch mode — diffs refresh automatically as files change

## Features

- **AI-powered review** — Run `/er-review` in Claude Code, get per-file risk levels, inline findings, and a review checklist
- **Four view modes** — Default (clean diff), Overlay (inline AI banners), Side Panel (3-column with AI panel), AI Review (full-screen dashboard)
- **Comment & feedback loop** — Press `c` to comment on lines/hunks, reply to threads with `r`, delete with `d`. Comments render inline after their target line or after the hunk
- **GitHub PR comment sync** — Pull existing PR review comments into `er` with `G`, push your comments back with `P`. Two-way sync via `gh` CLI
- **GitHub PR integration** — Open PRs directly: `er --pr 42` or `er <github-url>`
- **Three diff modes** — Branch diff, unstaged changes, staged changes
- **Line-level navigation** — Arrow keys move through individual diff lines within hunks
- **Syntax highlighting** — Language-aware coloring via syntect
- **Live watch mode** — Auto-refreshes when files change on disk; AI data reloads automatically
- **Multi-repo tabs** — Open multiple repos or worktrees side-by-side
- **Hunk staging** — Stage individual files or hunks without leaving the TUI
- **Review tracking** — Mark files as reviewed, filter to unreviewed only
- **File search** — Fuzzy filter the file list
- **Directory browser** — Open any repo on disk via `o`
- **Worktree picker** — Switch between worktrees via `t`
- **Yank to clipboard** — Copy the current hunk with `y`
- **Editor integration** — Jump to the current file in `$EDITOR` with `e`
- **Responsive layout** — Top and bottom bars adapt to terminal width

## Keybindings

### Navigation

```
j / k             Next / prev file
n / N             Next / prev hunk
↓ / ↑             Next / prev line (within hunks)
h / l             Scroll left / right
Ctrl-d / Ctrl-u   Scroll half page down / up
```

### Diff Modes

```
1                 Branch diff (vs base branch)
2                 Unstaged changes
3                 Staged changes
```

### Actions

```
s                 Stage / unstage file
S                 Stage current hunk
Space             Toggle file as reviewed
u                 Filter to unreviewed files only
c                 Comment on current line
C                 Comment on current hunk
y                 Yank (copy) current hunk
e                 Open file in $EDITOR
r                 Refresh diff (or reply when comment focused)
w                 Toggle live watch mode
/                 Search / filter files
```

### Comments

```
Tab               Toggle comment focus mode
↓ / ↑             Navigate between comments (when focused)
r                 Reply to focused comment
d                 Delete focused comment
R                 Toggle resolved on focused comment
```

### GitHub Sync (requires `gh` CLI)

```
G                 Pull PR review comments from GitHub
P                 Push local comments to GitHub PR
```

### AI Views

```
v / V             Cycle AI view mode forward / backward
                  (Default → Overlay → Side Panel → AI Review)
```

In AI Review mode:

```
j / k             Navigate file list or checklist
Tab               Switch focus between Files and Checklist columns
Space             Toggle checklist item
Enter             Jump to file in diff view
Esc               Return to default view
```

### Tabs & Repos

```
]  /  [           Next / prev tab
x                 Close tab
t                 Worktree picker
o                 Directory browser (Backspace to go up)
```

### General

```
Esc               Clear search filter
q                 Quit
```

## Architecture

```
src/
├── main.rs           Entry point, CLI parsing (clap), event loop, input routing
├── app/
│   ├── mod.rs        Module exports
│   └── state.rs      App state, navigation, comments, AI state management
├── git/
│   ├── mod.rs        Module exports
│   ├── diff.rs       Unified diff parser (raw text → structured data)
│   └── status.rs     Base branch detection, staging, git commands
├── github.rs         GitHub PR integration (gh CLI wrapper)
├── ai/
│   ├── mod.rs        Module exports
│   ├── review.rs     AI data model (AiState, findings, view modes)
│   └── loader.rs     .er-* file loading, diff hashing, mtime polling
├── ui/
│   ├── mod.rs        Layout coordinator (ViewMode-based dispatch)
│   ├── styles.rs     Color scheme (blue-undertone dark theme)
│   ├── highlight.rs  Syntax highlighting (syntect)
│   ├── file_tree.rs  Left panel — file list with risk indicators
│   ├── diff_view.rs  Right panel — diff with AI finding/comment banners
│   ├── ai_panel.rs   Side panel — per-file AI findings column
│   ├── ai_review_view.rs  Full-screen AI review dashboard
│   ├── overlay.rs    Modal overlays (directory browser, worktree picker)
│   ├── status_bar.rs Top bar (tabs, AI badges), bottom bar (hints, comment input)
│   └── utils.rs      Shared utilities (word wrapping)
└── watch/
    └── mod.rs        Debounced file watcher (notify crate, 500ms)
```

**Stack:** Rust, Ratatui, Crossterm, syntect, notify, serde/serde_json, sha2, clap. Shells out to `git` for diffs and `gh` for GitHub PRs. Single binary, no runtime dependencies beyond git (gh optional for PR features).

## AI Integration

`er` reads `.er-*.json` sidecar files written by Claude Code skills:

| File | Purpose |
|------|---------|
| `.er-review.json` | Per-file risk levels, findings with hunk anchors |
| `.er-order.json` | Suggested review order with groupings |
| `.er-summary.md` | Markdown summary of overall changes |
| `.er-checklist.json` | Review checklist items |
| `.er-feedback.json` | Your comments and GitHub-synced comments (the only file `er` writes) |

Claude Code skills: `/er-review` (full analysis), `/er-questions` (respond to comments), `/er-risk-sort`, `/er-summary`, `/er-checklist`. See `skills/README.md` for setup.

Staleness detection: each file stores a SHA-256 hash of the diff it was generated against. When the diff changes, `er` dims the AI data and shows a stale warning.
