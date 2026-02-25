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
  ├── check_ai_files_changed → reload .er-* files
  ├── periodic rescan (~5s)  → refresh_watched_files()
  └── ui::draw(frame, app, highlighter)
```

## main.rs (~445 lines)

Entry point. CLI parsing via clap (`--pr`, positional paths). Sets up crossterm alternate screen, creates `App` and `Highlighter`, runs `run_app()` loop.

**Input routing:** `InputMode` and `OverlayData` on `App` determine which handler runs:
- `Normal` → `handle_normal_input()` (file/hunk/line nav, mode switching, staging)
- `Search` → `handle_search_input()` (builds query, Enter snaps to match)
- `Comment` → `handle_comment_input()` (builds comment text, Enter submits)
- Overlay active → `handle_overlay_input()` (j/k/Enter/Esc in popup)

**Watch toggle:** `w` key swaps `Option<FileWatcher>` between `Some` (watching) and `None` (stopped). Dropping the watcher stops it (RAII).

**Watched files:** `W` key toggles visibility of watched files (configured via `.er-config.toml`). `s` key in watched context saves a snapshot. Rescan runs every ~50 ticks (~5s).

## github.rs (~200 lines)

Parses GitHub PR URLs (`owner/repo/pull/N`), calls `gh pr view` for base branch info, `gh pr checkout` to check out PRs. Used from `App::new_with_args` when a path argument is a GitHub URL, and from `main()` for `--pr N`.
