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

- **`git/`** — Shells out to `git diff` and parses unified diff format into structured data (`DiffFile` → `DiffHunk` → `DiffLine`). Handles base branch auto-detection: upstream tracking → main → master → develop → dev. Also provides staging (file + hunk level) and worktree listing. Includes two-phase lazy parsing for large diffs (header-only scan + on-demand file parse) and auto-compaction of low-value files (lock files, generated code).
- **`watch/`** — File system watcher using `notify` + `notify-debouncer-mini`. 500ms debounce. Filters out `.git/` directory changes. Sends events via `std::sync::mpsc` channel.
- **`app/`** — All application state in one `App` struct. Three diff modes (Branch, Unstaged, Staged). Three input modes (Normal, Search, Comment). File/hunk/line navigation, AI state management, comment persistence, watch notifications.
- **`ui/`** — Ratatui rendering. Four view modes: Default (2-col), Overlay (2-col + inline AI banners), SidePanel (3-col with AI panel), AiReview (full-screen dashboard). Cool blue-undertone dark theme in `styles.rs`. Viewport-based rendering for diff and file tree (only builds `Line` objects for visible rows). Syntax highlighting is cached by content hash.
- **`ai/`** — Data model and file loader for AI-generated review artifacts. Reads `.er-*.json` sidecar files written by external Claude Code skills. Manages staleness detection via SHA-256 diff hashing. Does NOT run AI — reads AI output.
- **`github.rs`** — GitHub CLI (`gh`) wrapper for PR integration. Parses PR URLs, checks out PR branches, resolves base branches. No API token needed — uses `gh auth`.

The event loop in `main.rs` polls for keyboard input (100ms timeout) and checks for file watch events each tick. Watch events are debounced (200ms) to batch rapid changes. No async runtime needed — crossterm polling + mpsc channels.

## Key Design Decisions

- **Shell out to git, not gitoxide.** Simpler, proven, handles all edge cases. Git is always available. Can optimize later if profiling shows it matters.
- **Sync event loop, not async.** Crossterm's polling + mpsc channels are sufficient. No async runtime needed.
- **File watching in v1.** This is a core feature, not a nice-to-have. The whole point is following AI-generated changes live.
- **Auto-detect base branch.** Users shouldn't have to specify it. Upstream tracking → main → master → develop fallback chain.
- **Sidecar file pattern for AI data.** `.er-*.json` files sit in the repo root, written by external Claude Code skills, read by `er`. Decoupled: skills can run from any terminal/CI, `er` picks up results via mtime polling (~1s).
- **Staleness via diff hashing.** Each `.er-*` file stores the SHA-256 hash of the diff it was generated against. When the diff changes, the UI warns that AI data may be out of date.
- **`gh` CLI for GitHub, not HTTP API.** No API token management. Users already have `gh auth login` configured.
- **One `er` instance per worktree.** Multi-worktree tabs work via `t` key (worktree picker).
- **Auto-compact low-value files.** Lock files, generated code, minified assets, and files above 500 lines are compacted automatically — hunks cleared from memory, expandable on demand via `Enter`. Pattern list in `CompactionConfig::default()`.
- **Two-phase lazy parsing for large diffs.** Diffs above 5,000 lines use a header-only scan (`parse_diff_headers`) for the file list, then parse individual files on demand when navigated to. Keeps raw diff string + byte offsets for instant re-parse.
- **Viewport-based rendering.** Both diff view and file tree only build `Line`/`ListItem` objects for the visible terminal rows (+ 20-line buffer). A 5,000-line file builds ~60 Lines instead of 5,000.
- **Precomputed hunk offsets.** `HunkOffsets` gives O(1) scroll position lookup (vs O(n) hunk iteration). Rebuilt on file selection.
- **Syntax highlighting cache.** Content+filename hash → cached spans. 10K entry limit, full eviction on overflow. High hit rate since most lines don't change between frames.
- **Fast diff hash for watch events.** `DefaultHasher` (non-cryptographic) for internal change detection during watch-mode refreshes. SHA-256 only used for `.er-review.json` compatibility where hashes are persisted.
- **Deduplicated git calls.** `refresh_diff_impl` guarantees at most 2 `git diff` invocations per refresh (down from 3) by reusing the raw output for both parsing and hash computation.

## Code Conventions

- Module structure: each directory has `mod.rs` for exports, separate files for implementation. Each module has its own `CLAUDE.md`.
- Error handling: `anyhow::Result` everywhere. Bubble up with `?`, context with `.context()`.
- Git commands: all in `git/status.rs`. Always pass `--no-color` and `--no-ext-diff` (bypasses difftastic/delta).
- GitHub commands: all in `github.rs`. Shell out to `gh` CLI, never use HTTP API directly.
- UI styles: all colors and composed styles in `ui/styles.rs`. Don't use raw colors elsewhere.
- Syntax highlighting: `syntect` crate via `ui/highlight.rs`. Highlighter is created once in main and passed as `&mut` to diff_view (mutable for cache writes). Uses `base16-ocean.dark` theme. Language detection is automatic from filename. Results are cached by content hash.
- AI sidecar files: `.er-*` in repo root, gitignored. `er` reads all, writes only `.er-feedback.json`. Atomic writes via tmp+rename.
- Diff parsing: the parser in `git/diff.rs` has unit tests. Run them with `cargo test`. For large diffs, `parse_diff_headers()` provides a fast header-only scan; `parse_file_at_offset()` parses a single file on demand.
- Compaction: `DiffFile.compacted` flag indicates a file whose hunks have been cleared. `compact_files()` applies pattern + size threshold. `expand_compacted_file()` re-fetches from git on demand. Glob matching via `glob_match()` in `diff.rs`.
- Performance state: `HunkOffsets`, `MemoryBudget`, `FileTreeCache`, and `lazy_mode` flag live on `TabState`. The `ensure_file_parsed()` method triggers on-demand parsing in lazy mode.

## File Map

```
src/main.rs              Event loop, CLI parsing (clap), input routing, debounced watch refresh
src/app/state.rs         App struct, all state, navigation, comments, HunkOffsets, MemoryBudget, lazy parsing
src/git/diff.rs          parse_diff(), parse_diff_headers(), compact_files(), expand_compacted_file()
src/git/status.rs        detect_base_branch(), git_diff_raw(), git_diff_raw_file(), staging, worktrees
src/github.rs            GitHub PR URL parsing, gh CLI wrapper
src/ai/review.rs         AI data model (AiState, ErReview, Finding, ViewMode)
src/ai/loader.rs         .er-* file loading, SHA-256 + fast diff hashing, mtime polling
src/watch/mod.rs         FileWatcher — debounced notify watcher
src/ui/mod.rs            draw() — ViewMode-based layout dispatch
src/ui/styles.rs         Color constants and style helpers (blue-undertone theme)
src/ui/highlight.rs      Syntect-based syntax highlighting with content-hash cache
src/ui/file_tree.rs      Left panel — virtualized file list with risk indicators
src/ui/diff_view.rs      Right panel — viewport-based rendering, compacted file view
src/ui/ai_panel.rs       SidePanel mode — per-file findings + comments column
src/ui/ai_review_view.rs AiReview mode — full-screen risk dashboard + checklist
src/ui/status_bar.rs     Top bar, bottom bar, AI status badges, memory budget (debug), comment input
src/ui/overlay.rs        Modal popups (worktree picker, directory browser)
src/ui/utils.rs          Shared utilities (word_wrap)
```

## Current State

v1.2 with performance hardening. Building locally with `cargo install --path .`. Debug mode via `ER_DEBUG=1 er` writes to `/tmp/er_debug.log` and shows memory budget in the status bar. Test fixtures via `scripts/generate-test-fixtures.sh`.

## Roadmap

**v1 (done):** Branch/unstaged/staged diffs, file+hunk navigation, search, live file watching, auto base branch detection, syntax highlighting (syntect), open-in-editor (`e` key).

**v1.1 (done):** AI review integration (4 view modes, inline findings, comments), GitHub PR support (`--pr` flag, URL arguments), line-level navigation (arrow keys), comment system (`c` key → `.er-feedback.json`).

**v1.2 (current):** Large diff performance — auto-compaction, two-phase lazy parsing, viewport-based rendering, syntax highlight cache, precomputed hunk offsets, debounced watch refresh, deduplicated git calls, fast diff hash, memory budget tracking.

**v2:** Multi-worktree tabs (Tab/Shift+Tab to cycle), per-worktree state, cross-worktree watch notifications.
