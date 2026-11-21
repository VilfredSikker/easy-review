use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::MatchedRiskRule;

/// One open tab's diff: the file list in the order the list shows it, and
/// what the toolbar and header read about it. Field names follow the
/// snapshot's, so the flag-off and flag-on reads have one shape.
///
/// A key the registry lists but no workspace has loaded answers this empty at
/// version 0; the push after the load brings the rows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct TabDiff {
    /// `branch`, `unstaged`, `staged`, `history`, `conflicts`, `hidden`,
    /// `pr` or `tour`. The three views the workspace only records carry no
    /// files: the page reads those from the App until they load in the service.
    pub mode: String,
    pub branch: String,
    pub base: String,
    pub files: Vec<FileRow>,
    /// Every file in the diff, whatever the filter keeps.
    #[ts(type = "number")]
    pub total_count: u64,
    pub diff_stats: DiffLineStats,
    pub filter: Option<String>,
    pub filter_suggestions: Vec<FilterSuggestion>,
    pub preview_context_key: String,
    pub review_bucket_er_dir: String,
    /// The branch a read-only branch view shows.
    pub local_branch: Option<String>,
    pub local_branch_checked_out: bool,
    pub unstaged_stat: ScopeStat,
    pub staged_stat: ScopeStat,
    /// The PR head the diff was built from, which the stale mark compares with
    /// the PR list's newest head. None for a tab with no PR.
    pub built_from_head_oid: Option<String>,
    /// The head the PR diff on screen was fetched at; set only while the view
    /// shows the PR's own head.
    pub pr_diff_head_oid: Option<String>,
    /// A branch view's base when origin's tip of it has moved past the commit
    /// the diff was built from.
    pub base_behind: Option<BaseBehind>,
    /// The checkout against its PR's head in the PR list.
    pub local_branch_divergence: Option<BranchDivergence>,
    /// Why the tab's last load failed. Set only while nothing loaded; a
    /// refresh loads again.
    pub load_error: Option<String>,
}

/// A branch view's base, built from one commit while origin's tip is another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BaseBehind {
    pub built_from_oid: String,
    pub latest_oid: String,
    /// Commits the tip has that the base lacks; none while either commit is
    /// missing locally.
    #[ts(type = "number | null")]
    pub commits_behind: Option<u64>,
}

/// A checkout against its PR's head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BranchDivergence {
    pub pr_head_oid: String,
    /// Commits on the checkout's HEAD that the PR head lacks; none while the
    /// PR head is missing locally.
    #[ts(type = "number | null")]
    pub ahead: Option<u64>,
    #[ts(type = "number")]
    pub unstaged_files: u64,
}

/// One file as the list shows it, without its hunks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct FileRow {
    pub path: String,
    pub status: String,
    /// The source path of a rename or copy.
    pub old_path: Option<String>,
    pub similarity: Option<u8>,
    /// The file kind after the repo's `[file_kinds]` overrides.
    pub kind: String,
    #[ts(type = "number")]
    pub additions: u64,
    #[ts(type = "number")]
    pub deletions: u64,
    pub reviewed: bool,
    pub compacted: bool,
    /// The AI review's rating; none before a review.
    pub risk: Option<String>,
    /// The tier the repo's risk rules give the path.
    pub risk_tier: String,
    pub risk_rule: Option<MatchedRiskRule>,
    #[ts(type = "number")]
    pub finding_count: u64,
    #[ts(type = "number")]
    pub comment_count: u64,
    #[ts(type = "number")]
    pub question_count: u64,
    /// Listed but not parsed yet.
    pub is_lazy_stub: bool,
    /// The file's position in the whole diff, which file commands name.
    #[ts(type = "number")]
    pub source_index: u64,
    /// Hash of the file's own hunk lines; moves only when they do.
    pub cache_key: String,
    pub preview_key: String,
}

/// Files and +/- lines in one scope.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ScopeStat {
    #[ts(type = "number")]
    pub files: u64,
    #[ts(type = "number")]
    pub additions: u64,
    #[ts(type = "number")]
    pub deletions: u64,
}

/// Line counts for the whole diff, whatever the filter keeps; `code` leaves
/// out tests, Storybook, generated files and docs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiffLineStats {
    pub total: ScopeStat,
    pub code: ScopeStat,
}

/// A filter the file tree offers: one per file kind present, the presets,
/// then the tab's recent filters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FilterSuggestion {
    /// `kind`, `preset` or `history`.
    pub kind: String,
    pub name: String,
    pub expr: String,
    /// How many files a `kind` suggestion selects.
    #[ts(type = "number | null")]
    pub files: Option<u64>,
}
