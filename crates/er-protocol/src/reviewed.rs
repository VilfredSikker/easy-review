use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One open tab's reviewed marks and counts, as its workspace saved them.
///
/// A key the registry lists but no workspace has loaded answers this empty at
/// version 0, as `diff(tab)` does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TabReviewed {
    /// The reviewed paths among the diff's files, in path order. A mark on a
    /// file the diff no longer holds is kept on disk and left out here.
    pub paths: Vec<String>,
    /// How many of the diff's files are reviewed, whatever the filter keeps.
    #[ts(type = "number")]
    pub reviewed_count: u64,
    /// Every file in the diff, whatever the filter keeps.
    #[ts(type = "number")]
    pub total_count: u64,
    /// The view these marks were read in. A write that paints from them sends
    /// it back, and the backend refuses it once the tab shows another view.
    pub view: PaintedView,
}

/// The view a page painted a write on: what the tab showed when the page read
/// it. Field names follow the snapshot's view parts, without the tab index,
/// which the write's `TabKey` replaces.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PaintedView {
    pub repo_root: String,
    #[ts(type = "number | null")]
    pub pr_number: Option<u64>,
    /// The branch a read-only branch view shows; empty on any other tab.
    pub branch: String,
    /// `branch`, `unstaged`, `staged`, `history`, `conflicts`,
    /// `hidden`, `pr` or `tour`.
    pub mode: String,
}
