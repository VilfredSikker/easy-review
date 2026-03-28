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

Rust + Ratatui TUI. Six modules + a standalone GitHub integration file:

- **`git/`** — Shells out to `git diff` and parses unified diff format into structured data (`DiffFile` → `DiffHunk` → `DiffLine`). Handles base branch auto-detection: upstream tracking → main → master → develop → dev. Also provides staging (file level), worktree listing, commit log loading (`git log` + `git diff` for individual commits), and watched file discovery/diffing. Includes two-phase lazy parsing for large diffs (header-only scan + on-demand file parse) and auto-compaction of low-value files (lock files, generated code).
- **`watch/`** — File system watcher using `notify` + `notify-debouncer-mini`. 500ms debounce. Watches working tree changes plus `.git/index` (staging) and `.git/refs/` (commits). Sends events via `std::sync::mpsc` channel. Starts automatically on launch.
- **`app/`** — All application state in one `App` struct. Six diff modes (Branch, Unstaged, Staged, History, Wizard, Quiz). Six input modes (Normal, Search, Comment, Confirm, Filter, Commit). File/hunk/line navigation, AI state management, comment persistence (with replies and deletion), comment focus navigation, watch notifications, auto-unmark reviewed files on diff change, cleanup commands. History mode has its own `HistoryState` with commit list, per-commit diff cache (LRU, 5 entries), and lazy loading. Wizard mode (`DiffMode::Wizard`, key `7`) provides guided AI review — smart filtering, risk-based file ordering, and context panel with symbol references; state in `WizardState`. Quiz mode (`DiffMode::Quiz`, key `8`) provides an interactive comprehension quiz driven by `.er/quiz.json` — MC and freeform answers, scoring, and level/category filtering; state in `QuizState`. Holds `ErConfig` for settings. Loads `.er-config.toml` for watched files configuration. Composable filter system (`filter.rs`) with glob, status, size, and risk rules. Mtime sort toggle (`Shift+R`) works in any diff mode.
- **`ui/`** — Ratatui rendering. InlineLayers + PanelContent system (replaces ViewMode). Cool blue-undertone dark theme in `styles.rs`. Viewport-based rendering for diff and file tree (only builds `Line` objects for visible rows). Syntax highlighting is cached by content hash. Settings overlay for live config editing via `S` key. Sticky file path header in diff view.
- **`ai/`** — Data model and file loader for AI-generated review artifacts. Reads `.er/` directory files written by external Claude Code skills. Manages staleness detection via SHA-256 diff hashing. Lazy comment index (`CommentIndexData`) for fast per-file comment lookup without loading all comments into memory. Does NOT run AI — reads AI output.
- **`config.rs`** — Configuration system. Loads `.er-config.toml` (per-repo) or `~/.config/er/config.toml` (global) with serde defaults. `ErConfig` struct holds `FeatureFlags`, `AgentConfig`, and `DisplayConfig`. Settings items for the overlay UI are defined here.
- **`github.rs`** — GitHub CLI (`gh`) wrapper for PR integration. Parses PR URLs, checks out PR branches, resolves base branches, detects open PRs for current branch. Two-way comment sync: pull review comments from GitHub, push local comments back, reply to threads, delete comments. No API token needed — uses `gh auth`.

The event loop in `main.rs` polls for keyboard input (100ms timeout) and checks for file watch events each tick. Watch events are debounced (200ms) to batch rapid changes. PR base hint check runs on a background thread to avoid blocking startup. No async runtime needed — crossterm polling + mpsc channels.

## Key Design Decisions

- **Shell out to git, not gitoxide.** Simpler, proven, handles all edge cases. Git is always available. Can optimize later if profiling shows it matters.
- **Sync event loop, not async.** Crossterm's polling + mpsc channels are sufficient. No async runtime needed.
- **File watching in v1.** This is a core feature, not a nice-to-have. The whole point is following AI-generated changes live.
- **Auto-detect base branch.** Users shouldn't have to specify it. Upstream tracking → main → master → develop fallback chain. When `gh` is available and the branch has an open PR targeting a different base, a hint is shown (no auto-switch).
- **`.er/` directory for AI data.** All sidecar files live in `.er/` in the repo root (e.g., `.er/review.json`, `.er/questions.json`, `.er/github-comments.json`). Cleaner than scattered dotfiles — single `.gitignore` entry covers everything. Written by external Claude Code skills, read by `er`. Decoupled: skills can run from any terminal/CI, `er` picks up results via mtime polling (~1s).
- **Staleness via diff hashing.** Each file in `.er/` stores the SHA-256 hash of the diff it was generated against. When the diff changes, the UI warns that AI data may be out of date.
- **`gh` CLI for GitHub, not HTTP API.** No API token management. Users already have `gh auth login` configured.
- **One `er` instance per worktree.** Multi-worktree tabs work via `t` key (worktree picker).
- **Auto-compact low-value files.** Lock files, generated code, minified assets, and files above 500 lines are compacted automatically — hunks cleared from memory, expandable on demand via `Enter`. Pattern list in `CompactionConfig::default()`.
- **Two-phase lazy parsing for large diffs.** Diffs above 5,000 lines use a header-only scan (`parse_diff_headers`) for the file list, then parse individual files on demand when navigated to. Keeps raw diff string + byte offsets for instant re-parse.
- **Viewport-based rendering.** Both diff view and file tree only build `Line`/`ListItem` objects for the visible terminal rows (+ 20-line buffer). A 5,000-line file builds ~60 Lines instead of 5,000.
- **Precomputed hunk offsets.** `HunkOffsets` gives O(1) scroll position lookup (vs O(n) hunk iteration). Rebuilt on file selection.
- **Syntax highlighting cache.** Content+filename hash → cached spans. LRU eviction (10K entry limit). High hit rate since most lines don't change between frames.
- **Fast diff hash for watch events.** `DefaultHasher` (non-cryptographic) for internal change detection during watch-mode refreshes. SHA-256 only used for `.er/review.json` compatibility where hashes are persisted.
- **Deduplicated git calls.** `refresh_diff_impl` guarantees at most 2 `git diff` invocations per refresh (down from 3) by reusing the raw output for both parsing and hash computation.
- **Split comment system.** Two separate files: `.er/questions.json` for personal review questions (read by `/er-questions` skill), `.er/github-comments.json` for GitHub PR comments (two-way sync). Questions use `q`/`Q` keys (yellow), comments use `c`/`C` keys (cyan). Questions are private; GitHub comments get pushed via `/er-publish`.
- **Flat single-level comment threads.** GitHub comments can have replies, but replies cannot have replies. Keeps the model simple — parent + N children, no recursive nesting. Questions don't support replies (use `/er-questions` for AI responses).
- **GitHub comments track source and sync state.** Each `GitHubReviewComment` has `source` (local/github), `github_id`, `author`, and `synced` fields. Deduplication on pull uses `github_id`. Push marks comments as synced.
- **Per-comment staleness.** Comments store `line_content` of their target line. When the diff changes, individual comments are marked stale and rendered dimmed with a warning indicator.
- **Auto-unmark reviewed files on diff change.** When a watched file change triggers a diff refresh, any file whose diff content changed is automatically unmarked as reviewed. Prevents stale reviewed state from hiding new changes.
- **Post-commit diff view.** After committing, `er` automatically switches to a view of the just-committed diff (using `git diff HEAD~1 HEAD`). Lets you review exactly what was committed before moving on.
- **Config via TOML, not CLI flags.** Per-repo (`.er-config.toml`) overrides global (`~/.config/er/config.toml`) overrides built-in defaults. Every feature is gated behind `config.features.*`. Settings overlay applies changes live; `s` persists, Esc reverts.
- **Watched files for git-ignored paths.** `.er-config.toml` with `[watched]` section specifies glob patterns for files to monitor (e.g., `.work/` agent sync folders). Two diff modes: "content" (show file contents) and "snapshot" (diff against saved baseline). Gitignore safety check warns if watched files aren't ignored.
- **Wizard and Quiz as first-class DiffMode views.** Both modes are `DiffMode` variants (not overlays) with dedicated rendering paths, input routing in `main.rs`, and their own state structs (`WizardState`, `QuizState`) on `TabState`. This keeps the mode-switch logic uniform and avoids modal stacking.
- **Quiz learning loop via `.er/` sidecar files.** Quiz generation (`/er-quiz` skill → `.er/quiz.json`) and feedback evaluation (skill reads `.er/quiz-answers.json` → writes `.er/quiz-feedback.json`) happen entirely outside `er`. The TUI provides the interaction surface; skills provide the AI. Same decoupling pattern as review/questions.

## Code Conventions

- Module structure: each directory has `mod.rs` for exports, separate files for implementation. Each module has its own `CLAUDE.md`.
- Error handling: `anyhow::Result` everywhere. Bubble up with `?`, context with `.context()`.
- Git commands: all in `git/status.rs`. Always pass `--no-color` and `--no-ext-diff` (bypasses difftastic/delta).
- GitHub commands: all in `github.rs`. Shell out to `gh` CLI, never use HTTP API directly.
- UI styles: all colors and composed styles in `ui/styles.rs`. Don't use raw colors elsewhere.
- Syntax highlighting: `syntect` crate via `ui/highlight.rs`. Highlighter is created once in main and passed as `&mut` to diff_view (mutable for cache writes). Uses `base16-ocean.dark` theme. Language detection is automatic from filename. Results are cached by content hash with LRU eviction.
- AI sidecar files: all live in `.er/` directory in the repo root, gitignored with a single `.gitignore` entry. `er` reads all files in `.er/`, writes `.er/questions.json` and `.er/github-comments.json`. Atomic writes via tmp+rename. `CommentIndexData` provides a lazy per-file index for fast comment lookup without loading full comment lists. Quiz sidecar files: `.er/quiz.json` (generated by `/er-quiz` skill, read-only from `er`), `.er/quiz-answers.json` (written by `er` when saving answers), `.er/quiz-feedback.json` (written by skill after AI evaluates freeform answers).
- Question fields: `ReviewQuestion` has `id`, `file`, `hunk_index`, `line_start`, `line_content`, `text`, `resolved`, `stale` (runtime-only).
- GitHub comment fields: `GitHubReviewComment` has `source` ("local"/"github"), `github_id` (Option<u64>), `author` (String), `synced` (bool), `in_reply_to` (Option<String>), `line_start` (Option<usize>), `stale` (runtime-only). Line comments have `line_start`; hunk comments do not.
- Unified query via `CommentRef` enum: query methods on `AiState` return `Vec<CommentRef>` which wraps either a `ReviewQuestion`, `GitHubReviewComment`, or legacy `FeedbackComment`.
- Diff parsing: the parser in `git/diff.rs` has unit tests. Run them with `cargo test`. For large diffs, `parse_diff_headers()` provides a fast header-only scan; `parse_file_at_offset()` parses a single file on demand.
- Compaction: `DiffFile.compacted` flag indicates a file whose hunks have been cleared. `compact_files()` applies pattern + size threshold. `expand_compacted_file()` re-fetches from git on demand. Glob matching via `glob_match()` in `diff.rs`.
- Performance state: `HunkOffsets`, `MemoryBudget`, `FileTreeCache`, and `lazy_mode` flag live on `TabState`. The `ensure_file_parsed()` method triggers on-demand parsing in lazy mode.
- Configuration: TOML via `toml` crate + `dirs` for platform config paths. All config types in `src/config.rs`. Feature flags default to `true` except `blame_annotations`. New features should add a flag to `FeatureFlags` and a `SettingsItem` entry in `settings_items()`.
- Config file: `.er-config.toml` in repo root, parsed with `toml` crate + `serde::Deserialize`. Watched file globs use the `glob` crate.

## File Map

```
src/main.rs              Event loop, CLI parsing (clap), input routing, debounced watch refresh
src/config.rs            ErConfig, FeatureFlags, load/save, settings items
src/app/state.rs         App struct, all state, navigation, comments, comment focus, replies, HistoryState, DiffCache, watched files config, filter, HunkOffsets, MemoryBudget, lazy parsing
src/app/filter.rs        Composable filter system (glob, status, size rules, presets)
src/git/diff.rs          parse_diff(), parse_diff_headers(), compact_files(), expand_compacted_file()
src/git/status.rs        detect_base_branch(), git_diff_raw(), git_diff_raw_file(), staging, worktrees, git_log_branch(), git_diff_commit(), watched file ops
src/github.rs            GitHub PR URL parsing, gh CLI wrapper, comment sync (pull/push/reply/delete), PR base hint
src/ai/review.rs         AI data model (AiState, ErReview, Finding, InlineLayers, PanelContent, CommentRef, ReviewQuestion, GitHubReviewComment, CommentIndexData)
src/ai/loader.rs         .er/ directory loading, SHA-256 + fast diff hashing, mtime polling, mtime cache
src/watch/mod.rs         FileWatcher — debounced notify watcher
src/ui/mod.rs            draw() — panel-based layout dispatch
src/ui/styles.rs         Color constants and style helpers (blue-undertone theme)
src/ui/highlight.rs      Syntect-based syntax highlighting with LRU content-hash cache
src/ui/file_tree.rs      Left panel — file list with risk indicators (or commit list in History mode)
src/ui/diff_view.rs      Right panel — viewport-based rendering, compacted file view, inline line comments, sticky file path header (or multi-file commit diff in History mode)
src/ui/panel.rs          Side panel — FileDetail, AiSummary, PrOverview content renderers
src/ui/status_bar.rs     Top bar, bottom bar, AI status badges, memory budget (debug), comment input
src/ui/overlay.rs        Modal popups (worktree picker, directory browser, filter history)
src/ui/settings.rs       Settings overlay — toggleable config items
src/ui/quiz.rs           Quiz view — question list, answer panel, freeform input
src/ui/utils.rs          Shared utilities (word_wrap)
```

## Current State

v1.5 with Wizard mode (key `7`) for guided AI review and Quiz mode (key `8`) for interactive comprehension quizzes. Building v0.2.0 release branch. Earlier: v1.4 with `.er/` directory migration, auto-unmark reviewed, post-commit diff view, cleanup commands, sticky file path header, and lazy comment index. Debug mode via `ER_DEBUG=1 er` writes to `/tmp/er_debug.log` and shows memory budget in the status bar. Test fixtures via `scripts/generate-test-fixtures.sh`.

## Roadmap

**v1 (done):** Branch/unstaged/staged diffs, file+hunk navigation, search, live file watching, auto base branch detection, syntax highlighting (syntect), open-in-editor (`e` key).

**v1.1 (done):** AI review integration (4 view modes, inline findings, comments), GitHub PR support (`--pr` flag, URL arguments), line-level navigation (arrow keys), basic comment system (`c` key → `.er/feedback.json`), watched files for git-ignored paths (`.er-config.toml`), composable filter system (`f` key, `--filter` flag, built-in presets via `F`), PR base hint when detected base differs from PR target, filtered reviewed count in status bar, mtime sort toggle (`Shift+R` — sort files by recency in any mode), watch mode on by default (detects edits, staging, and commits). Large diff performance — auto-compaction, two-phase lazy parsing, viewport-based rendering, syntax highlight cache, precomputed hunk offsets, debounced watch refresh, deduplicated git calls, fast diff hash, memory budget tracking.

**v1.2 (done):** Enhanced comment system — GitHub PR comment sync (pull with `G`, push with `P`), single-level reply threads (`r` key), comment deletion with cascade (`d` key), inline line-comment rendering (comments appear after their target line, not just at hunk end), comment focus navigation (`Tab` to enter, arrows to move between comments). Settings system (`S` key → settings overlay, `.er-config.toml` config file). Feature flags for split diff, exit heatmap, blame annotations, bookmarks. Display options (line numbers, wrap lines, tab width). Agent configuration.

**v1.3 (done):** Split comment system — personal questions (`q`/`Q` → `.er/questions.json`, yellow) vs GitHub comments (`c`/`C` → `.er/github-comments.json`, cyan). Per-comment staleness detection (dimmed when diff changes). Quit moved to `Ctrl+q`. `/er-publish` includes GitHub comments in PR review. `/er-questions` reads from `.er/questions.json`.

**v1.4 (done):** `.er/` directory migration — all sidecar files moved from scattered repo-root dotfiles into `.er/` (single `.gitignore` entry). Auto-unmark reviewed files when diff changes (prevents stale reviewed state). Jump to next unreviewed file (`U`). Git push from TUI (`Ctrl+P` in staged mode). Post-commit diff view (shows just-committed diff after committing). Cleanup commands (`z` to clean current file artifacts, `Z` to clean all). Sticky file path header in diff view. Lazy comment index (`CommentIndexData`) for fast per-file comment lookup. LRU eviction for syntax highlight cache (replaces full eviction on overflow). Mtime cache to reduce `stat` calls during polling. `er-questions` standalone redesign. History mode key deduplication.

**v1.5 (current):** Wizard mode (`7` key) — guided AI review with risk-based file ordering, `WizardState` completion tracking, symbol references panel, and smart Info-file filtering. Quiz mode (`8` key) — interactive comprehension quiz driven by `.er/quiz.json`; MC + freeform answers, scoring, level/category filters, freeform text input mode, answer persistence to `.er/quiz-answers.json` and feedback loop via `.er/quiz-feedback.json`.

**v2:** Split diff mode, review heatmap on exit, blame-aware findings, diff bookmarks. Multi-worktree tabs (Tab/Shift+Tab to cycle), per-worktree state, cross-worktree watch notifications.

## Design Context

**Brand personality:** Sharp, Focused, Fast. No decoration — every pixel earns its place.

**Theme system (planned):** Introducing multiple themes (dark, light, variants). All colors should flow through a theme abstraction — components reference semantic tokens, not hex values. Full design context in `.impeccable.md`.

**Semantic color tokens:**
- Background: `bg`, `surface`, `panel`, `border`
- Text: `text.primary`, `text.bright`, `text.dim`, `text.muted`
- Diff: `diff.add.bg/text`, `diff.del.bg/text`, `diff.hunk.bg`
- Accent: `accent.action` (blue), `accent.info` (cyan), `accent.success` (green), `accent.warning` (yellow), `accent.error` (red), `accent.selection` (purple), `accent.emphasis` (orange)
- Interactive: `cursor.bg`, `focus.bg`, `comment.bg`, `finding.bg`

**Design principles:**
1. Information density over whitespace
2. Semantic color, not decorative color
3. Contrast creates hierarchy (bright/normal/dim + accents)
4. Theme-ready architecture (semantic tokens, not hardcoded hex)
5. Speed is a feature (never trade performance for aesthetics)
