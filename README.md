# easy-review (`er`)

A terminal-based git diff review tool built for developers who work with AI coding assistants.

Reviewing is the bottleneck — not coding. `er` makes review fast, visual, and live.

## The Problem

When working with Claude Code (or similar AI tools), code gets written faster than you can review it. You need to see what changed across the whole branch, navigate between files and hunks quickly, and follow changes in real-time as the AI writes code.

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

# In a worktree
cd ~/worktrees/feature-branch
er
```

For the best workflow with AI coding tools, split your terminal (Ghostty, tmux, zellij) with Claude Code on one side and `er` on the other. Press `w` to enable watch mode — diffs refresh automatically as files change.

## Features

- **Three diff modes** — Branch diff, unstaged changes, staged changes
- **Syntax highlighting** — Language-aware coloring via syntect
- **Live watch mode** — Auto-refreshes when files change on disk
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
y                 Yank (copy) current hunk
e                 Open file in $EDITOR
r                 Refresh diff
w                 Toggle live watch mode
/                 Search / filter files
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
├── main.rs           Entry point, event loop, terminal setup
├── app/
│   ├── mod.rs        Module exports
│   └── state.rs      App state, navigation, mode switching
├── git/
│   ├── mod.rs        Module exports
│   ├── diff.rs       Unified diff parser (raw text → structured data)
│   └── status.rs     Base branch detection, staging, git commands
├── ui/
│   ├── mod.rs        Layout coordinator (splits, composition)
│   ├── styles.rs     Color scheme and style definitions
│   ├── highlight.rs  Syntax highlighting (syntect)
│   ├── file_tree.rs  Left panel — file list with status indicators
│   ├── diff_view.rs  Right panel — diff with line numbers and hunks
│   ├── overlay.rs    Modal overlays (directory browser, worktree picker)
│   └── status_bar.rs Top bar (tabs, branch, modes), bottom bar (keybinds)
└── watch/
    └── mod.rs        Debounced file watcher (notify crate, 500ms)
```

**Stack:** Rust, Ratatui, Crossterm, syntect, notify. Shells out to `git diff` for diff generation. Single binary, fast startup, no runtime dependencies beyond git.
