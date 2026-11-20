//! What a file row in a diff list shows beside its hunks: its status and
//! risk names, its content key and its open finding count. Both the snapshot
//! and the `diff(tab)` builder read these, so the two lists cannot drift.

use std::hash::{Hash, Hasher};

use crate::ai::{AiState, ArbiterRuling, ErFileReview, Finding, RiskLevel};
use crate::git::{DiffFile, FileStatus, LineType};
use crate::tab_identity::DiffMode;

/// Stable content hash over exactly what the hunk payload serializes for a
/// file (headers, line kinds/numbers, text). Per-file, so editing one file
/// does not re-key the others and highlight caches survive watch refreshes.
pub fn file_lines_key(f: &DiffFile) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.path.hash(&mut h);
    f.compacted.hash(&mut h);
    f.hunks.len().hash(&mut h);
    for hunk in &f.hunks {
        hunk.header.hash(&mut h);
        hunk.old_start.hash(&mut h);
        hunk.new_start.hash(&mut h);
        hunk.lines.len().hash(&mut h);
        for line in &hunk.lines {
            match line.line_type {
                LineType::Add => 1u8.hash(&mut h),
                LineType::Delete => 2u8.hash(&mut h),
                LineType::Context => 3u8.hash(&mut h),
                LineType::Fold(hidden) => {
                    4u8.hash(&mut h);
                    hidden.hash(&mut h);
                }
            }
            line.old_num.hash(&mut h);
            line.new_num.hash(&mut h);
            line.content.hash(&mut h);
        }
    }
    h.finish()
}

/// The name the desktop gives a diff mode.
pub const fn mode_str(mode: DiffMode) -> &'static str {
    match mode {
        DiffMode::Branch => "branch",
        DiffMode::Unstaged => "unstaged",
        DiffMode::Staged => "staged",
        DiffMode::History => "history",
        DiffMode::Conflicts => "conflicts",
        DiffMode::Hidden => "hidden",
        DiffMode::PrDiff => "pr",
        DiffMode::Tour => "tour",
    }
}

pub const fn severity_str(r: RiskLevel) -> &'static str {
    match r {
        RiskLevel::High => "high",
        RiskLevel::Medium => "med",
        RiskLevel::Low | RiskLevel::Info => "low",
    }
}

pub fn status_str(status: &FileStatus) -> String {
    match status {
        FileStatus::Added => "added",
        FileStatus::Modified => "modified",
        FileStatus::Deleted => "deleted",
        FileStatus::Renamed(_) => "renamed",
        FileStatus::Copied(_) => "copied",
        FileStatus::Unmerged => "unmerged",
    }
    .to_string()
}

pub fn finding_is_escalated(ai: &AiState, path: &str, finding: &Finding) -> bool {
    matches!(
        ai.arbiter_verdicts.get(&crate::ai::finding_key(
            path,
            finding.line_start,
            &finding.title
        )),
        Some((ArbiterRuling::Escalated, _))
    )
}

/// The open findings the review shows for `file`, or for every file.
pub fn active_finding_count(ai: &AiState, file: Option<&str>) -> usize {
    let Some(review) = &ai.review else { return 0 };
    let files: Box<dyn Iterator<Item = (&String, &ErFileReview)> + '_> = match file {
        Some(file) => Box::new(review.files.get_key_value(file).into_iter()),
        None => Box::new(review.files.iter()),
    };
    let rows = files.flat_map(|(path, review)| {
        review.findings.iter().filter_map(move |finding| {
            (!finding.resolved && (finding.is_active() || finding_is_escalated(ai, path, finding)))
                .then_some((path.as_str(), finding))
        })
    });
    crate::ai::project_visible_findings(rows, &ai.arbiter_graded_keys).len()
}
