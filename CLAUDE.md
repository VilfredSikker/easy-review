# CLAUDE.md — easy-review (`er`)

## What This Project Is

A terminal TUI for reviewing git diffs, designed for developers who use AI coding tools (Claude Code). The core insight: AI writes code faster than humans can review it, so the review tool needs to be fast, navigable, and live-updating.

Binary name is `er`. Run it from any git repo.

## Build & Run

```bash
cargo build --release
cargo install --path .
er                    # run from any git repo
```

No runtime dependencies beyond git. Single binary.

## Architecture

Rust + Ratatui TUI. Four modules:

- **`git/`** — Shells out to `git diff` and parses unified diff format into structured data (`DiffFile` → `DiffHunk` → `DiffLine`). Also handles base branch auto-detection: checks upstream tracking first (`@{upstream}`), then falls back to main → master → develop → dev.
- **`watch/`** — File system watcher using `notify` + `notify-debouncer-mini`. 500ms debounce. Filters out `.git/` directory changes. Sends events via `std::sync::mpsc` channel.
- **`app/`** — All application state in one `App` struct. Three diff modes (Branch, Unstaged, Staged). Two input modes (Normal, Search). Handles file navigation, hunk navigation, scrolling, search filtering, watch notifications.
- **`ui/`** — Ratatui rendering. Layout: top bar (1 row) + main area (file tree 32 cols | diff view) + bottom bar (1 row). Dark color scheme defined in `styles.rs`.

The event loop in `main.rs` polls for keyboard input (100ms timeout) and checks for file watch events each tick. No async runtime needed — crossterm polling + mpsc channels.

## Key Design Decisions

- **Shell out to git, not gitoxide.** Simpler, proven, handles all edge cases. Git is always available. Can optimize later if profiling shows it matters.
- **Sync event loop, not async.** Crossterm's polling + mpsc channels are sufficient. Tokio is in Cargo.toml for future use but not wired up yet.
- **File watching in v1.** This is a core feature, not a nice-to-have. The whole point is following AI-generated changes live.
- **Auto-detect base branch.** Users shouldn't have to specify it. Upstream tracking → main → master → develop fallback chain.
- **One `er` instance per worktree.** Multi-worktree tabs are planned for v2 but not in current scope.

## Code Conventions

- Module structure: each directory has `mod.rs` for exports, separate files for implementation.
- Error handling: `anyhow::Result` everywhere. Bubble up with `?`, context with `.context()`.
- Git commands: all in `git/status.rs`. Always pass `--no-color` and `--no-ext-diff` (bypasses difftastic/delta).
- UI styles: all colors and composed styles in `ui/styles.rs`. Don't use raw colors elsewhere.
- Syntax highlighting: `syntect` crate via `ui/highlight.rs`. Highlighter is created once in main and passed through to diff_view. Uses `base16-ocean.dark` theme. Language detection is automatic from filename.
- Diff parsing: the parser in `git/diff.rs` has unit tests. Run them with `cargo test`.

## File Map

```
src/main.rs              Event loop, terminal setup/teardown, input handling
src/app/state.rs         App struct, all state, navigation, open_in_editor()
src/git/diff.rs          parse_diff() — unified diff text → Vec<DiffFile>
src/git/status.rs        detect_base_branch(), git_diff_raw(), git_diff_stat()
src/watch/mod.rs         FileWatcher — debounced notify watcher
src/ui/mod.rs            draw() — top-level layout composition
src/ui/styles.rs         Color constants and style helpers
src/ui/highlight.rs      Syntect-based syntax highlighting for diff lines
src/ui/file_tree.rs      Left panel renderer
src/ui/diff_view.rs      Right panel renderer (hunks, line numbers, syntax hl)
src/ui/status_bar.rs     Top bar, bottom bar, watch notification overlay
```

## Current State

v1 feature-complete. Building locally with `cargo install --path .`. Debug mode via `ER_DEBUG=1 er` writes to `/tmp/er_debug.log`.

## Roadmap

**v1 (current):** Branch/unstaged/staged diffs, file+hunk navigation, search, live file watching, auto base branch detection, syntax highlighting (syntect), open-in-editor (`e` key).

**v2:** Multi-worktree tabs (Tab/Shift+Tab to cycle), per-worktree state, cross-worktree watch notifications.

**v3:** Claude Code integration hooks.
