# app/ — Application State

All state lives here. No rendering, no I/O beyond git commands and file persistence.

## Files

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | ~4 | Re-exports `App`, `TabState`, enums |
| `state.rs` | ~1250 | Everything else |

## Key Types

**`App`** — Top-level state. Owns `tabs: Vec<TabState>`, `active_tab`, `input_mode`, `should_quit`, `overlay`, watch state.

**`TabState`** — Per-repo state. One per open worktree/directory. Contains:
- Diff data: `files: Vec<DiffFile>`, `selected_file`, `current_hunk`, `current_line`
- Mode: `DiffMode` (Branch/Unstaged/Staged), `base_branch`, `current_branch`
- Scroll: `diff_scroll`, `h_scroll`
- Review tracking: `reviewed: HashSet<String>`, `show_unreviewed_only`
- AI: `ai: AiState` (loaded from `.er-*` files)
- Comments: `comment_input`, `comment_file`, `comment_hunk`, `comment_line_num`
- Watched: `watched_config`, `watched_files`, `selected_watched`, `show_watched`, `watched_not_ignored`

**`DiffMode`** — `Branch | Unstaged | Staged`. Each has a `git_mode()` string for `git_diff_raw`.

**`InputMode`** — `Normal | Search | Comment`. Determines which input handler runs in main.rs.

## Navigation Model

- `next_file/prev_file` — moves `selected_file` index, resets hunk/line. Seamlessly transitions into/out of watched files section when `show_watched` is true.
- `next_hunk/prev_hunk` — moves `current_hunk`, resets `current_line` to `None`
- `next_line/prev_line` — sets `current_line: Some(i)`, crosses hunk boundaries automatically
- `scroll_to_current_hunk()` — computes pixel offset for the current hunk position

`current_line: Option<usize>` — `None` = hunk-level navigation (n/N keys). `Some(i)` = line-level (arrow keys). Hunk keys reset it to `None`.

`selected_watched: Option<usize>` — `None` = cursor is in diff files section. `Some(idx)` = cursor is on a watched file. Navigation flows from diff files into watched files and back.

## Persistence

| File | Format | Written by |
|------|--------|------------|
| `.er-reviewed` | Plaintext, one path per line | `save_reviewed_files()` |
| `.er-feedback.json` | JSON (`ErFeedback`) | `submit_comment()` |
| `.er-checklist.json` | JSON (`ErChecklist`) | `review_toggle_checklist()` |
| `.er-config.toml` | TOML (`ErConfig`) | User-created (read-only) |
| `.er-snapshots/` | Raw file copies | `update_watched_snapshot()` |

`.er-reviewed` is deleted when empty. `.er-feedback.json` is reset when `diff_hash` changes (stale comments cleared).

## Important Patterns

- `refresh_watched_files()` — re-discovers watched files from glob patterns, verifies gitignore status
- `refresh_diff()` — re-runs git diff, re-parses, recomputes `diff_hash`, reloads AI state, clamps selection indices
- `reload_ai_state()` — preserves `view_mode/review_focus/review_cursor` across reloads
- `check_ai_files_changed()` — compares `.er-*` file mtimes against `last_ai_check`; triggers reload if changed
- `chrono_now()` — hand-rolled ISO 8601 UTC timestamp (avoids chrono crate dependency)
- `notify(msg)` + `tick()` — notification auto-clears after 20 ticks (~2 seconds at 100ms poll)
