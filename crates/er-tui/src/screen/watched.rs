//! The selected watched file's diff against its snapshot. The draw only
//! reads it.

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
