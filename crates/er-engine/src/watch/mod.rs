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
                            let git_noise = p.contains("/.git/")
                                && !(p.ends_with("/.git/index") || p.contains("/.git/refs/"));
                            (!git_noise).then_some(p)
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

        Ok(Self {
            _watcher: debouncer,
        })
    }
}

/// A debounced watcher over the managed store root.
///
/// It reports registered store files as [`er_store::StoreEvent`]s. Nothing
/// consumes the channel yet; mtime polling stays until phase 4.
pub struct StoreWatcher {
    _watcher: notify_debouncer_mini::Debouncer<RecommendedWatcher>,
}

impl StoreWatcher {
    /// Start watching a store root. Registered changes are sent on `tx`.
    pub fn new(
        root: &Path,
        registry: er_store::Registry,
        ignore: Vec<er_store::Pattern>,
        debounce_ms: u64,
        tx: mpsc::Sender<er_store::StoreEvent>,
    ) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let root = std::fs::canonicalize(root)?;
        let watched_root = root.clone();
        let mut debouncer = new_debouncer(
            Duration::from_millis(debounce_ms),
            move |result: std::result::Result<
                Vec<notify_debouncer_mini::DebouncedEvent>,
                notify::Error,
            >| {
                if let Ok(events) = result {
                    for event in events
                        .iter()
                        .filter(|event| event.kind == DebouncedEventKind::Any)
                    {
                        if let Some(store_event) =
                            er_store::event_for_path(&watched_root, &event.path, &registry, &ignore)
                        {
                            let _ = tx.send(store_event);
                        }
                    }
                }
            },
        )?;
        debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: debouncer,
        })
    }
}

#[cfg(test)]
mod store_watcher_tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use er_store::{ArtifactStore, BranchView, Pattern, Registry, RelPath, Scope, Slug};

    use super::StoreWatcher;

    #[test]
    fn store_watcher_reports_a_bucket_write() {
        let root = tempfile::TempDir::new().unwrap();
        let store = ArtifactStore::new(root.path());
        let scope = Scope::Branch {
            repo: Slug::new("my-repo").unwrap(),
            branch: Slug::new("feature-x").unwrap(),
            view: BranchView::Branch,
        };

        let mut registry = Registry::new();
        registry.register_text("test", "notes.txt").unwrap();
        let ignore = vec![Pattern::new(".lock"), Pattern::new(".lease")];
        let (tx, rx) = mpsc::channel();
        let _watcher = StoreWatcher::new(root.path(), registry, ignore, 100, tx).unwrap();

        // The bucket does not exist yet; its files are written before inotify
        // arms the new directory.
        store
            .write_text(&scope, &RelPath::new("notes.txt").unwrap(), "hi")
            .unwrap();

        let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.scope, scope);
        assert_eq!(event.kind, "test");
    }
}
