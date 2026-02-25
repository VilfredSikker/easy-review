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

No runtime dependencies beyond git. Single binary. (`gh` CLI optional for GitHub PR features.)

## Architecture

Rust + Ratatui TUI. Five modules + a standalone GitHub integration file:

- **`git/`** — Shells out to `git diff` and parses unified diff format into structured data (`DiffFile` → `DiffHunk` → `DiffLine`). Handles base branch auto-detection: upstream tracking → main → master → develop → dev. Also provides staging (file + hunk level), worktree listing, and commit log loading (`git log` + `git diff` for individual commits).
- **`watch/`** — File system watcher using `notify` + `notify-debouncer-mini`. 500ms debounce. Filters out `.git/` directory changes. Sends events via `std::sync::mpsc` channel.
- **`app/`** — All application state in one `App` struct. Four diff modes (Branch, Unstaged, Staged, History). Three input modes (Normal, Search, Comment). File/hunk/line navigation, AI state management, comment persistence, watch notifications. History mode has its own `HistoryState` with commit list, per-commit diff cache (LRU, 5 entries), and lazy loading.
- **`ui/`** — Ratatui rendering. Four view modes: Default (2-col), Overlay (2-col + inline AI banners), SidePanel (3-col with AI panel), AiReview (full-screen dashboard). Cool blue-undertone dark theme in `styles.rs`.
- **`ai/`** — Data model and file loader for AI-generated review artifacts. Reads `.er-*.json` sidecar files written by external Claude Code skills. Manages staleness detection via SHA-256 diff hashing. Does NOT run AI — reads AI output.
- **`github.rs`** — GitHub CLI (`gh`) wrapper for PR integration. Parses PR URLs, checks out PR branches, resolves base branches. No API token needed — uses `gh auth`.

The event loop in `main.rs` polls for keyboard input (100ms timeout) and checks for file watch events each tick. No async runtime needed — crossterm polling + mpsc channels.

## Key Design Decisions

- **Shell out to git, not gitoxide.** Simpler, proven, handles all edge cases. Git is always available. Can optimize later if profiling shows it matters.
- **Sync event loop, not async.** Crossterm's polling + mpsc channels are sufficient. No async runtime needed.
- **File watching in v1.** This is a core feature, not a nice-to-have. The whole point is following AI-generated changes live.
- **Auto-detect base branch.** Users shouldn't have to specify it. Upstream tracking → main → master → develop fallback chain.
- **Sidecar file pattern for AI data.** `.er-*.json` files sit in the repo root, written by external Claude Code skills, read by `er`. Decoupled: skills can run from any terminal/CI, `er` picks up results via mtime polling (~1s).
- **Staleness via diff hashing.** Each `.er-*` file stores the SHA-256 hash of the diff it was generated against. When the diff changes, the UI warns that AI data may be out of date.
- **`gh` CLI for GitHub, not HTTP API.** No API token management. Users already have `gh auth login` configured.
- **One `er` instance per worktree.** Multi-worktree tabs work via `t` key (worktree picker).

## Code Conventions

- Module structure: each directory has `mod.rs` for exports, separate files for implementation. Each module has its own `CLAUDE.md`.
- Error handling: `anyhow::Result` everywhere. Bubble up with `?`, context with `.context()`.
- Git commands: all in `git/status.rs`. Always pass `--no-color` and `--no-ext-diff` (bypasses difftastic/delta).
- GitHub commands: all in `github.rs`. Shell out to `gh` CLI, never use HTTP API directly.
- UI styles: all colors and composed styles in `ui/styles.rs`. Don't use raw colors elsewhere.
- Syntax highlighting: `syntect` crate via `ui/highlight.rs`. Highlighter is created once in main and passed through to diff_view. Uses `base16-ocean.dark` theme. Language detection is automatic from filename.
- AI sidecar files: `.er-*` in repo root, gitignored. `er` reads all, writes only `.er-feedback.json`. Atomic writes via tmp+rename.
- Diff parsing: the parser in `git/diff.rs` has unit tests. Run them with `cargo test`.

## File Map

```
src/main.rs              Event loop, CLI parsing (clap), input routing
src/app/state.rs         App struct, all state, navigation, comments, HistoryState, DiffCache
src/git/diff.rs          parse_diff() — unified diff text → Vec<DiffFile>
src/git/status.rs        detect_base_branch(), git_diff_raw(), staging, worktrees, git_log_branch(), git_diff_commit()
src/github.rs            GitHub PR URL parsing, gh CLI wrapper
src/ai/review.rs         AI data model (AiState, ErReview, Finding, ViewMode)
src/ai/loader.rs         .er-* file loading, SHA-256 diff hashing, mtime polling
src/watch/mod.rs         FileWatcher — debounced notify watcher
src/ui/mod.rs            draw() — ViewMode-based layout dispatch
src/ui/styles.rs         Color constants and style helpers (blue-undertone theme)
src/ui/highlight.rs      Syntect-based syntax highlighting for diff lines
src/ui/file_tree.rs      Left panel — file list with risk indicators (or commit list in History mode)
src/ui/diff_view.rs      Right panel — hunks, line numbers, AI finding/comment banners (or multi-file commit diff in History mode)
src/ui/ai_panel.rs       SidePanel mode — per-file findings + comments column
src/ui/ai_review_view.rs AiReview mode — full-screen risk dashboard + checklist
src/ui/status_bar.rs     Top bar, bottom bar, AI status badges, comment input
src/ui/overlay.rs        Modal popups (worktree picker, directory browser)
src/ui/utils.rs          Shared utilities (word_wrap)
```

## Current State

v1.2 with commit history. Building locally with `cargo install --path .`. Debug mode via `ER_DEBUG=1 er` writes to `/tmp/er_debug.log`. Test fixtures via `scripts/generate-test-fixtures.sh`.

## Roadmap

**v1 (done):** Branch/unstaged/staged diffs, file+hunk navigation, search, live file watching, auto base branch detection, syntax highlighting (syntect), open-in-editor (`e` key).

**v1.1 (done):** AI review integration (4 view modes, inline findings, comments), GitHub PR support (`--pr` flag, URL arguments), line-level navigation (arrow keys), comment system (`c` key → `.er-feedback.json`).

**v1.2 (current):** Commit history mode (`4` key). Left panel shows commit list, right panel shows full multi-file diff of selected commit. j/k navigates commits, n/N navigates files within a commit, ↑/↓ navigates lines. LRU diff cache (5 entries), lazy loading (50 commits per batch), commit search by subject/hash/author.

**v2:** Multi-worktree tabs (Tab/Shift+Tab to cycle), per-worktree state, cross-worktree watch notifications.
