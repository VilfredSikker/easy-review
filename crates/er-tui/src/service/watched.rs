//! The watched files on the flag path (P69.2). The list and the selected
//! file's snapshot diff are read on one worker thread through er-git's read
//! functions, so no key waits on the disk, and in snapshot mode no frame
//! does. In content mode the shared renderer still reads the file during the
//! draw, as the App's does (left from P63). A snapshot save or a delete is a
//! service command; this module writes nothing.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::config::WatchedConfig;
use er_engine::TabKey;
use er_git::{Git, WatchedFile};

use crate::screen::watched::SnapshotDiff;

/// How often the active tab's watched files are listed again, as the App's
/// rescan.
const RESCAN_INTERVAL: Duration = Duration::from_secs(5);

/// A read the worker runs.
enum Request {
    Scan {
        key: TabKey,
        root: String,
        patterns: Vec<String>,
    },
    Diff {
        key: TabKey,
        root: String,
        path: String,
        snapshots: String,
    },
}

/// A read that landed.
pub enum Landed {
    Scanned {
        key: TabKey,
        files: Vec<WatchedFile>,
        not_ignored: Vec<String>,
    },
    Diff {
        key: TabKey,
        path: String,
        diff: SnapshotDiff,
    },
}

/// The worker, and when the active tab's list was last asked for.
pub struct Reader {
    requests: Sender<Request>,
    landed: Receiver<Landed>,
    /// The tab, settings revision and time of the last scan.
    scanned: Option<(TabKey, u64, Instant)>,
    /// A write landed, so the list is read again on the next tick.
    pub rescan: bool,
}

impl Reader {
    /// Starts the worker. It ends when the reader drops.
    pub fn spawn(git: Arc<dyn Git>) -> Self {
        let (requests, inbox) = mpsc::channel::<Request>();
        let (outbox, landed) = mpsc::channel();
        std::thread::spawn(move || {
            for request in inbox {
                if outbox.send(read(&*git, request)).is_err() {
                    return;
                }
            }
        });
        Self {
            requests,
            landed,
            scanned: None,
            rescan: false,
        }
    }

    pub fn try_landed(&self) -> Option<Landed> {
        self.landed.try_recv().ok()
    }

    /// Whether `key`'s list is due: on its first frame, after a write, every
    /// five seconds, and when the settings revision moves, so a hub edit to
    /// the watched paths shows.
    pub fn scan_due(&mut self, key: TabKey, settings_revision: u64, now: Instant) -> bool {
        let due = std::mem::take(&mut self.rescan)
            || self.scanned.is_none_or(|(tab, revision, at)| {
                tab != key
                    || revision != settings_revision
                    || now.saturating_duration_since(at) >= RESCAN_INTERVAL
            });
        if due {
            self.scanned = Some((key, settings_revision, now));
        }
        due
    }

    pub fn scan(&self, key: TabKey, root: &str, config: &WatchedConfig) {
        let _ = self.requests.send(Request::Scan {
            key,
            root: root.to_string(),
            patterns: config.paths.clone(),
        });
    }

    pub fn diff(&self, key: TabKey, root: &str, path: String, er_dir: &str) {
        let _ = self.requests.send(Request::Diff {
            key,
            root: root.to_string(),
            path,
            snapshots: format!("{er_dir}/snapshots"),
        });
    }
}

fn read(git: &dyn Git, request: Request) -> Landed {
    match request {
        Request::Scan {
            key,
            root,
            patterns,
        } => {
            let files = er_git::discover_watched_files(&root, &patterns).unwrap_or_default();
            let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
            let ignored = git.gitignored_paths(&root, &paths);
            let not_ignored = paths
                .into_iter()
                .filter(|path| !ignored.contains(path))
                .collect();
            Landed::Scanned {
                key,
                files,
                not_ignored,
            }
        }
        Request::Diff {
            key,
            root,
            path,
            snapshots,
        } => {
            let diff = match er_git::read_watched_snapshot_diff(&root, &path, &snapshots) {
                Ok(Some(raw)) if raw.is_empty() => SnapshotDiff::Unchanged,
                Ok(Some(raw)) => {
                    SnapshotDiff::Changed(er_engine::git::parse_diff(&raw).into_iter().next())
                }
                Ok(None) => SnapshotDiff::Created,
                Err(_) => SnapshotDiff::Unreadable,
            };
            Landed::Diff { key, path, diff }
        }
    }
}

/// One tab's watched files, as the App's tab keeps them.
#[derive(Default)]
pub struct TabWatched {
    /// Whether the section shows; until `W`, it shows when the config lists
    /// watched paths, as the App's tab starts.
    pub shown: Option<bool>,
    pub files: Vec<WatchedFile>,
    pub not_ignored: Vec<String>,
    pub selected: Option<usize>,
    /// The path a delete prompt asks about.
    pub deleting: Option<String>,
    /// The last snapshot diff read, and its path.
    snapshot: Option<(String, SnapshotDiff)>,
    reading: bool,
    stale: bool,
}

impl TabWatched {
    /// Whether the section shows. The App's switch to Hidden shows it; that
    /// switch is P71's, so here it follows `W` and the config.
    pub fn is_shown(&self, config: &WatchedConfig) -> bool {
        self.shown.unwrap_or(!config.paths.is_empty())
    }

    /// The listed files, filtered by the search, with their index.
    pub fn visible(&self, shown: bool, search: &str) -> Vec<(usize, &WatchedFile)> {
        if !shown {
            return Vec::new();
        }
        let search = search.to_lowercase();
        self.files
            .iter()
            .enumerate()
            .filter(|(_, file)| search.is_empty() || file.path.to_lowercase().contains(&search))
            .collect()
    }

    pub fn selected_file(&self) -> Option<&WatchedFile> {
        self.selected.and_then(|idx| self.files.get(idx))
    }

    /// The snapshot diff read for the selected file.
    pub fn snapshot(&self) -> Option<&SnapshotDiff> {
        let path = &self.selected_file()?.path;
        match &self.snapshot {
            Some((read, diff)) if read == path => Some(diff),
            _ => None,
        }
    }

    /// `W`: shows or hides the section. Answers the status line's message,
    /// and whether the list must be read.
    pub fn toggle(&mut self, config: &WatchedConfig) -> (&'static str, bool) {
        if config.paths.is_empty() {
            return (
                "No watched paths configured — add them in the config hub (,)",
                false,
            );
        }
        let shown = !self.is_shown(config);
        self.shown = Some(shown);
        if shown {
            return ("Watched files shown", true);
        }
        self.clear();
        ("Watched files hidden", false)
    }

    /// The list is gone: the section is hidden or the config lists no
    /// paths, as the App's rescan clears it.
    pub fn clear(&mut self) {
        self.files.clear();
        self.selected = None;
    }

    /// A listing landed. The selection is clamped as the App's rescan does,
    /// and the selected file's diff is read again. A listing that lands
    /// after `W` hid the section is dropped.
    pub fn scanned(&mut self, files: Vec<WatchedFile>, not_ignored: Vec<String>) {
        if self.shown == Some(false) {
            return;
        }
        self.files = files;
        self.not_ignored = not_ignored;
        if let Some(idx) = self.selected {
            self.selected = self.files.len().checked_sub(1).map(|last| idx.min(last));
        }
        self.stale = true;
    }

    /// A snapshot diff landed. Answers the path to save a snapshot of when
    /// the file had none, which the frame shows as created until the save
    /// lands and the diff is read again.
    pub fn read(&mut self, path: String, diff: SnapshotDiff) -> Option<String> {
        self.reading = false;
        let created = matches!(diff, SnapshotDiff::Created).then(|| path.clone());
        self.snapshot = Some((path, diff));
        created
    }

    /// The selected file's diff is read again on the next tick.
    pub fn reread(&mut self) {
        self.stale = true;
    }

    /// The path whose diff to read now: the selection reached a file this
    /// has not read, or something asked for it again.
    pub fn wants_read(&mut self) -> Option<String> {
        if self.reading {
            return None;
        }
        let path = self.selected_file()?.path.clone();
        let read = matches!(&self.snapshot, Some((read, _)) if *read == path);
        if read && !self.stale {
            return None;
        }
        self.reading = true;
        self.stale = false;
        Some(path)
    }
}
