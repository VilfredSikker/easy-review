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
    pub fn new(root: &Path, debounce_ms: u64, tx: mpsc::Sender<WatchEvent>) -> Result<Self> {
        let mut debouncer = new_debouncer(
            Duration::from_millis(debounce_ms),
            move |result: std::result::Result<
                Vec<notify_debouncer_mini::DebouncedEvent>,
                notify::Error,
            >| {
                // Watcher errors are discarded — if the OS watch limit is hit
                // (e.g. inotify ENOSPC), live updates silently stop.
                if let Ok(events) = result {
                    let paths: Vec<String> = events
                        .iter()
                        .filter(|e| e.kind == DebouncedEventKind::Any)
                        .filter_map(|e| {
                            let p = e.path.to_string_lossy().to_string();
                            // Skip .er/ directory — written by er itself (session saves,
                            // reviewed markers, comments, snapshots). Watching these causes
                            // spurious "N files changed" refresh loops. AI sidecar files
                            // are polled separately via mtime checks.
                            if p.contains("/.er/") {
                                return None;
                            }
                            // Allow .git/index (staging) and .git/refs/ (commits) through
                            // but skip other .git/ noise (objects, logs, etc.)
                            if p.contains("/.git/") {
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
                        // Fire-and-forget: a send error means the receiver was dropped
                        // (main loop exited), so the event is intentionally discarded.
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
