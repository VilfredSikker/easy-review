# src/ — Source Overview

## Module Map

| Module | Purpose | Key file |
|--------|---------|----------|
| `main.rs` | Event loop, terminal setup, input routing | — |
| `app/` | All application state (`App`, `TabState`) | `state.rs` |
| `git/` | Diff parsing + git commands | `diff.rs`, `status.rs` |
| `ui/` | Ratatui rendering (6 sub-renderers) | `mod.rs` orchestrates |
| `watch/` | Debounced file system watcher | `mod.rs` |
| `ai/` | AI review data model + file loader | `review.rs`, `loader.rs` |
| `github.rs` | GitHub CLI (`gh`) integration for PRs | — |

## Data Flow

```
main.rs event loop
  ├── crossterm poll (100ms) → input handlers → mutate App
  ├── watch_rx.try_recv()    → App::refresh_diff()
  ├── hint_rx.try_recv()     → PR base hint (background thread, fires once)
  ├── check_ai_files_changed → reload .er-* files
  └── ui::draw(frame, app, highlighter)
```

## main.rs (~445 lines)

Entry point. CLI parsing via clap (`--pr`, `--filter`, positional paths). Sets up crossterm alternate screen, creates `App` and `Highlighter`, runs `run_app()` loop. PR base hint check spawns a background thread before terminal init; result is consumed via `hint_rx.try_recv()` in the event loop.

**Input routing:** `InputMode` and `OverlayData` on `App` determine which handler runs:
- `Normal` → `handle_normal_input()` (file/hunk/line nav, mode switching, staging)
- `Search` → `handle_search_input()` (builds query, Enter snaps to match)
- `Comment` → `handle_comment_input()` (builds comment text, Enter submits)
- `Filter` → `handle_filter_input()` (builds filter expression, Enter applies)
- Overlay active → `handle_overlay_input()` (j/k/Enter/Esc in popup)

**Watch toggle:** `w` key swaps `Option<FileWatcher>` between `Some` (watching) and `None` (stopped). Dropping the watcher stops it (RAII).

## github.rs (~200 lines)

Parses GitHub PR URLs (`owner/repo/pull/N`), calls `gh pr view` for base branch info, `gh pr checkout` to check out PRs. `gh_pr_for_current_branch()` detects if the current branch has an open PR (used for the base hint). Uses `--jq` for robust output parsing. Used from `App::new_with_args` when a path argument is a GitHub URL, and from `main()` for `--pr N` and the background hint check.
