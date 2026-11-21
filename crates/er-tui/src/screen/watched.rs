//! The selected watched file's diff against its snapshot. The draw only
//! reads it; the loop reads the port when the selection reaches a watched
//! file, after `s` updates the snapshot, and on the watched-file rescan.

use er_engine::app::App;
use er_engine::git::DiffFile;

/// What the diff view shows for a watched file in snapshot mode.
pub enum SnapshotDiff {
    Unchanged,
    Changed(Option<DiffFile>),
    /// There was no snapshot, and reading the diff saved one.
    Created,
    /// The diff could not be read; the view shows the file's content.
    Unreadable,
}

/// The last snapshot diff read, keyed by the repo and the watched path.
#[derive(Default)]
pub struct WatchedSnapshot {
    read: Option<(String, String, SnapshotDiff)>,
}

impl WatchedSnapshot {
    /// Reads the diff when the selection is on a watched file this has not
    /// read. A `Created` read is read again on the next frame, as the draw
    /// did, so the first-view notice shows for one frame.
    pub fn sync(&mut self, app: &App) {
        let Some((root, path)) = selected(app) else {
            self.read = None;
            return;
        };
        match &self.read {
            Some((r, p, diff))
                if r == root && p == path && !matches!(diff, SnapshotDiff::Created) => {}
            _ => self.reload(app),
        }
    }

    /// Reads the selected watched file's diff again.
    pub fn reload(&mut self, app: &App) {
        self.read = selected(app).map(|(root, path)| {
            let tab = app.tab();
            let diff = match tab.ports.git.diff_watched_file_snapshot(
                root,
                path,
                &tab.er_root.snapshots_dir(),
            ) {
                Ok(Some(raw)) if raw.is_empty() => SnapshotDiff::Unchanged,
                Ok(Some(raw)) => {
                    SnapshotDiff::Changed(er_engine::git::parse_diff(&raw).into_iter().next())
                }
                Ok(None) => SnapshotDiff::Created,
                Err(_) => SnapshotDiff::Unreadable,
            };
            (root.to_owned(), path.to_owned(), diff)
        });
    }

    /// The diff read for `path` in the repo at `root`.
    pub fn diff(&self, root: &str, path: &str) -> Option<&SnapshotDiff> {
        match &self.read {
            Some((r, p, diff)) if r == root && p == path => Some(diff),
            _ => None,
        }
    }
}

/// The selected watched file, when the tab diffs watched files against a snapshot.
fn selected(app: &App) -> Option<(&str, &str)> {
    let tab = app.tab();
    if tab.watched_config.diff_mode != "snapshot" {
        return None;
    }
    tab.selected_watched_file()
        .map(|file| (tab.repo_root.as_str(), file.path.as_str()))
}
