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
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

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

// ── Comment push cores ────────────────────────────────────────────────────────
//
// Pure push logic (no `App` dependency): everything needed comes from
// `CommentPushTarget`, captured under a brief App lock by the callers. The
// `gh` network calls and the comments-JSON rewrite run without holding any
// lock; callers re-lock briefly afterwards to reload tab state.

/// Identity + file path for a comment push, captured from the active tab.
#[derive(Debug, Clone)]
pub struct CommentPushTarget {
    pub owner: String,
    pub repo_name: String,
    pub pr_number: u64,
    pub is_remote: bool,
    pub repo_root: String,
    pub comments_path: String,
}

/// Result of [`push_all_comments_data`].
#[derive(Debug, Clone, Copy)]
pub struct PushAllOutcome {
    pub pushed: u32,
    pub failed: u32,
}

fn read_comments_file(comments_path: &str) -> Option<ai::ErGitHubComments> {
    let content = std::fs::read_to_string(comments_path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_comments_file(comments_path: &str, gc: &ai::ErGitHubComments) -> Result<()> {
    let json = serde_json::to_string_pretty(gc)?;
    let tmp_path = format!("{}.tmp", comments_path);
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, comments_path)?;
    Ok(())
}

/// Push one parent comment (general or line-anchored) to GitHub.
fn push_parent_comment(
    target: &CommentPushTarget,
    comment: &ai::GitHubReviewComment,
) -> Result<u64> {
    if comment.file.is_empty() {
        // General comments (empty file) route to the issues API.
        return if target.is_remote {
            github::gh_pr_general_comment_remote(
                &target.owner,
                &target.repo_name,
                target.pr_number,
                &comment.comment,
            )
        } else {
            github::gh_pr_general_comment(
                &target.owner,
                &target.repo_name,
                target.pr_number,
                &comment.comment,
                &target.repo_root,
            )
        };
    }
    // Hunk-level comments have no line_start; the line-level push API requires
    // a line, so they get anchored to line 1 on GitHub.
    let start = comment.line_start.unwrap_or(1);
    let end = comment.line_end.unwrap_or(start);
    let side = comment.side.as_str();
    if target.is_remote {
        github::gh_pr_push_comment_remote(
            &target.owner,
            &target.repo_name,
            target.pr_number,
            &comment.file,
            start,
            Some(end),
            &comment.comment,
            side,
        )
    } else {
        github::gh_pr_push_comment(
            &target.owner,
            &target.repo_name,
            target.pr_number,
            &comment.file,
            start,
            Some(end),
            &comment.comment,
            side,
            &target.repo_root,
        )
    }
}

fn push_reply_comment(target: &CommentPushTarget, parent_gh_id: u64, body: &str) -> Result<u64> {
    if target.is_remote {
        github::gh_pr_reply_comment_remote(
            &target.owner,
            &target.repo_name,
            target.pr_number,
            parent_gh_id,
            body,
        )
    } else {
        github::gh_pr_reply_comment(
            &target.owner,
            &target.repo_name,
            target.pr_number,
            parent_gh_id,
            body,
            &target.repo_root,
        )
    }
}

/// Push all unpushed local comments (parents first, then replies) to GitHub.
/// Reads and rewrites the comments JSON file; never touches App state.
pub fn push_all_comments_data(target: &CommentPushTarget) -> Result<PushAllOutcome> {
    let Some(mut gc) = read_comments_file(&target.comments_path) else {
        return Ok(PushAllOutcome {
            pushed: 0,
            failed: 0,
        });
    };

    let mut pushed = 0u32;
    let mut failed = 0u32;

    // Push parents first
    let comment_ids: Vec<String> = gc
        .comments
        .iter()
        .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.is_none())
        .map(|c| c.id.clone())
        .collect();

    for cid in &comment_ids {
        let Some(comment) = gc.comments.iter().find(|c| c.id == *cid).cloned() else {
            continue;
        };
        match push_parent_comment(target, &comment) {
            Ok(github_id) => {
                if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                    c.github_id = Some(github_id);
                    c.synced = true;
                }
                pushed += 1;
            }
            Err(_) => {
                failed += 1;
            }
        }
    }

    // Then push replies
    let reply_ids: Vec<String> = gc
        .comments
        .iter()
        .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.is_some())
        .map(|c| c.id.clone())
        .collect();

    for cid in &reply_ids {
        let Some(comment) = gc.comments.iter().find(|c| c.id == *cid).cloned() else {
            continue;
        };
        let parent_gh_id = comment
            .in_reply_to
            .as_ref()
            .and_then(|rt| gc.comments.iter().find(|c| c.id == *rt))
            .and_then(|c| c.github_id);
        let Some(parent_gh_id) = parent_gh_id else {
            failed += 1;
            continue;
        };
        match push_reply_comment(target, parent_gh_id, &comment.comment) {
            Ok(github_id) => {
                if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                    c.github_id = Some(github_id);
                    c.synced = true;
                }
                pushed += 1;
            }
            Err(_) => {
                failed += 1;
            }
        }
    }

    write_comments_file(&target.comments_path, &gc)?;
    Ok(PushAllOutcome { pushed, failed })
}

/// Push one local comment thread (root + unsynced replies) to GitHub.
/// Returns the number of replies that failed to push.
pub fn push_comment_thread_data(target: &CommentPushTarget, thread_id: &str) -> Result<u32> {
    let mut gc = match read_comments_file(&target.comments_path) {
        Some(gc) => gc,
        None => anyhow::bail!("No github-comments.json found"),
    };

    let parent_idx = gc
        .comments
        .iter()
        .position(|c| c.id == thread_id)
        .ok_or_else(|| anyhow::anyhow!("Comment not found: {thread_id}"))?;
    let parent = &gc.comments[parent_idx];
    if parent.source != "local" {
        anyhow::bail!("Only local comments can be pushed");
    }
    if parent.synced {
        anyhow::bail!("Comment already pushed");
    }
    if parent.in_reply_to.is_some() {
        anyhow::bail!("Use Push only this on the thread root, not a reply");
    }
    if !parent.file.is_empty() && parent.line_start.is_none() {
        anyhow::bail!("Comment has no line anchor; add it on a diff line before pushing");
    }

    let github_id = push_parent_comment(target, &gc.comments[parent_idx])
        .map_err(|e| anyhow::anyhow!("Failed to push comment: {e}"))?;
    gc.comments[parent_idx].github_id = Some(github_id);
    gc.comments[parent_idx].synced = true;

    let reply_ids: Vec<String> = gc
        .comments
        .iter()
        .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.as_deref() == Some(thread_id))
        .map(|c| c.id.clone())
        .collect();

    let mut reply_failed = 0u32;
    for rid in reply_ids {
        let Some(comment) = gc.comments.iter().find(|c| c.id == rid).cloned() else {
            continue;
        };
        match push_reply_comment(target, github_id, &comment.comment) {
            Ok(reply_gh_id) => {
                if let Some(c) = gc.comments.iter_mut().find(|c| c.id == rid) {
                    c.github_id = Some(reply_gh_id);
                    c.synced = true;
                }
            }
            Err(_) => reply_failed += 1,
        }
    }

    write_comments_file(&target.comments_path, &gc)?;
    Ok(reply_failed)
}

/// Push one unsynced local reply whose parent comment is already on GitHub.
pub fn push_comment_reply_data(target: &CommentPushTarget, reply_id: &str) -> Result<()> {
    if reply_id.starts_with("fr-") {
        anyhow::bail!("Finding validation replies cannot be pushed individually");
    }
    let mut gc = match read_comments_file(&target.comments_path) {
        Some(gc) => gc,
        None => anyhow::bail!("No github-comments.json found"),
    };

    let reply = gc
        .comments
        .iter()
        .find(|c| c.id == reply_id)
        .ok_or_else(|| anyhow::anyhow!("Comment not found: {reply_id}"))?;
    if reply.source != "local" {
        anyhow::bail!("Only local comments can be pushed");
    }
    if reply.synced {
        anyhow::bail!("Reply already pushed");
    }
    let parent_id = reply
        .in_reply_to
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Push only works on replies, not thread roots"))?;
    let parent = gc
        .comments
        .iter()
        .find(|c| c.id == parent_id)
        .ok_or_else(|| anyhow::anyhow!("Parent comment not found"))?;
    if !parent.synced {
        anyhow::bail!("Push the thread root to GitHub first");
    }
    let parent_github_id = parent
        .github_id
        .ok_or_else(|| anyhow::anyhow!("Parent comment has no GitHub id"))?;

    let reply_body = reply.comment.clone();
    let github_id = push_reply_comment(target, parent_github_id, &reply_body)
        .map_err(|e| anyhow::anyhow!("Failed to push reply: {e}"))?;

    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == reply_id) {
        c.github_id = Some(github_id);
        c.synced = true;
    }
    write_comments_file(&target.comments_path, &gc)?;
    Ok(())
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
///
/// Carries the raw diff only — `apply_remote_diff_result` runs it through
/// `TabState::install_raw_diff` so the swap uses the exact same lazy-parse +
/// compaction pipeline as a fresh open. (A pre-parsed `files` field here
/// previously bypassed that pipeline and left stale lazy bookkeeping.)
#[derive(Debug, Clone)]
pub struct RemoteDiffResult {
    pub raw_diff: String,
    pub branch_diff_hash: String,
    pub diff_hash: String,
    pub head_oid: Option<String>,
    /// (repo_root, pr_number, is_remote) — used to find the right tab on apply.
    pub tab_key: (String, Option<u64>, bool),
}

/// Run the network fetch + hash for a remote-PR diff refresh.
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
    let branch_diff_hash = crate::ai::compute_diff_hash(&raw);
    let diff_hash = format!("{:016x}", crate::ai::compute_diff_hash_fast(&raw));

    Ok(Some(RemoteDiffResult {
        raw_diff: raw,
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
}
