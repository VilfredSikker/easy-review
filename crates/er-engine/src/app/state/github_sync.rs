use anyhow::Result;

use crate::ai;
use crate::github;

// The pure sync core (no App dependency) lives in `crate::sync`; re-exported
// here so existing `crate::app::...` paths keep working.
pub use crate::sync::{fetch_comment_sync_data, CommentSyncContext, CommentSyncResult};
use crate::sync::{
    find_local_line_for_diff_hunk, local_pr_target, merged_outdated_state, resolve_anchor,
};

use super::chrono_now;
use super::App;

// ── Two-phase background comment sync (App wrappers) ──────────────────────────

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
            match local_pr_target(&repo_root, explicit_pr_number) {
                Ok(info) => info,
                Err(_) => {
                    self.notify("No PR found for current branch");
                    return Ok(());
                }
            }
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

        // Load existing github-comments.json (PR-scoped: shared PR bucket for PR tabs)
        let comments_dir = self.tab().github_comments_dir();
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

        if let Some(dir) = std::path::Path::new(&comments_path).parent() {
            std::fs::create_dir_all(dir)?;
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
            match local_pr_target(&repo_root, explicit_pr_number) {
                Ok(info) => info,
                Err(_) => {
                    self.notify("No PR found for current branch");
                    return Ok(());
                }
            }
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
                // Hunk-level comments have no line_start; the line-level push API requires
                // a line, so they get anchored to line 1 on GitHub.
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

    /// Push one unsynced local reply whose parent comment is already on GitHub.
    pub fn push_github_comment_reply(
        &mut self,
        reply_id: &str,
        pr_number_hint: Option<u64>,
    ) -> Result<()> {
        if reply_id.starts_with("fr-") {
            anyhow::bail!("Finding validation replies cannot be pushed individually");
        }

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
        let github_id = if is_remote {
            github::gh_pr_reply_comment_remote(
                &owner,
                &repo_name,
                pr_number,
                parent_github_id,
                &reply_body,
            )
        } else {
            github::gh_pr_reply_comment(
                &owner,
                &repo_name,
                pr_number,
                parent_github_id,
                &reply_body,
                &repo_root,
            )
        }
        .map_err(|e| anyhow::anyhow!("Failed to push reply: {e}"))?;

        if let Some(c) = gc.comments.iter_mut().find(|c| c.id == reply_id) {
            c.github_id = Some(github_id);
            c.synced = true;
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
        self.notify("Reply pushed to GitHub");
        Ok(())
    }
}
