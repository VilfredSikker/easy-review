use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// Events emitted by the file watcher
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// One or more files changed — time to refresh diffs
    FilesChanged(Vec<String>),
}

/// A debounced file watcher that monitors a git working tree
pub struct FileWatcher {
    _watcher: notify_debouncer_mini::Debouncer<RecommendedWatcher>,
}

impl FileWatcher {
    /// Start watching a directory. Changed file events are sent to the provided sender.
    /// Events are debounced by `debounce_ms` milliseconds.
    pub fn new(
        root: &Path,
        debounce_ms: u64,
        tx: mpsc::Sender<WatchEvent>,
    ) -> Result<Self> {
        let mut debouncer = new_debouncer(
            Duration::from_millis(debounce_ms),
            move |result: std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                // TODO(risk:minor): Watcher errors (`Err` branch) are silently discarded.
                // If the OS watch limit is hit (inotify: ENOSPC, kqueue: open file limit)
                // the watcher will stop delivering events with no indication to the user —
                // the UI will appear to stop live-updating without any error message.
                if let Ok(events) = result {
                    let paths: Vec<String> = events
                        .iter()
                        .filter(|e| e.kind == DebouncedEventKind::Any)
                        .filter_map(|e| {
                            // TODO(risk:minor): `to_string_lossy()` silently replaces
                            // non-UTF-8 path bytes with U+FFFD. On Linux, file paths can
                            // contain arbitrary bytes; such a path would be reported with
                            // corrupted characters and may fail to match downstream filters
                            // or open-in-editor logic.
                            let p = e.path.to_string_lossy().to_string();
                            // Allow .git/index (staging) and .git/refs/ (commits) through
                            // but skip other .git/ noise (objects, logs, etc.)
                            if p.contains("/.git/") {
                                // TODO(risk:minor): The `.git/` filter uses substring
                                // matching on the string representation of the path.
                                // On Windows the separator is `\`, so `/.git/` and
                                // `/.git/refs/` would never match — the filter would pass
                                // all `.git\` noise events through, causing unnecessary
                                // diff refreshes on every Git internal write.
                                if p.ends_with("/.git/index") || p.contains("/.git/refs/") {
                                    Some(p)
                                } else {
                                    None
                                }
                            } else {
                                Some(p)
                            }
                        })
                        .collect();

                    if !paths.is_empty() {
                        // TODO(risk:medium): The channel send is fire-and-forget (`let _ =`).
                        // If the receiver has been dropped (e.g. the main loop exited) this
                        // silently succeeds. More importantly, if the mpsc channel is bounded
                        // and full, the send will fail and the watch event is lost entirely —
                        // the UI will miss a file change and show a stale diff until the next
                        // watch event or manual refresh.
                        let _ = tx.send(WatchEvent::FilesChanged(paths));
                    }
                }
            },
        )?;

        debouncer.watcher().watch(root, RecursiveMode::Recursive)?;

        Ok(FileWatcher {
            _watcher: debouncer,
        })
    }
}
