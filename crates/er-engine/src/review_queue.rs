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
    /// Current user's latest review state on this PR, if any
    /// (`APPROVED` / `CHANGES_REQUESTED` / `COMMENTED` / `DISMISSED` / …).
    #[serde(default)]
    pub my_latest_review_state: Option<String>,
    /// Optional CI signal when enriched: `failing` | `passing` | `pending` | `unknown`.
    #[serde(default)]
    pub checks_state: Option<String>,
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

fn is_open_non_draft(pr: &QueuePr) -> bool {
    pr.state.eq_ignore_ascii_case("OPEN") && !pr.is_draft
}

/// True when the authenticated user still owes a review on this PR.
pub fn is_review_debt(pr: &QueuePr) -> bool {
    if !is_open_non_draft(pr) {
        return false;
    }
    match pr
        .my_latest_review_state
        .as_deref()
        .map(|s| s.to_ascii_uppercase())
        .as_deref()
    {
        Some("APPROVED") | Some("CHANGES_REQUESTED") | Some("DISMISSED") => false,
        _ => pr.review_requested_of_me,
    }
}

/// PRs where you were requested and have not finished a review decision.
pub fn filter_review_debt(prs: &[QueuePr], limit: usize) -> Vec<RankedPr> {
    let mut ranked: Vec<_> = prs
        .iter()
        .filter(|p| is_review_debt(p))
        .map(score_pr)
        .collect();
    ranked.sort_by(|a, b| {
        b.priority_score
            .cmp(&a.priority_score)
            .then_with(|| a.total_lines.cmp(&b.total_lines))
    });
    ranked.truncate(limit);
    ranked
}

/// Parse a GitHub `updatedAt` ISO-8601 timestamp into epoch seconds.
pub fn parse_github_updated_at(updated_at: &str) -> Option<i64> {
    // Accept `2026-07-01T00:00:00Z` and `2026-07-01T00:00:00+00:00`.
    let s = updated_at.trim();
    if s.is_empty() {
        return None;
    }
    // Prefer chrono-free parsing: take YYYY-MM-DDTHH:MM:SS prefix.
    let (date, rest) = s.split_once('T')?;
    let mut dp = date.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let time = rest.trim_end_matches('Z');
    let time = time.split(['+', '-']).next().unwrap_or(time);
    let mut tp = time.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let minute: i64 = tp.next()?.parse().ok()?;
    let second: i64 = tp
        .next()
        .and_then(|x| x.split('.').next())
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    // Days from civil date → Unix (algorithm from Howard Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

/// Age of a PR's `updated_at` in whole days, relative to `now_epoch_secs`.
pub fn age_days(pr: &QueuePr, now_epoch_secs: i64) -> Option<u64> {
    let updated = parse_github_updated_at(&pr.updated_at)?;
    let age = now_epoch_secs.saturating_sub(updated);
    if age < 0 {
        Some(0)
    } else {
        Some((age as u64) / 86_400)
    }
}

/// True when the PR has had no GitHub activity for at least `days`.
pub fn is_stale(pr: &QueuePr, days: u64, now_epoch_secs: i64) -> bool {
    is_open_non_draft(pr) && age_days(pr, now_epoch_secs).is_some_and(|age| age >= days)
}

pub fn filter_stale(
    prs: &[QueuePr],
    days: u64,
    now_epoch_secs: i64,
    limit: usize,
) -> Vec<RankedPr> {
    let mut ranked: Vec<_> = prs
        .iter()
        .filter(|p| is_stale(p, days, now_epoch_secs))
        .map(score_pr)
        .collect();
    ranked.sort_by(|a, b| {
        // Oldest first.
        let age_a = age_days(&a.pr, now_epoch_secs).unwrap_or(0);
        let age_b = age_days(&b.pr, now_epoch_secs).unwrap_or(0);
        age_b
            .cmp(&age_a)
            .then_with(|| a.pr.number.cmp(&b.pr.number))
    });
    ranked.truncate(limit);
    ranked
}

/// Merge conflicts or dirty merge state.
pub fn has_merge_conflicts(pr: &QueuePr) -> bool {
    let mergeable = pr.mergeable.as_deref().unwrap_or("").to_ascii_uppercase();
    let mss = pr
        .merge_state_status
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();
    mergeable == "CONFLICTING" || mss == "DIRTY"
}

/// GitHub reports merge state blocked (often CI / review gates).
pub fn is_merge_state_blocked(pr: &QueuePr) -> bool {
    pr.merge_state_status
        .as_deref()
        .unwrap_or("")
        .eq_ignore_ascii_case("BLOCKED")
}

/// Conflicts, failing CI (when known), or mergeStateStatus=BLOCKED.
pub fn is_blocked(pr: &QueuePr) -> bool {
    if !is_open_non_draft(pr) {
        return false;
    }
    if has_merge_conflicts(pr) || is_merge_state_blocked(pr) {
        return true;
    }
    matches!(
        pr.checks_state
            .as_deref()
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("failing") | Some("fail")
    )
}

pub fn filter_blocked(prs: &[QueuePr], limit: usize) -> Vec<RankedPr> {
    let mut ranked: Vec<_> = prs.iter().filter(|p| is_blocked(p)).map(score_pr).collect();
    ranked.sort_by_key(|b| std::cmp::Reverse(b.priority_score));
    ranked.truncate(limit);
    ranked
}

pub fn filter_failing_ci(prs: &[QueuePr], limit: usize) -> Vec<RankedPr> {
    let mut ranked: Vec<_> = prs
        .iter()
        .filter(|p| {
            is_open_non_draft(p)
                && matches!(
                    p.checks_state
                        .as_deref()
                        .map(|s| s.to_ascii_lowercase())
                        .as_deref(),
                    Some("failing") | Some("fail")
                )
        })
        .map(score_pr)
        .collect();
    ranked.sort_by_key(|a| a.pr.number);
    ranked.truncate(limit);
    ranked
}

/// Summary of review-thread addressing for "already fixed" detection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadAddressingSummary {
    pub thread_count: usize,
    pub resolved: usize,
    pub outdated: usize,
    pub open: usize,
    /// True when there is at least one thread and every thread is resolved or outdated.
    pub all_addressed: bool,
}

impl ThreadAddressingSummary {
    pub fn from_thread_flags(threads: &[(bool, bool)]) -> Self {
        // (resolved, outdated)
        let thread_count = threads.len();
        let mut resolved = 0;
        let mut outdated = 0;
        let mut open = 0;
        for &(is_resolved, is_outdated) in threads {
            if is_resolved {
                resolved += 1;
            } else if is_outdated {
                outdated += 1;
            } else {
                open += 1;
            }
        }
        Self {
            thread_count,
            resolved,
            outdated,
            open,
            all_addressed: thread_count > 0 && open == 0,
        }
    }
}

/// How to open a PR in Easy Review (no OS deep-link yet — instructions + URLs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenInEasyReview {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub github_url: String,
    pub desktop_hint: String,
    pub tui_command: String,
    pub note: String,
}

pub fn open_in_easy_review(owner: &str, repo: &str, number: u64) -> OpenInEasyReview {
    OpenInEasyReview {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
        github_url: format!("https://github.com/{owner}/{repo}/pull/{number}"),
        desktop_hint: format!(
            "In Easy Review Desktop: open project for {owner}/{repo}, then open PR #{number} from the sidebar (or paste the GitHub URL)."
        ),
        tui_command: format!("er --pr {number}   # run inside a clone of {owner}/{repo}"),
        note: "No er:// deep-link handler exists yet; use the desktop open-PR flow or the TUI --pr flag."
            .into(),
    }
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
            my_latest_review_state: None,
            checks_state: None,
        }
    }

    #[test]
    fn tiny_ready_pr_outranks_huge_one() {
        let small = pr(1, 5, 2);
        let mut huge = pr(2, 900, 400);
        huge.review_requested_of_me = true;
        let ranked = rank_priority(&[huge.clone(), small.clone()], 2);
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

    #[test]
    fn review_debt_requires_outstanding_request() {
        let mut p = pr(1, 10, 0);
        p.review_requested_of_me = true;
        assert!(is_review_debt(&p));
        p.my_latest_review_state = Some("APPROVED".into());
        assert!(!is_review_debt(&p));
    }

    #[test]
    fn stale_uses_updated_at_age() {
        let p = pr(1, 10, 0);
        // 2026-07-01T00:00:00Z → epoch
        let updated = parse_github_updated_at("2026-07-01T00:00:00Z").unwrap();
        let now = updated + 10 * 86_400;
        assert!(is_stale(&p, 7, now));
        assert!(!is_stale(&p, 14, now));
    }

    #[test]
    fn thread_addressing_requires_threads() {
        assert!(!ThreadAddressingSummary::from_thread_flags(&[]).all_addressed);
        let s = ThreadAddressingSummary::from_thread_flags(&[(true, false), (false, true)]);
        assert!(s.all_addressed);
        assert_eq!(s.open, 0);
        let s2 = ThreadAddressingSummary::from_thread_flags(&[(false, false)]);
        assert!(!s2.all_addressed);
        assert_eq!(s2.open, 1);
    }

    #[test]
    fn blocked_detects_conflicts_and_failing_ci() {
        let mut p = pr(1, 10, 0);
        p.mergeable = Some("CONFLICTING".into());
        assert!(is_blocked(&p));
        let mut p2 = pr(2, 10, 0);
        p2.checks_state = Some("failing".into());
        assert!(is_blocked(&p2));
        let mut p3 = pr(3, 10, 0);
        p3.merge_state_status = Some("BLOCKED".into());
        assert!(is_blocked(&p3));
    }
}
