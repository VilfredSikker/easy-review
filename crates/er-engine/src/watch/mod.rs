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

        Ok(Self {
            _watcher: debouncer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc::Receiver;
    use std::time::Instant;
    use tempfile::TempDir;

    /// Collect every path the watcher emits until a path ending in
    /// `barrier_suffix` arrives — that arrival is the proof the watcher is
    /// actually live, so the negative assertions below can never pass
    /// vacuously — then drain stragglers for one more timeout window.
    fn drain_until_barrier(rx: &Receiver<WatchEvent>, barrier_suffix: &str) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        let mut saw_barrier = false;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(WatchEvent::FilesChanged(paths)) => {
                    if paths.iter().any(|p| p.ends_with(barrier_suffix)) {
                        saw_barrier = true;
                    }
                    seen.extend(paths);
                }
                // One quiet window after the barrier means the batch is done.
                Err(_) if saw_barrier => break,
                Err(_) => {}
            }
        }
        assert!(
            saw_barrier,
            "watcher never reported the barrier file {barrier_suffix}; got {seen:?}"
        );
        seen
    }

    #[test]
    fn watcher_reports_worktree_writes_but_never_er_sidecar_writes() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        // Directories exist before the watch starts so only the file writes
        // below can produce events.
        fs::create_dir_all(root.join(".er")).unwrap();

        let (tx, rx) = mpsc::channel();
        let _watcher = FileWatcher::new(root, 50, tx).unwrap();

        fs::write(root.join(".er").join("review.json"), "{}").unwrap();
        fs::write(root.join("src.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("barrier.txt"), "x").unwrap();

        let seen = drain_until_barrier(&rx, "barrier.txt");

        assert!(
            seen.iter().any(|p| p.ends_with("src.rs")),
            "ordinary worktree writes must reach the main loop: {seen:?}"
        );
        assert!(
            !seen.iter().any(|p| p.contains("/.er/")),
            "`.er/` is written by er itself — reporting it causes refresh loops: {seen:?}"
        );
    }

    #[test]
    fn watcher_passes_git_index_and_refs_but_drops_other_git_noise() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".git").join("refs").join("heads")).unwrap();
        fs::create_dir_all(root.join(".git").join("objects").join("ab")).unwrap();

        let (tx, rx) = mpsc::channel();
        let _watcher = FileWatcher::new(root, 50, tx).unwrap();

        fs::write(root.join(".git").join("objects").join("ab").join("cdef"), "b").unwrap();
        fs::write(root.join(".git").join("index"), "idx").unwrap();
        fs::write(root.join(".git").join("refs").join("heads").join("main"), "sha\n").unwrap();
        fs::write(root.join("barrier.txt"), "x").unwrap();

        let seen = drain_until_barrier(&rx, "barrier.txt");

        assert!(
            seen.iter().any(|p| p.ends_with("/.git/index")),
            "staging changes (.git/index) must be reported: {seen:?}"
        );
        assert!(
            seen.iter().any(|p| p.contains("/.git/refs/")),
            "new commits (.git/refs/) must be reported: {seen:?}"
        );
        assert!(
            !seen.iter().any(|p| p.contains("/.git/objects/")),
            "loose-object churn is git noise and must be dropped: {seen:?}"
        );
    }

    #[test]
    fn new_fails_and_names_the_path_when_root_does_not_exist() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        let (tx, _rx) = mpsc::channel();

        // `FileWatcher` is not `Debug`, so unwrap_err/expect_err are unavailable.
        let err = match FileWatcher::new(&missing, 50, tx) {
            Ok(_) => panic!("watching a missing directory must fail, not silently no-op"),
            Err(e) => e,
        };

        assert!(
            err.to_string().contains("does-not-exist"),
            "the error must name the unwatchable path: {err}"
        );
    }
}
