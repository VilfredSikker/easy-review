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

- **`git/`** — Shells out to `git diff` and parses unified diff format into structured data (`DiffFile` → `DiffHunk` → `DiffLine`). Handles base branch auto-detection: upstream tracking → main → master → develop → dev. Also provides staging (file + hunk level), worktree listing, and watched file discovery/diffing.
- **`watch/`** — File system watcher using `notify` + `notify-debouncer-mini`. 500ms debounce. Watches working tree changes plus `.git/index` (staging) and `.git/refs/` (commits). Sends events via `std::sync::mpsc` channel. Starts automatically on launch.
- **`app/`** — All application state in one `App` struct. Three diff modes (Branch, Unstaged, Staged). Six input modes (Normal, Search, Comment, Confirm, Filter, Commit). File/hunk/line navigation, AI state management, comment persistence (with replies and deletion), comment focus navigation, watch notifications. Loads `.er-config.toml` for watched files configuration. Composable filter system (`filter.rs`) with glob, status, and size rules. Mtime sort toggle (`Shift+R`) works in any diff mode.
- **`ui/`** — Ratatui rendering. Four view modes: Default (2-col), Overlay (2-col + inline AI banners), SidePanel (3-col with AI panel), AiReview (full-screen dashboard). Cool blue-undertone dark theme in `styles.rs`.
- **`ai/`** — Data model and file loader for AI-generated review artifacts. Reads `.er-*.json` sidecar files written by external Claude Code skills. Manages staleness detection via SHA-256 diff hashing. Does NOT run AI — reads AI output.
- **`github.rs`** — GitHub CLI (`gh`) wrapper for PR integration. Parses PR URLs, checks out PR branches, resolves base branches, detects open PRs for current branch. Two-way comment sync: pull review comments from GitHub, push local comments back, reply to threads, delete comments. No API token needed — uses `gh auth`.

The event loop in `main.rs` polls for keyboard input (100ms timeout) and checks for file watch events each tick. PR base hint check runs on a background thread to avoid blocking startup. No async runtime needed — crossterm polling + mpsc channels.

## Key Design Decisions

- **Shell out to git, not gitoxide.** Simpler, proven, handles all edge cases. Git is always available. Can optimize later if profiling shows it matters.
- **Sync event loop, not async.** Crossterm's polling + mpsc channels are sufficient. No async runtime needed.
- **File watching in v1.** This is a core feature, not a nice-to-have. The whole point is following AI-generated changes live.
- **Auto-detect base branch.** Users shouldn't have to specify it. Upstream tracking → main → master → develop fallback chain. When `gh` is available and the branch has an open PR targeting a different base, a hint is shown (no auto-switch).
- **Sidecar file pattern for AI data.** `.er-*.json` files sit in the repo root, written by external Claude Code skills, read by `er`. Decoupled: skills can run from any terminal/CI, `er` picks up results via mtime polling (~1s).
- **Staleness via diff hashing.** Each `.er-*` file stores the SHA-256 hash of the diff it was generated against. When the diff changes, the UI warns that AI data may be out of date.
- **`gh` CLI for GitHub, not HTTP API.** No API token management. Users already have `gh auth login` configured.
- **One `er` instance per worktree.** Multi-worktree tabs work via `t` key (worktree picker).
- **Split comment system.** Two separate files: `.er-questions.json` for personal review questions (read by `/er-questions` skill), `.er-github-comments.json` for GitHub PR comments (two-way sync). Questions use `q`/`Q` keys (yellow), comments use `c`/`C` keys (cyan). Questions are private; GitHub comments get pushed via `/er-publish`.
- **Flat single-level comment threads.** GitHub comments can have replies, but replies cannot have replies. Keeps the model simple — parent + N children, no recursive nesting. Questions don't support replies (use `/er-questions` for AI responses).
- **GitHub comments track source and sync state.** Each `GitHubReviewComment` has `source` (local/github), `github_id`, `author`, and `synced` fields. Deduplication on pull uses `github_id`. Push marks comments as synced.
- **Per-comment staleness.** Comments store `line_content` of their target line. When the diff changes, individual comments are marked stale and rendered dimmed with a warning indicator.
- **Watched files for git-ignored paths.** `.er-config.toml` with `[watched]` section specifies glob patterns for files to monitor (e.g., `.work/` agent sync folders). Two diff modes: "content" (show file contents) and "snapshot" (diff against saved baseline). Gitignore safety check warns if watched files aren't ignored.

## Code Conventions

- Module structure: each directory has `mod.rs` for exports, separate files for implementation. Each module has its own `CLAUDE.md`.
- Error handling: `anyhow::Result` everywhere. Bubble up with `?`, context with `.context()`.
- Git commands: all in `git/status.rs`. Always pass `--no-color` and `--no-ext-diff` (bypasses difftastic/delta).
- GitHub commands: all in `github.rs`. Shell out to `gh` CLI, never use HTTP API directly.
- UI styles: all colors and composed styles in `ui/styles.rs`. Don't use raw colors elsewhere.
- Syntax highlighting: `syntect` crate via `ui/highlight.rs`. Highlighter is created once in main and passed through to diff_view. Uses `base16-ocean.dark` theme. Language detection is automatic from filename.
- AI sidecar files: `.er-*` in repo root, gitignored. `er` reads all, writes `.er-questions.json` and `.er-github-comments.json`. Atomic writes via tmp+rename.
- Question fields: `ReviewQuestion` has `id`, `file`, `hunk_index`, `line_start`, `line_content`, `text`, `resolved`, `stale` (runtime-only).
- GitHub comment fields: `GitHubReviewComment` has `source` ("local"/"github"), `github_id` (Option<u64>), `author` (String), `synced` (bool), `in_reply_to` (Option<String>), `line_start` (Option<usize>), `stale` (runtime-only). Line comments have `line_start`; hunk comments do not.
- Unified query via `CommentRef` enum: query methods on `AiState` return `Vec<CommentRef>` which wraps either a `ReviewQuestion`, `GitHubReviewComment`, or legacy `FeedbackComment`.
- Diff parsing: the parser in `git/diff.rs` has unit tests. Run them with `cargo test`.
- Config file: `.er-config.toml` in repo root, parsed with `toml` crate + `serde::Deserialize`. Watched file globs use the `glob` crate.

## File Map

```
src/main.rs              Event loop, CLI parsing (clap), input routing
src/app/state.rs         App struct, all state, navigation, comments, comment focus, replies, watched files config, filter
src/app/filter.rs        Composable filter system (glob, status, size rules, presets)
src/git/diff.rs          parse_diff() — unified diff text → Vec<DiffFile>
src/git/status.rs        detect_base_branch(), git_diff_raw(), staging, worktrees, watched file ops
src/github.rs            GitHub PR URL parsing, gh CLI wrapper, comment sync (pull/push/reply/delete), PR base hint
src/ai/review.rs         AI data model (AiState, ErReview, Finding, ViewMode, CommentRef, ReviewQuestion, GitHubReviewComment)
src/ai/loader.rs         .er-* file loading, SHA-256 diff hashing, mtime polling
src/watch/mod.rs         FileWatcher — debounced notify watcher
src/ui/mod.rs            draw() — ViewMode-based layout dispatch
src/ui/styles.rs         Color constants and style helpers (blue-undertone theme)
src/ui/highlight.rs      Syntect-based syntax highlighting for diff lines
src/ui/file_tree.rs      Left panel — file list with risk indicators
src/ui/diff_view.rs      Right panel — hunks, line numbers, AI finding/comment banners, inline line comments
src/ui/ai_panel.rs       SidePanel mode — per-file findings + comments with threaded replies
src/ui/ai_review_view.rs AiReview mode — full-screen risk dashboard + checklist
src/ui/status_bar.rs     Top bar, bottom bar, AI status badges, comment input
src/ui/overlay.rs        Modal popups (worktree picker, directory browser, filter history)
src/ui/utils.rs          Shared utilities (word_wrap)
```

## Current State

v1.3 with split comment system (questions + GitHub comments). Building locally with `cargo install --path .`. Debug mode via `ER_DEBUG=1 er` writes to `/tmp/er_debug.log`. Test fixtures via `scripts/generate-test-fixtures.sh`.

## Roadmap

**v1 (done):** Branch/unstaged/staged diffs, file+hunk navigation, search, live file watching, auto base branch detection, syntax highlighting (syntect), open-in-editor (`e` key).

**v1.1 (done):** AI review integration (4 view modes, inline findings, comments), GitHub PR support (`--pr` flag, URL arguments), line-level navigation (arrow keys), basic comment system (`c` key → `.er-feedback.json`), watched files for git-ignored paths (`.er-config.toml`), composable filter system (`f` key, `--filter` flag, built-in presets via `F`), PR base hint when detected base differs from PR target, filtered reviewed count in status bar, mtime sort toggle (`Shift+R` — sort files by recency in any mode), watch mode on by default (detects edits, staging, and commits).

**v1.2 (done):** Enhanced comment system — GitHub PR comment sync (pull with `G`, push with `P`), single-level reply threads (`r` key), comment deletion with cascade (`d` key), inline line-comment rendering (comments appear after their target line, not just at hunk end), comment focus navigation (`Tab` to enter, arrows to move between comments).

**v1.3 (current):** Split comment system — personal questions (`q`/`Q` → `.er-questions.json`, yellow) vs GitHub comments (`c`/`C` → `.er-github-comments.json`, cyan). Per-comment staleness detection (dimmed when diff changes). Quit moved to `Ctrl+q`. `/er-publish` includes GitHub comments in PR review. `/er-questions` reads from `.er-questions.json`.

**v2:** Multi-worktree tabs (Tab/Shift+Tab to cycle), per-worktree state, cross-worktree watch notifications.
