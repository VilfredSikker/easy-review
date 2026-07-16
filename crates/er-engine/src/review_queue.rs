//! Heuristic PR review-queue ranking and status labels.
//!
//! Pure scoring — no network I/O. Callers supply GitHub metadata (and optionally
//! production-only line counts) from `gh` / diff stats.

use serde::{Deserialize, Serialize};

/// Open-PR metadata used for ranking / status classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuePr {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub is_draft: bool,
    pub state: String,
    #[serde(default)]
    pub review_decision: Option<String>,
    /// GitHub `mergeable`: MERGEABLE | CONFLICTING | UNKNOWN
    #[serde(default)]
    pub mergeable: Option<String>,
    /// GitHub `mergeStateStatus`: BEHIND | BLOCKED | CLEAN | DIRTY | DRAFT | …
    #[serde(default)]
    pub merge_state_status: Option<String>,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<String>,
    /// Pending review-request logins.
    #[serde(default)]
    pub reviewers: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub head_ref: String,
    #[serde(default)]
    pub base_ref: String,
    /// When known, production-only changed lines (adds+dels). Overrides size scoring.
    #[serde(default)]
    pub production_lines: Option<u64>,
    /// True when the current user was explicitly requested as a reviewer.
    #[serde(default)]
    pub review_requested_of_me: bool,
}

/// Why a PR sits where it does in the queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedPr {
    pub pr: QueuePr,
    /// Higher = review sooner.
    pub priority_score: i32,
    pub status: ReviewStatus,
    pub reasons: Vec<String>,
    pub total_lines: u64,
    pub size_bucket: SizeBucket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeBucket {
    Xsmall,
    Small,
    Medium,
    Large,
    Xlarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    /// Open, not draft, looks reviewable.
    ReadyToReview,
    /// Author marked draft.
    Draft,
    /// Needs rebase onto base (`mergeStateStatus == BEHIND`).
    Outdated,
    /// Merge conflicts (`CONFLICTING` / `DIRTY`).
    BlockedConflicts,
    /// Reviewer requested changes — waiting on author.
    WaitingOnAuthor,
    /// Already approved (less urgent for reviewers).
    Approved,
    /// Approved + clean merge state.
    MergeReady,
    /// Closed / merged — usually filtered out of active queues.
    Inactive,
    /// Catch-all when signals conflict or are incomplete.
    Unknown,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadyToReview => "ready_to_review",
            Self::Draft => "draft",
            Self::Outdated => "outdated",
            Self::BlockedConflicts => "blocked_conflicts",
            Self::WaitingOnAuthor => "waiting_on_author",
            Self::Approved => "approved",
            Self::MergeReady => "merge_ready",
            Self::Inactive => "inactive",
            Self::Unknown => "unknown",
        }
    }
}

/// Classify a PR's review/workflow status from GitHub fields.
pub fn classify_status(pr: &QueuePr) -> ReviewStatus {
    let state = pr.state.to_ascii_uppercase();
    if state == "MERGED" || state == "CLOSED" {
        return ReviewStatus::Inactive;
    }
    if pr.is_draft {
        return ReviewStatus::Draft;
    }

    let mergeable = pr.mergeable.as_deref().unwrap_or("").to_ascii_uppercase();
    let mss = pr
        .merge_state_status
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();

    if mergeable == "CONFLICTING" || mss == "DIRTY" {
        return ReviewStatus::BlockedConflicts;
    }
    if mss == "BEHIND" {
        return ReviewStatus::Outdated;
    }

    let decision = pr
        .review_decision
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();
    if decision == "CHANGES_REQUESTED" {
        return ReviewStatus::WaitingOnAuthor;
    }
    if decision == "APPROVED" {
        if mss == "CLEAN" || mergeable == "MERGEABLE" {
            return ReviewStatus::MergeReady;
        }
        return ReviewStatus::Approved;
    }

    if state == "OPEN" {
        return ReviewStatus::ReadyToReview;
    }
    ReviewStatus::Unknown
}

fn size_lines(pr: &QueuePr) -> u64 {
    pr.production_lines
        .unwrap_or_else(|| pr.additions.saturating_add(pr.deletions))
}

pub fn size_bucket(lines: u64) -> SizeBucket {
    match lines {
        0..=20 => SizeBucket::Xsmall,
        21..=80 => SizeBucket::Small,
        81..=250 => SizeBucket::Medium,
        251..=800 => SizeBucket::Large,
        _ => SizeBucket::Xlarge,
    }
}

/// Score one PR. Higher means "review sooner".
pub fn score_pr(pr: &QueuePr) -> RankedPr {
    let status = classify_status(pr);
    let total_lines = size_lines(pr);
    let size_bucket = size_bucket(total_lines);
    let mut score: i32 = 0;
    let mut reasons = Vec::new();

    match status {
        ReviewStatus::Inactive => {
            score -= 10_000;
            reasons.push("inactive".into());
        }
        ReviewStatus::Draft => {
            score -= 500;
            reasons.push("draft".into());
        }
        ReviewStatus::BlockedConflicts => {
            score -= 80;
            reasons.push("merge conflicts".into());
        }
        ReviewStatus::Outdated => {
            score -= 40;
            reasons.push("outdated vs base (needs rebase)".into());
        }
        ReviewStatus::WaitingOnAuthor => {
            score -= 60;
            reasons.push("changes requested — waiting on author".into());
        }
        ReviewStatus::Approved | ReviewStatus::MergeReady => {
            score -= 30;
            reasons.push("already approved".into());
        }
        ReviewStatus::ReadyToReview => {
            score += 40;
            reasons.push("ready to review".into());
        }
        ReviewStatus::Unknown => {}
    }

    if pr.review_requested_of_me {
        score += 50;
        reasons.push("review requested of you".into());
    } else if !pr.reviewers.is_empty() {
        score += 10;
        reasons.push("has pending reviewers".into());
    }

    // Smaller production surface → higher priority (low-hanging fruit).
    let size_bonus = match size_bucket {
        SizeBucket::Xsmall => 45,
        SizeBucket::Small => 30,
        SizeBucket::Medium => 10,
        SizeBucket::Large => -15,
        SizeBucket::Xlarge => -40,
    };
    score += size_bonus;
    reasons.push(format!(
        "size={:?} ({} lines{})",
        size_bucket,
        total_lines,
        if pr.production_lines.is_some() {
            " production"
        } else {
            " total"
        }
    ));

    for label in &pr.labels {
        let l = label.to_ascii_lowercase();
        if matches!(
            l.as_str(),
            "priority" | "urgent" | "critical" | "blocking" | "p0" | "p1"
        ) {
            score += 35;
            reasons.push(format!("label:{label}"));
        }
    }

    RankedPr {
        pr: pr.clone(),
        priority_score: score,
        status,
        reasons,
        total_lines,
        size_bucket,
    }
}

/// Rank open PRs for a "what should I review next?" answer.
pub fn rank_priority(prs: &[QueuePr], limit: usize) -> Vec<RankedPr> {
    let mut ranked: Vec<_> = prs.iter().map(score_pr).collect();
    ranked.sort_by(|a, b| {
        b.priority_score
            .cmp(&a.priority_score)
            .then_with(|| a.total_lines.cmp(&b.total_lines))
            .then_with(|| a.pr.number.cmp(&b.pr.number))
    });
    ranked.truncate(limit);
    ranked
}

/// Smallest reviewable PRs (skips drafts / inactive by default).
pub fn rank_low_hanging(prs: &[QueuePr], limit: usize, include_drafts: bool) -> Vec<RankedPr> {
    let mut ranked: Vec<_> = prs
        .iter()
        .filter(|p| {
            let s = p.state.to_ascii_uppercase();
            s == "OPEN" && (include_drafts || !p.is_draft)
        })
        .map(score_pr)
        .collect();
    ranked.sort_by(|a, b| {
        a.total_lines
            .cmp(&b.total_lines)
            .then_with(|| b.priority_score.cmp(&a.priority_score))
            .then_with(|| a.pr.number.cmp(&b.pr.number))
    });
    ranked.truncate(limit);
    ranked
}

/// Filter ranked results by status label.
pub fn filter_by_status(prs: &[QueuePr], status: ReviewStatus) -> Vec<RankedPr> {
    prs.iter()
        .map(score_pr)
        .filter(|r| r.status == status)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(number: u64, adds: u64, dels: u64) -> QueuePr {
        QueuePr {
            number,
            title: format!("PR {number}"),
            author: "alice".into(),
            is_draft: false,
            state: "OPEN".into(),
            review_decision: Some("REVIEW_REQUIRED".into()),
            mergeable: Some("MERGEABLE".into()),
            merge_state_status: Some("CLEAN".into()),
            additions: adds,
            deletions: dels,
            changed_files: 3,
            updated_at: "2026-07-01T00:00:00Z".into(),
            labels: vec![],
            reviewers: vec![],
            url: format!("https://github.com/o/r/pull/{number}"),
            head_ref: "feat".into(),
            base_ref: "main".into(),
            production_lines: None,
            review_requested_of_me: false,
        }
    }

    #[test]
    fn tiny_ready_pr_outranks_huge_one() {
        let small = pr(1, 5, 2);
        let mut huge = pr(2, 900, 400);
        huge.review_requested_of_me = true; // still shouldn't beat tiny ready PR by much... actually +50
        let ranked = rank_priority(&[huge.clone(), small.clone()], 2);
        // Small ready (+40+45) = 85; huge requested (+40-40+50)=50 → small first
        assert_eq!(ranked[0].pr.number, 1);
    }

    #[test]
    fn low_hanging_sorts_by_size() {
        let a = pr(10, 100, 0);
        let b = pr(11, 10, 0);
        let c = pr(12, 50, 0);
        let ranked = rank_low_hanging(&[a, b, c], 3, false);
        assert_eq!(
            ranked.iter().map(|r| r.pr.number).collect::<Vec<_>>(),
            vec![11, 12, 10]
        );
    }

    #[test]
    fn classifies_outdated_and_blocked() {
        let mut behind = pr(1, 10, 0);
        behind.merge_state_status = Some("BEHIND".into());
        assert_eq!(classify_status(&behind), ReviewStatus::Outdated);

        let mut dirty = pr(2, 10, 0);
        dirty.mergeable = Some("CONFLICTING".into());
        assert_eq!(classify_status(&dirty), ReviewStatus::BlockedConflicts);

        let mut cr = pr(3, 10, 0);
        cr.review_decision = Some("CHANGES_REQUESTED".into());
        assert_eq!(classify_status(&cr), ReviewStatus::WaitingOnAuthor);
    }

    #[test]
    fn production_lines_override_github_totals() {
        let mut p = pr(1, 500, 500);
        p.production_lines = Some(12);
        let ranked = score_pr(&p);
        assert_eq!(ranked.total_lines, 12);
        assert_eq!(ranked.size_bucket, SizeBucket::Xsmall);
    }
}
