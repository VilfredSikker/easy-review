//! What a review reads from the tab it reviews. The App reads it from its
//! tab and the workspace service from a tab's published content, both by the
//! rules here, so the two hosts review the same diff of the same view.

use crate::tab_identity::{DiffMode, TabIdentity};

/// Whether the diff a tab shows is the one a review of `scope` reads, so a
/// review need not fetch it again. A remote tab and a branch view with no
/// checkout show their one diff in every view.
pub fn cached_diff_matches_scope(id: &TabIdentity<'_>, scope: &str) -> bool {
    if id.remote_repo.is_some() {
        return true;
    }
    if id.local_branch_view.is_some() && id.checkout_root.is_none() {
        return true;
    }
    match scope {
        "unstaged" => id.mode == DiffMode::Unstaged,
        "staged" => id.mode == DiffMode::Staged,
        // Tour is the branch diff regrouped, so its cached raw_diff is the
        // branch diff and matches a "branch"-scoped review.
        _ => matches!(
            id.mode,
            DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour
        ),
    }
}
