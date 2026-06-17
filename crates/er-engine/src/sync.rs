//! Pure sync core — GitHub comment merge/anchoring and remote diff refresh
//! without any `App`/`TabState` dependency.
//!
//! The TUI and desktop call these through thin `App` wrappers in
//! `app/state/github_sync.rs` and `app/state/remote_diff_sync.rs` (three-phase
//! pattern: snapshot under the App lock → fetch/process here without the lock →
//! apply under the lock). Headless consumers (the er-api server) call these
//! directly with their own session state.
//!
//! This module is always compiled — it must not depend on any feature-gated
//! module (`app`, `arena`, `watch`, `highlight`).

use anyhow::Result;

use crate::ai;
use crate::git;
use crate::github;
use crate::github::PrOverviewData;
use crate::github::ReviewThreadState;

// ── Timestamp helper ──────────────────────────────────────────────────────────

/// Simple ISO 8601 UTC timestamp (no external crate needed).
/// Kept in ISO format so .er-feedback.json timestamps are human-readable.
pub fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_iso8601(secs)
}

/// Format a Unix timestamp (seconds since the epoch) as an ISO 8601 UTC string.
///
/// Pure calendar math — no external crate and no system clock — so the leap-year
/// and month-walking logic is unit-testable with known inputs.
fn format_iso8601(secs: u64) -> String {
    let days = secs / 86400;
    let remaining = secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    // Walk years from epoch, subtracting days per year (handles leap years via Gregorian rule)
    let mut y = 1970i64;
    let mut d = i64::try_from(days).unwrap_or(i64::MAX);
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }

    // Walk months within the year (m is 0-indexed, d ends as 0-indexed day-of-month)
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for md in &month_days {
        if d < *md {
            break;
        }
        d -= *md;
        m += 1;
    }
    // Guard against overflow past December (shouldn't happen, but be safe)
    if m >= 12 {
        m = 11;
        d = 0;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m + 1,
        d + 1,
        hours,
        minutes,
        seconds
    )
}

// ── Two-phase background comment sync ─────────────────────────────────────────

/// Data snapshotted from App state before releasing the lock.
/// Contains everything needed to perform a comment sync without holding App.
pub struct CommentSyncContext {
    pub owner: String,
    pub repo_name: String,
    pub pr_number: u64,
    pub is_remote: bool,
    pub repo_root: String,
    pub comments_path: String,
    /// SHA-256 of the branch diff (stored in comments JSON)
    pub diff_hash: String,
    /// Current diff hash (for anchor status on new comments)
    pub anchor_hash: String,
    /// File list snapshot for content-based hunk matching
    pub files: Vec<git::DiffFile>,
    /// pr_number from the tab field (may differ from pr_number in local mode)
    pub pr_number_for_overview: Option<u64>,
}

/// Pre-processed results ready to apply to App state.
pub struct CommentSyncResult {
    pub gc: ai::ErGitHubComments,
    pub pr_data: Option<PrOverviewData>,
    pub github_count: usize,
    pub local_count: usize,
    pub is_remote: bool,
    pub comments_path: String,
    /// Tab identity for safe application without race conditions.
    /// (repo_root, pr_number, is_remote)
    pub tab_key: (String, Option<u64>, bool),
}

pub(crate) fn merged_outdated_state(thread_state: ReviewThreadState, rest_outdated: bool) -> bool {
    thread_state.outdated || rest_outdated
}

/// Perform the network I/O and data processing for a comment sync.
/// Does NOT hold the App mutex — all data comes from `CommentSyncContext`.
/// Writes the comments JSON file to disk before returning.
pub fn fetch_comment_sync_data(ctx: &CommentSyncContext) -> Result<CommentSyncResult> {
    let gh_comments = if ctx.is_remote {
        github::gh_pr_comments_remote(&ctx.owner, &ctx.repo_name, ctx.pr_number)?
    } else {
        github::gh_pr_comments(&ctx.owner, &ctx.repo_name, ctx.pr_number, &ctx.repo_root)?
    };

    let thread_state = if ctx.is_remote {
        github::gh_pr_review_threads_remote(&ctx.owner, &ctx.repo_name, ctx.pr_number)
            .unwrap_or_default()
    } else {
        github::gh_pr_review_threads(&ctx.owner, &ctx.repo_name, ctx.pr_number, &ctx.repo_root)
            .unwrap_or_default()
    };

    let mut gc: ai::ErGitHubComments = match std::fs::read_to_string(&ctx.comments_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| ai::ErGitHubComments {
            version: 1,
            diff_hash: ctx.diff_hash.clone(),
            github: None,
            comments: Vec::new(),
        }),
        Err(_) => ai::ErGitHubComments {
            version: 1,
            diff_hash: ctx.diff_hash.clone(),
            github: None,
            comments: Vec::new(),
        },
    };

    gc.github = Some(ai::GitHubSyncState {
        pr_number: Some(ctx.pr_number),
        owner: ctx.owner.clone(),
        repo: ctx.repo_name.clone(),
        last_synced: chrono_now(),
    });

    let local_unpushed: Vec<_> = gc
        .comments
        .into_iter()
        .filter(|c| c.source == "local" && !c.synced)
        .collect();

    let mut github_entries = Vec::new();
    for gh in &gh_comments {
        let file_path = gh.path.clone().unwrap_or_default();
        let stable_line = gh.original_line.or(gh.line);
        let line_end = match (gh.start_line, gh.line) {
            (Some(start), Some(end)) if end > start => Some(end),
            _ => None,
        };
        let resolved_line: Option<usize> = if let (Some(diff_hunk), Some(f)) = (
            &gh.diff_hunk,
            ctx.files.iter().find(|f| f.path == file_path),
        ) {
            find_local_line_for_diff_hunk(diff_hunk, f)
                .map(|(_, ln)| ln)
                .or(stable_line)
        } else {
            stable_line
        };

        let (
            hunk_index,
            anchor_line_content,
            anchor_ctx_before,
            anchor_ctx_after,
            anchor_old_line,
            anchor_hunk_header,
        ) = resolve_anchor(
            resolved_line,
            &file_path,
            &ctx.files,
            gh.diff_hunk.as_deref(),
        );

        let in_reply_to = gh.in_reply_to_id.map(|pid| format!("gh-{}", pid));
        let state = thread_state.get(&gh.id).copied().unwrap_or_default();
        let outdated = merged_outdated_state(state, gh.outdated);
        github_entries.push(ai::GitHubReviewComment {
            id: format!("gh-{}", gh.id),
            timestamp: gh.created_at.clone(),
            file: file_path,
            hunk_index,
            line_start: gh.start_line.or(resolved_line),
            line_end,
            line_content: anchor_line_content,
            comment: gh.body.clone(),
            in_reply_to,
            resolved: state.resolved,
            source: "github".to_string(),
            github_id: Some(gh.id),
            author: gh.user.login.clone(),
            synced: true,
            outdated,
            stale: outdated,
            context_before: anchor_ctx_before,
            context_after: anchor_ctx_after,
            old_line_start: anchor_old_line,
            hunk_header: anchor_hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: ctx.anchor_hash.clone(),
            finding_ref: None,
            side: gh.side.clone().unwrap_or_else(|| "RIGHT".to_string()),
        });
    }

    let github_count = github_entries.len();
    let local_count = local_unpushed.len();
    gc.comments = local_unpushed;
    gc.comments.extend(github_entries);

    // Write to disk (atomic rename, outside app lock)
    if let Some(dir) = std::path::Path::new(&ctx.comments_path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(&gc)?;
    let tmp_path = format!("{}.tmp", ctx.comments_path);
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &ctx.comments_path)?;

    // Refresh PR overview (still outside app lock)
    let pr_data = if ctx.is_remote {
        github::gh_pr_overview_remote(
            &ctx.owner,
            &ctx.repo_name,
            ctx.pr_number_for_overview.unwrap_or(ctx.pr_number),
        )
    } else {
        github::gh_pr_overview(&ctx.repo_root, ctx.pr_number_for_overview)
    };

    Ok(CommentSyncResult {
        gc,
        pr_data,
        github_count,
        local_count,
        is_remote: ctx.is_remote,
        comments_path: ctx.comments_path.clone(),
        tab_key: (
            ctx.repo_root.clone(),
            ctx.pr_number_for_overview,
            ctx.is_remote,
        ),
    })
}

/// Resolve anchor location for a GitHub comment against our local diff files.
pub fn resolve_anchor(
    resolved_line: Option<usize>,
    file_path: &str,
    files: &[git::DiffFile],
    diff_hunk: Option<&str>,
) -> (
    Option<usize>,
    String,
    Vec<String>,
    Vec<String>,
    Option<usize>,
    String,
) {
    if let Some(line) = resolved_line {
        if let Some(f) = files.iter().find(|f| f.path == file_path) {
            if let Some((i, hunk)) = f
                .hunks
                .iter()
                .enumerate()
                .find(|(_, h)| line >= h.new_start && line < h.new_start + h.new_count)
            {
                let target_idx = hunk.lines.iter().position(|l| l.new_num == Some(line));
                let (lc, old_ln, ctx_before, ctx_after) = if let Some(idx) = target_idx {
                    let start = idx.saturating_sub(3);
                    let end = (idx + 4).min(hunk.lines.len());
                    (
                        hunk.lines[idx].content.clone(),
                        hunk.lines[idx].old_num,
                        hunk.lines[start..idx]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect(),
                        hunk.lines[(idx + 1)..end]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect(),
                    )
                } else if let Some(dh) = diff_hunk {
                    let (fallback_lc, fallback_ctx) = extract_anchor_from_diff_hunk(dh);
                    (fallback_lc, None, fallback_ctx, Vec::new())
                } else {
                    let nearest = hunk
                        .lines
                        .iter()
                        .filter_map(|l| l.new_num.map(|n| (n, l)))
                        .min_by_key(|(n, _)| (*n as isize - line as isize).unsigned_abs());
                    let (lc, old_ln) = nearest
                        .map(|(_, l)| (l.content.clone(), l.old_num))
                        .unwrap_or_default();
                    (lc, old_ln, Vec::new(), Vec::new())
                };
                return (
                    Some(i),
                    lc,
                    ctx_before,
                    ctx_after,
                    old_ln,
                    hunk.header.clone(),
                );
            }
        }
    }
    (
        None,
        String::new(),
        Vec::new(),
        Vec::new(),
        None,
        String::new(),
    )
}

/// GitHub's diff_hunk ends at the commented line. The last non-deleted line is the target;
/// the preceding non-deleted lines are context. Used as a fallback when the local DiffLine
/// lookup fails (e.g. because the PR base has drifted from our local base).
///
/// Returns `(line_content, context_before)`.
fn extract_anchor_from_diff_hunk(diff_hunk: &str) -> (String, Vec<String>) {
    let new_side: Vec<&str> = diff_hunk
        .lines()
        .skip(1) // skip @@ header
        .filter(|l| !l.starts_with('-'))
        .map(|l| {
            if l.starts_with('+') || l.starts_with(' ') {
                &l[1..]
            } else {
                l
            }
        })
        .collect();
    let line_content = new_side.last().copied().unwrap_or("").to_string();
    let ctx_start = new_side.len().saturating_sub(4);
    let context_before: Vec<String> = if new_side.len() > 1 {
        new_side[ctx_start..new_side.len() - 1]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    (line_content, context_before)
}

/// Given a GitHub `diff_hunk` string, find the matching local line number in a file's diff.
///
/// GitHub's `line` field uses line numbers from the PR diff (against the PR base commit),
/// which may differ from our local diff when `main` has advanced since the PR was filed.
/// `diff_hunk` contains the actual diff text, so we can use content-based matching instead.
///
/// Returns `(hunk_index, local_line_start)` if a match is found.
pub fn find_local_line_for_diff_hunk(
    diff_hunk: &str,
    file: &git::DiffFile,
) -> Option<(usize, usize)> {
    // Parse diff_hunk lines: first line is the @@ header, rest are +/-/space content lines.
    let hunk_lines: Vec<&str> = diff_hunk.lines().collect();
    let content_lines: Vec<&str> = hunk_lines.iter().skip(1).copied().collect();
    if content_lines.is_empty() {
        return None;
    }

    // Strip the +/-/space prefix to get raw content (matching DiffLine.content which is pre-stripped).
    let stripped: Vec<&str> = content_lines
        .iter()
        .map(|l| {
            if l.starts_with('+') || l.starts_with('-') || l.starts_with(' ') {
                &l[1..]
            } else {
                l
            }
        })
        .collect();

    // Use the last N lines as a sliding-window fingerprint.
    // Skip deleted lines in the window — they won't appear on the new side of the diff.
    let new_side_stripped: Vec<&str> = content_lines
        .iter()
        .zip(stripped.iter())
        .filter(|(raw, _)| !raw.starts_with('-'))
        .map(|(_, s)| *s)
        .collect();

    if new_side_stripped.is_empty() {
        return None;
    }

    // Use a window of up to 4 lines ending at the target line (last line in the hunk).
    let window_size = new_side_stripped.len().min(4);
    let window: Vec<&str> = new_side_stripped[new_side_stripped.len() - window_size..].to_vec();

    // Slide the window across each hunk in our local diff to find a content match.
    // Require a unique match — if the window appears more than once, fall back to gh.line
    // to avoid silently anchoring to the wrong location in repetitive code.
    let mut unique_match: Option<(usize, usize)> = None;
    for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
        let new_side_lines: Vec<(&str, Option<usize>)> = hunk
            .lines
            .iter()
            .filter(|l| !matches!(l.line_type, git::LineType::Delete))
            .map(|l| (l.content.as_str(), l.new_num))
            .collect();

        if new_side_lines.len() < window_size {
            continue;
        }

        for i in 0..=(new_side_lines.len() - window_size) {
            let candidate: Vec<&str> = new_side_lines[i..i + window_size]
                .iter()
                .map(|(c, _)| *c)
                .collect();
            if candidate == window {
                let (_, local_new_num) = new_side_lines[i + window_size - 1];
                if let Some(line_num) = local_new_num {
                    if unique_match.is_some() {
                        // Ambiguous — two locations match the window; refuse to guess.
                        return None;
                    }
                    unique_match = Some((hunk_idx, line_num));
                }
            }
        }
    }
    if let Some(m) = unique_match {
        return Some(m);
    }

    None
}

/// Resolve (owner, repo, pr_number) for a local-mode tab, preferring an
/// explicit PR number over branch detection.
pub fn local_pr_target(
    repo_root: &str,
    explicit_pr_number: Option<u64>,
) -> Result<(String, String, u64)> {
    if let Some(n) = explicit_pr_number {
        let (owner, repo_name) = github::get_repo_info(repo_root)?;
        return Ok((owner, repo_name, n));
    }
    github::get_pr_info(repo_root)
}

// ── Remote diff refresh ───────────────────────────────────────────────────────

/// Inputs for one remote-PR diff refresh cycle. Built while holding the App
/// lock (or, headless, from server session state).
#[derive(Debug, Clone)]
pub struct RemoteDiffContext {
    pub owner: String,
    pub repo: String,
    pub pr_number: u64,
    /// Used by `apply_remote_diff_result` to find the right tab if the user
    /// switches or closes tabs during the network fetch.
    pub repo_root: String,
    /// What `last_diff_head_oid` was on the tab when the snapshot was taken.
    pub last_head_oid: Option<String>,
    /// Latest head_oid available out-of-band (typically the PR cache). When
    /// this equals `last_head_oid` the loop short-circuits.
    pub expected_head_oid: Option<String>,
}

/// Output of one refresh cycle. Applied via `apply_remote_diff_result`.
#[derive(Debug, Clone)]
pub struct RemoteDiffResult {
    pub raw_diff: String,
    pub files: Vec<git::DiffFile>,
    pub branch_diff_hash: String,
    pub diff_hash: String,
    pub head_oid: Option<String>,
    /// (repo_root, pr_number, is_remote) — used to find the right tab on apply.
    pub tab_key: (String, Option<u64>, bool),
}

/// Run the network fetch + parse + hash for a remote-PR diff refresh.
/// Returns `Ok(None)` when the expected head_oid matches the last fetched
/// one (no work needed).
pub fn fetch_remote_diff_data(ctx: &RemoteDiffContext) -> Result<Option<RemoteDiffResult>> {
    if let (Some(expected), Some(last)) = (
        ctx.expected_head_oid.as_deref(),
        ctx.last_head_oid.as_deref(),
    ) {
        if expected == last {
            return Ok(None);
        }
    }

    let raw = github::gh_pr_diff_remote(&ctx.owner, &ctx.repo, ctx.pr_number)?;
    let files = git::parse_diff(&raw);
    let branch_diff_hash = crate::ai::compute_diff_hash(&raw);
    let diff_hash = format!("{:016x}", crate::ai::compute_diff_hash_fast(&raw));

    Ok(Some(RemoteDiffResult {
        raw_diff: raw,
        files,
        branch_diff_hash,
        diff_hash,
        head_oid: ctx.expected_head_oid.clone(),
        tab_key: (ctx.repo_root.clone(), Some(ctx.pr_number), true),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_outdated_state_preserves_graphql_thread_outdated() {
        let state = ReviewThreadState {
            resolved: false,
            outdated: true,
        };

        assert!(merged_outdated_state(state, false));
    }

    #[test]
    fn merged_outdated_state_preserves_rest_comment_outdated() {
        let state = ReviewThreadState {
            resolved: false,
            outdated: false,
        };

        assert!(merged_outdated_state(state, true));
    }

    #[test]
    fn merged_outdated_state_false_when_neither_outdated() {
        let state = ReviewThreadState {
            resolved: true,
            outdated: false,
        };

        assert!(!merged_outdated_state(state, false));
    }

    // ── format_iso8601 ────────────────────────────────────────────────────────

    #[test]
    fn format_iso8601_epoch() {
        assert_eq!(format_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn format_iso8601_end_of_first_day() {
        assert_eq!(format_iso8601(86_399), "1970-01-01T23:59:59Z");
    }

    #[test]
    fn format_iso8601_rolls_into_second_day() {
        assert_eq!(format_iso8601(86_400), "1970-01-02T00:00:00Z");
    }

    #[test]
    fn format_iso8601_year_boundary_non_leap() {
        // 1970 has 365 days (not a leap year), so 365 days lands on 1971-01-01.
        assert_eq!(format_iso8601(31_536_000), "1971-01-01T00:00:00Z");
    }

    #[test]
    fn format_iso8601_leap_day_1972() {
        // 1972 is a leap year — exercises the Feb 29 branch.
        assert_eq!(format_iso8601(68_169_600), "1972-02-29T00:00:00Z");
        assert_eq!(format_iso8601(68_256_000), "1972-03-01T00:00:00Z");
    }

    #[test]
    fn format_iso8601_modern_timestamp() {
        assert_eq!(format_iso8601(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn format_iso8601_leap_day_2024() {
        // 2024 is divisible by 4 and not by 100 — a leap year.
        assert_eq!(format_iso8601(1_709_251_199), "2024-02-29T23:59:59Z");
        assert_eq!(format_iso8601(1_709_251_200), "2024-03-01T00:00:00Z");
    }

    #[test]
    fn chrono_now_is_well_formed() {
        let now = chrono_now();
        // YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(now.len(), 20, "got {now}");
        assert!(now.ends_with('Z'));
        let bytes = now.as_bytes();
        assert_eq!(bytes[4], b'-');
        assert_eq!(bytes[7], b'-');
        assert_eq!(bytes[10], b'T');
        assert_eq!(bytes[13], b':');
        assert_eq!(bytes[16], b':');
        // Year is at least the project's lifetime — sanity that the math isn't wildly off.
        let year: i64 = now[0..4].parse().unwrap();
        assert!(year >= 2024, "got year {year}");
    }

    // ── extract_anchor_from_diff_hunk ─────────────────────────────────────────

    #[test]
    fn extract_anchor_single_line_has_no_context() {
        let hunk = "@@ -1,1 +1,1 @@\n only";
        let (content, before) = extract_anchor_from_diff_hunk(hunk);
        assert_eq!(content, "only");
        assert!(before.is_empty());
    }

    #[test]
    fn extract_anchor_filters_deleted_lines() {
        let hunk = "@@ -1,4 +1,3 @@\n keep1\n-removed\n keep2\n+added3";
        let (content, before) = extract_anchor_from_diff_hunk(hunk);
        // Last new-side line is the target; deleted lines never appear on the new side.
        assert_eq!(content, "added3");
        assert_eq!(before, vec!["keep1".to_string(), "keep2".to_string()]);
    }

    #[test]
    fn extract_anchor_caps_context_at_three_lines() {
        let hunk = "@@ -1,5 +1,5 @@\n a\n b\n c\n d\n e";
        let (content, before) = extract_anchor_from_diff_hunk(hunk);
        assert_eq!(content, "e");
        // Only the three lines immediately before the target are kept.
        assert_eq!(
            before,
            vec!["b".to_string(), "c".to_string(), "d".to_string()]
        );
    }

    // ── find_local_line_for_diff_hunk ─────────────────────────────────────────

    fn parse_one(raw: &str) -> git::DiffFile {
        let mut files = git::parse_diff(raw);
        assert_eq!(files.len(), 1, "fixture should parse to exactly one file");
        files.remove(0)
    }

    #[test]
    fn find_local_line_unique_match() {
        let file = parse_one(
            "diff --git a/src/lib.rs b/src/lib.rs\n\
             --- a/src/lib.rs\n\
             +++ b/src/lib.rs\n\
             @@ -1,4 +1,5 @@\n\
             \x20alpha\n\
             \x20beta\n\
             +gamma\n\
             \x20delta\n\
             \x20epsilon\n",
        );
        let diff_hunk = "@@ -1,2 +1,3 @@\n alpha\n beta\n+gamma";
        // gamma is the new-side line 3 in our local diff.
        assert_eq!(
            find_local_line_for_diff_hunk(diff_hunk, &file),
            Some((0, 3))
        );
    }

    #[test]
    fn find_local_line_header_only_hunk_is_none() {
        let file =
            parse_one("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,1 @@\n+x\n");
        assert_eq!(
            find_local_line_for_diff_hunk("@@ -1,1 +1,1 @@", &file),
            None
        );
    }

    #[test]
    fn find_local_line_all_deleted_window_is_none() {
        let file =
            parse_one("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,1 @@\n+x\n");
        // diff_hunk has only deleted lines → empty new side → cannot anchor.
        let diff_hunk = "@@ -1,2 +0,0 @@\n-gone1\n-gone2";
        assert_eq!(find_local_line_for_diff_hunk(diff_hunk, &file), None);
    }

    #[test]
    fn find_local_line_ambiguous_match_is_none() {
        let file = parse_one(
            "diff --git a/a.rs b/a.rs\n\
             --- a/a.rs\n\
             +++ b/a.rs\n\
             @@ -1,3 +1,3 @@\n\
             \x20repeat\n\
             \x20middle\n\
             \x20repeat\n",
        );
        // Single-line window "repeat" appears twice → refuse to guess.
        let diff_hunk = "@@ -1,1 +1,1 @@\n repeat";
        assert_eq!(find_local_line_for_diff_hunk(diff_hunk, &file), None);
    }

    #[test]
    fn find_local_line_skips_deleted_lines_on_both_sides() {
        let file = parse_one(
            "diff --git a/a.rs b/a.rs\n\
             --- a/a.rs\n\
             +++ b/a.rs\n\
             @@ -1,4 +1,4 @@\n\
             \x20keep1\n\
             -oldline\n\
             +newline\n\
             \x20keep2\n",
        );
        // The window [keep1, newline, keep2] matches the new side, ignoring deletions.
        let diff_hunk = "@@ -1,4 +1,3 @@\n keep1\n-removed\n+newline\n keep2";
        assert_eq!(
            find_local_line_for_diff_hunk(diff_hunk, &file),
            Some((0, 3))
        );
    }

    #[test]
    fn find_local_line_reports_correct_hunk_index() {
        let file = parse_one(
            "diff --git a/a.rs b/a.rs\n\
             --- a/a.rs\n\
             +++ b/a.rs\n\
             @@ -1,2 +1,2 @@\n\
             \x20a1\n\
             \x20a2\n\
             @@ -10,2 +10,3 @@\n\
             \x20b1\n\
             +b2\n\
             \x20b3\n",
        );
        let diff_hunk = "@@ -10,1 +10,2 @@\n b1\n+b2";
        // Match is in the second hunk (index 1), new line 11.
        assert_eq!(
            find_local_line_for_diff_hunk(diff_hunk, &file),
            Some((1, 11))
        );
    }

    // ── resolve_anchor ────────────────────────────────────────────────────────

    fn empty_anchor() -> (
        Option<usize>,
        String,
        Vec<String>,
        Vec<String>,
        Option<usize>,
        String,
    ) {
        (
            None,
            String::new(),
            Vec::new(),
            Vec::new(),
            None,
            String::new(),
        )
    }

    #[test]
    fn resolve_anchor_none_line_returns_empty() {
        let files: Vec<git::DiffFile> = Vec::new();
        assert_eq!(resolve_anchor(None, "a.rs", &files, None), empty_anchor());
    }

    #[test]
    fn resolve_anchor_missing_file_returns_empty() {
        let file =
            parse_one("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,1 @@\n+x\n");
        let files = vec![file];
        assert_eq!(
            resolve_anchor(Some(1), "other.rs", &files, None),
            empty_anchor()
        );
    }

    #[test]
    fn resolve_anchor_line_outside_hunks_returns_empty() {
        let file = parse_one(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,3 @@\n+x\n y\n z\n",
        );
        let files = vec![file];
        assert_eq!(
            resolve_anchor(Some(100), "a.rs", &files, None),
            empty_anchor()
        );
    }

    #[test]
    fn resolve_anchor_exact_match_collects_context() {
        let file = parse_one(
            "diff --git a/a.rs b/a.rs\n\
             --- a/a.rs\n\
             +++ b/a.rs\n\
             @@ -1,5 +1,6 @@\n\
             \x20ctxA\n\
             \x20ctxB\n\
             +added\n\
             \x20ctxC\n\
             \x20ctxD\n\
             \x20ctxE\n",
        );
        let files = vec![file];
        // ctxC is new line 4; old line 3.
        let (hunk_idx, lc, before, after, old_ln, header) =
            resolve_anchor(Some(4), "a.rs", &files, None);
        assert_eq!(hunk_idx, Some(0));
        assert_eq!(lc, "ctxC");
        assert_eq!(
            before,
            vec!["ctxA".to_string(), "ctxB".to_string(), "added".to_string()]
        );
        assert_eq!(after, vec!["ctxD".to_string(), "ctxE".to_string()]);
        assert_eq!(old_ln, Some(3));
        assert_eq!(header, "@@ -1,5 +1,6 @@");
    }

    /// Build a single-hunk file whose middle context lines are folded away
    /// (new line numbers 12–16 have no `DiffLine`), so a comment anchored there
    /// cannot find an exact line and must fall back.
    fn folded_file() -> Vec<git::DiffFile> {
        let mut raw = String::from(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,25 +1,26 @@\n+line1\n",
        );
        for n in 2..=26 {
            raw.push_str(&format!(" line{n}\n"));
        }
        let files = git::parse_diff(&raw);
        // Sanity: the 25-line context run should have folded (no new line 14).
        let has_line_14 = files[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|l| l.new_num == Some(14));
        assert!(!has_line_14, "fixture must fold out the middle context run");
        files
    }

    #[test]
    fn resolve_anchor_falls_back_to_nearest_line_without_diff_hunk() {
        let files = folded_file();
        // Line 13 is inside the folded region; nearest present new line is 11 ("line11").
        let (hunk_idx, lc, before, after, old_ln, header) =
            resolve_anchor(Some(13), "f.txt", &files, None);
        assert_eq!(hunk_idx, Some(0));
        assert_eq!(lc, "line11");
        assert!(before.is_empty());
        assert!(after.is_empty());
        assert_eq!(old_ln, Some(10));
        assert_eq!(header, "@@ -1,25 +1,26 @@");
    }

    #[test]
    fn resolve_anchor_falls_back_to_diff_hunk_when_provided() {
        let files = folded_file();
        let diff_hunk = "@@ -1,3 +1,3 @@\n foo\n bar\n baz";
        let (hunk_idx, lc, before, after, old_ln, header) =
            resolve_anchor(Some(13), "f.txt", &files, Some(diff_hunk));
        assert_eq!(hunk_idx, Some(0));
        // Content + context come from the supplied diff_hunk, not the local lines.
        assert_eq!(lc, "baz");
        assert_eq!(before, vec!["foo".to_string(), "bar".to_string()]);
        assert!(after.is_empty());
        assert_eq!(old_ln, None);
        // Header is still the local hunk's header.
        assert_eq!(header, "@@ -1,25 +1,26 @@");
    }
}
