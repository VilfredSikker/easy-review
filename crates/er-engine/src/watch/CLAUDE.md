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
path containing `/.er/` — an `.er/` directory left in a repo by an older
version or another tool must not refresh the diff.

AI sidecars never arrive this way: they sit outside the repo root, under
managed storage. They are polled instead, by `check_ai_files_changed()`
comparing `er_dir()` mtimes.

## The store root

`StoreWatcher::new(root, registry, ignore, debounce_ms, tx)` watches the managed
store root recursively and sends a `StoreEvent` for each registered file that
changes; ignored and unregistered paths are dropped. Nothing consumes its
channel yet — mtime polling stays until phase 4.

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
- inotify only watches directories that exist when the watch starts, and it
  does not replay files written into a directory before that watch lands. A
  write into a bucket created after the watch starts can be missed until the
  directory is armed.
