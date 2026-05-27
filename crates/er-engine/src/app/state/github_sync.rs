use anyhow::Result;

use crate::ai;
use crate::git;
use crate::github;
use crate::github::PrOverviewData;
use crate::github::ReviewThreadState;

use super::chrono_now;
use super::App;

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

fn merged_outdated_state(thread_state: ReviewThreadState, rest_outdated: bool) -> bool {
    thread_state.outdated || rest_outdated
}

impl App {
    /// Snapshot identity + files from the active tab. Call while holding the lock,
    /// then release it before calling `fetch_comment_sync_data`.
    pub fn snapshot_for_comment_sync(
        &self,
        owner: String,
        repo_name: String,
        pr_number: u64,
    ) -> CommentSyncContext {
        let tab = self.tab();
        CommentSyncContext {
            owner,
            repo_name,
            pr_number,
            is_remote: tab.is_remote(),
            repo_root: tab.repo_root.clone(),
            comments_path: tab.github_comments_path(),
            diff_hash: tab.branch_diff_hash.clone(),
            anchor_hash: tab.diff_hash.clone(),
            files: tab.files.clone(),
            pr_number_for_overview: tab.pr_number,
        }
    }

    /// Apply pre-fetched comment sync results. Finds the correct tab by identity
    /// (safe against the user switching/closing tabs during network I/O).
    pub fn apply_comment_sync_result(&mut self, result: CommentSyncResult) {
        let (target_root, target_pr, target_is_remote) = &result.tab_key;
        let tab_idx = self.tabs.iter().position(|t| {
            &t.repo_root == target_root
                && t.pr_number == *target_pr
                && t.is_remote() == *target_is_remote
        });
        let idx = match tab_idx {
            Some(i) => i,
            None => return, // tab was closed or switched — file was written; next activate picks it up
        };
        if result.is_remote {
            self.tabs[idx].reload_remote_comments();
        } else {
            self.tabs[idx].reload_ai_state();
        }
        if let Some(pr_data) = result.pr_data {
            self.tabs[idx].pr_data = Some(pr_data);
        }
        // Only notify if this is the currently active tab.
        if idx == self.active_tab {
            self.notify(&format!(
                "GitHub sync: {} from GitHub, {} local kept, PR status refreshed",
                result.github_count, result.local_count
            ));
        }
    }
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
    if !ctx.is_remote {
        std::fs::create_dir_all(format!("{}/.er", ctx.repo_root))?;
    } else if let Some(dir) = std::path::Path::new(&ctx.comments_path).parent() {
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
fn resolve_anchor(
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
fn find_local_line_for_diff_hunk(diff_hunk: &str, file: &git::DiffFile) -> Option<(usize, usize)> {
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

fn local_pr_target(
    repo_root: &str,
    explicit_pr_number: Option<u64>,
) -> Result<(String, String, u64)> {
    if let Some(n) = explicit_pr_number {
        let (owner, repo_name) = github::get_repo_info(repo_root)?;
        return Ok((owner, repo_name, n));
    }
    github::get_pr_info(repo_root)
}

impl App {
    /// Sync GitHub PR comments (pull)
    pub fn sync_github_comments(&mut self) -> Result<()> {
        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let explicit_pr_number = tab.pr_number;
        let is_remote = tab.is_remote();
        let remote_repo = tab.remote_repo.clone();

        let (owner, repo_name, pr_number) = if is_remote {
            if let (Some(ref slug), Some(n)) = (&remote_repo, explicit_pr_number) {
                let parts: Vec<&str> = slug.split('/').collect();
                if parts.len() == 2 {
                    (parts[0].to_string(), parts[1].to_string(), n)
                } else {
                    self.notify("Invalid remote repo slug");
                    return Ok(());
                }
            } else {
                self.notify("No PR info for remote mode");
                return Ok(());
            }
        } else {
            let pr_info = local_pr_target(&repo_root, explicit_pr_number);
            let pr_info = match pr_info {
                Ok(info) => info,
                Err(_) => {
                    self.notify("No PR found for current branch");
                    return Ok(());
                }
            };
            pr_info
        };

        let gh_comments = if is_remote {
            match github::gh_pr_comments_remote(&owner, &repo_name, pr_number) {
                Ok(c) => c,
                Err(e) => {
                    self.notify(&format!("GitHub sync error: {}", e));
                    return Ok(());
                }
            }
        } else {
            match github::gh_pr_comments(&owner, &repo_name, pr_number, &repo_root) {
                Ok(c) => c,
                Err(e) => {
                    self.notify(&format!("GitHub sync error: {}", e));
                    return Ok(());
                }
            }
        };

        // Fetch review-thread state that the REST comments endpoint does not expose reliably.
        let thread_state = if is_remote {
            github::gh_pr_review_threads_remote(&owner, &repo_name, pr_number).unwrap_or_default()
        } else {
            github::gh_pr_review_threads(&owner, &repo_name, pr_number, &repo_root)
                .unwrap_or_default()
        };

        // Load existing github-comments.json (uses cache dir in remote mode)
        let comments_dir = self.tab().comments_dir();
        let _ = std::fs::create_dir_all(&comments_dir);
        let comments_path = self.tab().github_comments_path();
        let diff_hash = tab.branch_diff_hash.clone();
        let mut gc: ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
            Ok(content) => {
                serde_json::from_str(&content).unwrap_or_else(|_| ai::ErGitHubComments {
                    version: 1,
                    diff_hash: diff_hash.clone(),
                    github: None,
                    comments: Vec::new(),
                })
            }
            Err(_) => ai::ErGitHubComments {
                version: 1,
                diff_hash: diff_hash.clone(),
                github: None,
                comments: Vec::new(),
            },
        };

        gc.github = Some(ai::GitHubSyncState {
            pr_number: Some(pr_number),
            owner: owner.clone(),
            repo: repo_name.clone(),
            last_synced: chrono_now(),
        });

        // Keep only truly local unpushed comments
        let local_unpushed: Vec<_> = gc
            .comments
            .into_iter()
            .filter(|c| c.source == "local" && !c.synced)
            .collect();

        // Build fresh GitHub entries from API response
        let tab_files = self.tab().files.clone();
        let diff_hash_for_anchor = self.tab().diff_hash.clone();
        let mut github_entries = Vec::new();

        for gh in &gh_comments {
            let file_path = gh.path.clone().unwrap_or_default();

            // Prefer content-based matching via diff_hunk — robust against line-number drift when
            // main has advanced since the PR was filed.
            let stable_line = gh.original_line.or(gh.line);
            let resolved_line: Option<usize> = if let (Some(diff_hunk), Some(f)) = (
                &gh.diff_hunk,
                tab_files.iter().find(|f| f.path == file_path),
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
                &tab_files,
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
                line_start: resolved_line,
                line_end: None,
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
                relocated_at_hash: diff_hash_for_anchor.clone(),
                finding_ref: None,
                side: gh.side.clone().unwrap_or_else(|| "RIGHT".to_string()),
            });
        }

        let github_count = github_entries.len();
        let local_count = local_unpushed.len();
        gc.comments = local_unpushed;
        gc.comments.extend(github_entries);

        if !is_remote {
            std::fs::create_dir_all(format!("{}/.er", repo_root))?;
        }
        let json = serde_json::to_string_pretty(&gc)?;
        let tmp_path = format!("{}.tmp", comments_path);
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &comments_path)?;

        if is_remote {
            self.tab_mut().reload_remote_comments();
        } else {
            self.tab_mut().reload_ai_state();
        }

        // Refresh PR overview data (CI checks + reviewer status)
        let pr_number_for_overview = self.tab().pr_number;
        if is_remote {
            if let Some(pr_data) = github::gh_pr_overview_remote(
                &owner,
                &repo_name,
                pr_number_for_overview.unwrap_or(pr_number),
            ) {
                self.tab_mut().pr_data = Some(pr_data);
            }
        } else if let Some(pr_data) = github::gh_pr_overview(&repo_root, pr_number_for_overview) {
            self.tab_mut().pr_data = Some(pr_data);
        }

        self.notify(&format!(
            "GitHub sync: {} from GitHub, {} local kept, PR status refreshed",
            github_count, local_count
        ));
        Ok(())
    }

    /// Push all unpushed local comments to GitHub
    pub fn push_all_comments_to_github(&mut self) -> Result<()> {
        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let explicit_pr_number = tab.pr_number;
        let is_remote = tab.is_remote();
        let remote_repo = tab.remote_repo.clone();

        let (owner, repo_name, pr_number) = if is_remote {
            if let (Some(ref slug), Some(n)) = (&remote_repo, explicit_pr_number) {
                let parts: Vec<&str> = slug.split('/').collect();
                if parts.len() == 2 {
                    (parts[0].to_string(), parts[1].to_string(), n)
                } else {
                    self.notify("Invalid remote repo slug");
                    return Ok(());
                }
            } else {
                self.notify("No PR info for remote mode");
                return Ok(());
            }
        } else {
            let pr_info = match local_pr_target(&repo_root, explicit_pr_number) {
                Ok(info) => info,
                Err(_) => {
                    self.notify("No PR found for current branch");
                    return Ok(());
                }
            };
            pr_info
        };

        let comments_path = self.tab().github_comments_path();
        let mut gc: ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(gc) => gc,
                Err(_) => return Ok(()),
            },
            Err(_) => return Ok(()),
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
            let comment = gc.comments.iter().find(|c| c.id == *cid).cloned();
            if let Some(comment) = comment {
                // General comments (empty file) route to the issues API
                if comment.file.is_empty() {
                    match if is_remote {
                        github::gh_pr_general_comment_remote(
                            &owner,
                            &repo_name,
                            pr_number,
                            &comment.comment,
                        )
                    } else {
                        github::gh_pr_general_comment(
                            &owner,
                            &repo_name,
                            pr_number,
                            &comment.comment,
                            &repo_root,
                        )
                    } {
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
                    continue;
                }

                let path = &comment.file;
                // TODO(risk:medium): a comment with no line_start (hunk-level comment) falls back
                // to line 1, silently attributing the GitHub comment to the wrong location.
                let start = comment.line_start.unwrap_or(1);
                let end = comment.line_end.unwrap_or(start);
                let side = comment.side.as_str();
                match if is_remote {
                    github::gh_pr_push_comment_remote(
                        &owner,
                        &repo_name,
                        pr_number,
                        path,
                        start,
                        Some(end),
                        &comment.comment,
                        side,
                    )
                } else {
                    github::gh_pr_push_comment(
                        &owner,
                        &repo_name,
                        pr_number,
                        path,
                        start,
                        Some(end),
                        &comment.comment,
                        side,
                        &repo_root,
                    )
                } {
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
        }

        // Then push replies
        let reply_ids: Vec<String> = gc
            .comments
            .iter()
            .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.is_some())
            .map(|c| c.id.clone())
            .collect();

        for cid in &reply_ids {
            let comment = gc.comments.iter().find(|c| c.id == *cid).cloned();
            if let Some(comment) = comment {
                let parent_gh_id = comment
                    .in_reply_to
                    .as_ref()
                    .and_then(|rt| gc.comments.iter().find(|c| c.id == *rt))
                    .and_then(|c| c.github_id);

                if let Some(parent_gh_id) = parent_gh_id {
                    match if is_remote {
                        github::gh_pr_reply_comment_remote(
                            &owner,
                            &repo_name,
                            pr_number,
                            parent_gh_id,
                            &comment.comment,
                        )
                    } else {
                        github::gh_pr_reply_comment(
                            &owner,
                            &repo_name,
                            pr_number,
                            parent_gh_id,
                            &comment.comment,
                            &repo_root,
                        )
                    } {
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
                } else {
                    failed += 1;
                }
            }
        }

        let json = serde_json::to_string_pretty(&gc)?;
        let tmp_path = format!("{}.tmp", comments_path);
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &comments_path)?;
        if is_remote {
            self.tab_mut().reload_remote_comments();
        } else {
            self.tab_mut().reload_ai_state();
        }

        if failed > 0 {
            self.notify(&format!("Pushed {} comments ({} failed)", pushed, failed));
        } else {
            self.notify(&format!("Pushed {} comments", pushed));
        }
        Ok(())
    }

    /// Push one local comment thread (root + unsynced replies) to GitHub.
    pub fn push_github_comment_thread(
        &mut self,
        thread_id: &str,
        pr_number_hint: Option<u64>,
    ) -> Result<()> {
        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let explicit_pr_number = tab.pr_number.or(pr_number_hint);
        let is_remote = tab.is_remote();
        let remote_repo = tab.remote_repo.clone();

        let (owner, repo_name, pr_number) = if is_remote {
            if let (Some(ref slug), Some(n)) = (&remote_repo, explicit_pr_number) {
                let parts: Vec<&str> = slug.split('/').collect();
                if parts.len() == 2 {
                    (parts[0].to_string(), parts[1].to_string(), n)
                } else {
                    anyhow::bail!("Invalid remote repo slug");
                }
            } else {
                anyhow::bail!("No PR info for remote mode");
            }
        } else {
            local_pr_target(&repo_root, explicit_pr_number)
                .map_err(|_| anyhow::anyhow!("No PR found for current branch"))?
        };

        let comments_path = self.tab().github_comments_path();
        let mut gc: ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(gc) => gc,
                Err(e) => anyhow::bail!("Failed to parse github-comments.json: {e}"),
            },
            Err(_) => anyhow::bail!("No github-comments.json found"),
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

        let push_parent = |comment: &ai::GitHubReviewComment| -> Result<u64> {
            if comment.file.is_empty() {
                if is_remote {
                    github::gh_pr_general_comment_remote(
                        &owner,
                        &repo_name,
                        pr_number,
                        &comment.comment,
                    )
                } else {
                    github::gh_pr_general_comment(
                        &owner,
                        &repo_name,
                        pr_number,
                        &comment.comment,
                        &repo_root,
                    )
                }
            } else {
                let start = comment.line_start.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Comment has no line anchor; add it on a diff line before pushing"
                    )
                })?;
                let end = comment.line_end.unwrap_or(start);
                let side = comment.side.as_str();
                if is_remote {
                    github::gh_pr_push_comment_remote(
                        &owner,
                        &repo_name,
                        pr_number,
                        &comment.file,
                        start,
                        Some(end),
                        &comment.comment,
                        side,
                    )
                } else {
                    github::gh_pr_push_comment(
                        &owner,
                        &repo_name,
                        pr_number,
                        &comment.file,
                        start,
                        Some(end),
                        &comment.comment,
                        side,
                        &repo_root,
                    )
                }
            }
        };

        let github_id =
            push_parent(parent).map_err(|e| anyhow::anyhow!("Failed to push comment: {e}"))?;
        gc.comments[parent_idx].github_id = Some(github_id);
        gc.comments[parent_idx].synced = true;

        let reply_ids: Vec<String> = gc
            .comments
            .iter()
            .filter(|c| {
                c.source == "local" && !c.synced && c.in_reply_to.as_deref() == Some(thread_id)
            })
            .map(|c| c.id.clone())
            .collect();

        let mut reply_failed = 0u32;
        for rid in reply_ids {
            let Some(comment) = gc.comments.iter().find(|c| c.id == rid).cloned() else {
                continue;
            };
            match if is_remote {
                github::gh_pr_reply_comment_remote(
                    &owner,
                    &repo_name,
                    pr_number,
                    github_id,
                    &comment.comment,
                )
            } else {
                github::gh_pr_reply_comment(
                    &owner,
                    &repo_name,
                    pr_number,
                    github_id,
                    &comment.comment,
                    &repo_root,
                )
            } {
                Ok(reply_gh_id) => {
                    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == rid) {
                        c.github_id = Some(reply_gh_id);
                        c.synced = true;
                    }
                }
                Err(_) => reply_failed += 1,
            }
        }

        let json = serde_json::to_string_pretty(&gc)?;
        let tmp_path = format!("{}.tmp", comments_path);
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &comments_path)?;
        if is_remote {
            self.tab_mut().reload_remote_comments();
        } else {
            self.tab_mut().reload_ai_state();
        }

        if reply_failed > 0 {
            self.notify(&format!(
                "Comment pushed; {reply_failed} repl{} failed",
                if reply_failed == 1 { "y" } else { "ies" }
            ));
        } else {
            self.notify("Comment pushed to GitHub");
        }
        Ok(())
    }
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
