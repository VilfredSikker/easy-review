//! GitHub payload types shared with the engine's review queue.
//!
//! `QueuePr` and `ThreadAddressingSummary` are what `github.rs` returns and
//! parses; the engine re-exports them from `review_queue` so its callers keep
//! their paths.

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

/// Summary of review-thread addressing for "already fixed" detection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadAddressingSummary {
    pub thread_count: usize,
    pub resolved: usize,
    pub outdated: usize,
    pub open: usize,
    /// True when there is at least one thread and every thread is resolved or outdated.
    pub all_addressed: bool,
    /// True when the GraphQL fetch hit a page cap and more threads may exist.
    /// When set, [`Self::all_addressed`] is forced false (conservative).
    #[serde(default)]
    pub truncated: bool,
}

impl ThreadAddressingSummary {
    pub fn from_thread_flags(threads: &[(bool, bool)]) -> Self {
        Self::from_thread_flags_truncated(threads, false)
    }

    pub fn from_thread_flags_truncated(threads: &[(bool, bool)], truncated: bool) -> Self {
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
        let all_addressed = !truncated && thread_count > 0 && open == 0;
        Self {
            thread_count,
            resolved,
            outdated,
            open,
            all_addressed,
            truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn thread_addressing_truncated_forces_not_all_addressed() {
        let s = ThreadAddressingSummary::from_thread_flags_truncated(&[(true, false)], true);
        assert!(s.truncated);
        assert!(!s.all_addressed);
    }
}
