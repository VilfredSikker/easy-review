use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::TabKey;

/// What a tab shows: the worktree's changes, a local branch, a PR from a
/// local clone, or a PR with no clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    Working,
    LocalBranch,
    LocalPr,
    RemotePr,
}

/// One open tab as the strip shows it. Field names follow the snapshot's tab
/// rows, so the flag-off and flag-on reads have one shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TabRow {
    pub key: TabKey,
    pub kind: TabKind,
    /// Built from the saved descriptor. A Working tab names its repo folder,
    /// since the live branch is not part of the descriptor.
    pub label: String,
    pub repo_root: String,
    /// The branch a branch or local PR view shows; none on the other kinds.
    pub branch: Option<String>,
    #[ts(type = "number | null")]
    pub pr_number: Option<u64>,
    /// `owner/repo` of a PR with no clone.
    pub remote: Option<String>,
}

/// One tab's desktop view: its review browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct TabView {
    pub browser: BrowserView,
}

/// One tab's review browser. Field names follow the snapshot's `browser`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct BrowserView {
    pub url: String,
    pub layout: BrowserLayout,
    /// The diff column's share of a split.
    pub split_ratio: f32,
    pub annotate_mode: bool,
    pub show_tooltips: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BrowserLayout {
    Hidden,
    Split,
    Fullscreen,
}

/// What the sidebar row knew about a PR when the pointer rested on it: the
/// open cache keeps a prefetched diff only while these still match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PrOpenHint {
    pub base_ref: String,
    pub head_ref: String,
    pub head_oid: String,
    pub updated_at: String,
}
