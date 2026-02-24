# easy-review (`er`)

A terminal-based git diff review tool built for developers who work with AI coding assistants.

Reviewing is the bottleneck — not coding. `er` makes review fast, visual, and live.

## The Problem

When working with Claude Code (or similar AI tools), code gets written faster than you can review it. You need to see what changed across the whole branch, navigate between files and hunks quickly, and follow changes in real-time as the AI writes code.

## Install

```bash
cd easy-review
cargo build --release
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

## Keybindings

```
j / k           Next / prev file
n / N           Next / prev hunk
1 / 2 / 3       Branch diff / Unstaged / Staged
w               Toggle live watch mode
/               Search / filter files
Ctrl-d / Ctrl-u Scroll half page down / up
r               Manual refresh
Esc             Clear search filter
q               Quit
```

## Diff Modes

**Branch (1)** — Full diff of current branch vs base branch. Shows everything you'd be submitting in a PR.

**Unstaged (2)** — Working directory changes not yet staged. What `git diff` shows.

**Staged (3)** — Changes in the index ready to commit. What `git diff --staged` shows.

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
│   └── status.rs     Base branch detection, git command runners
├── ui/
│   ├── mod.rs        Layout coordinator (splits, composition)
│   ├── styles.rs     Color scheme and style definitions
│   ├── file_tree.rs  Left panel — file list with status indicators
│   ├── diff_view.rs  Right panel — diff with line numbers and hunk markers
│   └── status_bar.rs Top bar (branch, modes), bottom bar (keybindings)
└── watch/
    └── mod.rs        Debounced file watcher (notify crate, 500ms)
```

**Stack:** Rust, Ratatui, Crossterm, notify. Shells out to `git diff` for diff generation. Single binary, fast startup, no runtime dependencies beyond git.

## Roadmap

- [x] Interactive HTML prototype (`prototype/index.html`)
- [x] Core Rust scaffolding with all modules
- [ ] First successful compile and basic usage
- [ ] Syntax-aware highlighting (tree-sitter)
- [ ] Multi-worktree support (tab between worktrees)
- [ ] Claude Code integration hooks
