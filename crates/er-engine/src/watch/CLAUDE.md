# watch/ — file watching

`notify` + `notify-debouncer-mini`, one file. It reports that files changed; it
never decides when to refresh.

## The debounce belongs to the caller

`FileWatcher::new(root, debounce_ms, tx)` takes the interval as an argument.
There is no module constant and no config key. The TUI passes 500ms, the
desktop 250ms, and the TUI adds its own 200ms coalescing window on top, so a
burst of edits produces one refresh.

## What it watches

`root`, recursively, plus the two paths under `.git/` that matter: `index`
(staging) and `refs/` (commits). The rest of `.git/` is filtered out, as is any
path containing `/.er/` — er's own writes (session saves, reviewed markers,
comments) would otherwise refresh it against its own output.

AI sidecars never arrive this way. Under managed storage they sit outside the
repo root; under `ER_REPO_LOCAL` the `/.er/` filter drops them. They are polled
instead, by `check_ai_files_changed()` comparing `er_dir()` mtimes.

## Kept alive by RAII, started automatically

Holding the `FileWatcher` is what keeps it running. The TUI holds
`Option<FileWatcher>` from launch — `w` toggles it, remote mode skips it. The
desktop holds it in a thread that re-targets when the active tab's checkout
changes, which is why it can watch a linked worktree instead of the repo root.

## Traps

- Watcher errors are dropped. Hitting the OS watch limit (inotify ENOSPC) stops
  live updates with nothing surfaced.
- Send failures are dropped on purpose — a dead receiver means the loop exited.
- Consumers drain non-blocking with `try_recv()`. A refresh auto-unmarks the
  reviewed files whose diff content actually changed.
