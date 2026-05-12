# watch/ — File System Watcher

Debounced file watching using `notify` + `notify-debouncer-mini`. Single file module.

## mod.rs (~60 lines)

**`FileWatcher`** — wraps a `Debouncer<RecommendedWatcher>`. Created with `FileWatcher::new(root, debounce_ms, tx)`.

- Watches the repo root recursively
- 500ms debounce (configurable)
- Filters out `.git/` directory changes
- Sends `WatchEvent::FilesChanged(Vec<String>)` over the provided `mpsc::Sender`

## Lifecycle

The watcher is RAII-based: storing the `FileWatcher` keeps it alive, dropping it stops watching. In main.rs it's held as `Option<FileWatcher>` — the `w` key toggles between `Some(new watcher)` and `None`.

Events are received in the main loop via `watch_rx.try_recv()` (non-blocking). On receiving a watch event, `App::refresh_diff()` is called to reload the diff.

Note: `.er-*` AI file changes are NOT detected by the watcher. They're polled separately every tick via `check_ai_files_changed()` using file mtime comparison.
