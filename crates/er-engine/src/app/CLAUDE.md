# app/ — Application State

All state lives here. No rendering, no I/O beyond git commands and file
persistence. See `state/agent.md` for the `App` vs `TabState` vs desktop
`AppState` ownership boundary.

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports `App`, `TabState`, enums |
| `state/mod.rs` | Core types (`App`, `TabState`, `DiffMode`, `InputMode`, overlays), diff refresh, review tracking, tabs, watched files, persistence |
| `state/navigation.rs` | File/hunk/line movement, lazy parsing, scroll state, split-diff helpers |
| `state/comments.rs` | Comment/question lifecycle, AI review spawning, background task polling |
| `state/github_sync.rs` | GitHub comment sync capture/fetch/apply flow |
| `state/background.rs` | App-level background review task identity and lifecycle |
| `state/arena.rs` | Arena (multi-reviewer) run start/promotion glue |
| `state/remote_diff_sync.rs` | Remote PR diff polling |
| `filter.rs` | Composable filter system (parse, apply, presets) |
| `card_ai_context.rs` / `card_ai_spawn.rs` | Per-card AI invocation context + subprocess spawn |

## Key Types

**`App`** — Top-level state. Owns `tabs: Vec<TabState>`, `active_tab`, `input_mode`, `should_quit`, `overlay`, config, background tasks, watch state.

**`TabState`** — Per-review-target state (working tree, branch view, local PR, or remote PR). Contains:
- Diff data: `files: Vec<DiffFile>`, `selected_file`, `current_hunk`, `current_line`
- Mode: `DiffMode` (Branch/Unstaged/Staged/History), `base_branch`, `current_branch`
- Scroll: `diff_scroll`, `h_scroll`
- Review tracking: `reviewed: HashSet<String>`, `show_unreviewed_only`, `filtered_reviewed_count()`
- Filters: `filter_expr`, `filter_rules: Vec<FilterRule>`, `filter_history`, `filter_input`
- AI: `ai: AiState` (loaded from review sidecar files)
- Comments: comment textarea state, `comment_file`, `comment_hunk`, `comment_line_num`
- Watched: `watched_config`, `watched_files`, `selected_watched`, `show_watched`, `watched_not_ignored`
- Performance: `hunk_offsets`, `mem_budget`, `lazy_mode`, `raw_diff` + byte offsets

**`DiffMode`** — `Branch | Unstaged | Staged | History`. Each has a `git_mode()` string for `git_diff_raw`.

**`InputMode`** — `Normal | Search | Comment | Confirm | Filter | Commit | RemoteUrl`. Determines which input handler runs in the TUI event loop.

**`OverlayData`** — Modal overlays: worktree picker, directory browser, filter history, hubs, config hub.

## Navigation Model

- `next_file/prev_file` — moves `selected_file` index, resets hunk/line. Seamlessly transitions into/out of watched files section when `show_watched` is true.
- `next_hunk/prev_hunk` — moves `current_hunk`, resets `current_line` to `None`
- `next_line/prev_line` — sets `current_line: Some(i)`, crosses hunk boundaries automatically
- `scroll_to_current_hunk()` — computes scroll offset for the current hunk position via `HunkOffsets`

`current_line: Option<usize>` — `None` = hunk-level navigation (n/N keys). `Some(i)` = line-level (arrow keys). Hunk keys reset it to `None`.

`selected_watched: Option<usize>` — `None` = cursor is in diff files section. `Some(idx)` = cursor is on a watched file. Navigation flows from diff files into watched files and back.

## Persistence

Review sidecars live under the managed storage root resolved by
`TabState::er_dir()` (see `storage.rs`; `ER_REPO_LOCAL=1` falls back to repo
`.er/`):

| File | Format | Written by |
|------|--------|------------|
| `reviewed` | Plaintext, one path per line | `save_reviewed_files()` |
| `questions.json` | JSON | `submit_comment()` (questions) |
| `github-comments.json` | JSON | `submit_comment()` (GitHub comments) |
| `checklist.json` | JSON (`ErChecklist`) | `review_toggle_checklist()` |
| `snapshots/` | Raw file copies | `update_watched_snapshot()` |

`.er-config.toml` (repo root, read-only here) configures features and watched
files. `reviewed` is deleted when empty. Comments are marked stale per-comment
when the diff changes.

## Important Patterns

- `refresh_diff()` — re-runs git diff, re-parses, recomputes `diff_hash`, reloads AI state, clamps selection indices
- `refresh_watched_files()` — re-discovers watched files from glob patterns, verifies gitignore status
- `reload_ai_state()` — preserves review focus/cursor across reloads
- `check_ai_files_changed()` — compares sidecar mtimes against `last_ai_check`; triggers reload if changed
- `notify(msg)` + `tick()` — notification auto-clears after 20 ticks (~2 seconds at 100ms poll)
- `apply_filter_expr()` — parses filter expression into rules, updates history (MRU, deduped, max 20)
- `filtered_reviewed_count()` — single-pass reviewed count among filtered files; returns `None` when no filter active
- Filter rules: `Glob` (include/exclude by pattern), `Status` (added/modified/deleted/renamed), `Size` (line count threshold)
