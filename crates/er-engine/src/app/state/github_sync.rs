use anyhow::Result;

use crate::ai;
use crate::github;

// The pure sync core (no App dependency) lives in `crate::sync`; re-exported
// here so existing `crate::app::...` paths keep working.
pub use crate::sync::{
    fetch_comment_sync_data, fetch_comment_sync_data_cached, CommentSyncContext, CommentSyncResult,
};
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

        // The hover prefetch warms this bundle (first-paint plan step 3): the
        // two gh calls cost ~2.5–3 s on a cold cache, ~0 ms on a warm one.
        let bundle = if is_remote {
            github::gh_pr_comment_bundle_cached(&owner, &repo_name, pr_number, None)
        } else {
            github::gh_pr_comment_bundle_cached(&owner, &repo_name, pr_number, Some(&repo_root))
        };
        let (gh_comments, thread_state) = match &bundle {
            Ok(b) => (b.comments.clone(), b.threads.clone()),
            Err(e) => {
                self.notify(&format!("GitHub sync error: {}", e));
                return Ok(());
            }
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
                diff_hash,
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
        // Any push invalidates the comment-bundle cache (the next pull must
        // see the pushed comments — review-fix-loop A1).
        crate::github::invalidate_pr_comments_cache();
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
        crate::github::invalidate_pr_comments_cache();
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
        crate::github::invalidate_pr_comments_cache();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TabState;
    use crate::github::gh_support::with_fake_gh;
    use tempfile::TempDir;

    // Every test here stops before a `gh` subprocess can run: the guards and
    // the failure classification are reachable offline, the post-push tail
    // (file rewrite, reply fan-out, reload, notify) is not.

    fn gh_comment(id: &str, file: &str, line_start: Option<usize>) -> ai::GitHubReviewComment {
        ai::GitHubReviewComment {
            id: id.to_string(),
            timestamp: String::new(),
            file: file.to_string(),
            hunk_index: Some(0),
            line_start,
            line_end: None,
            line_content: String::new(),
            comment: format!("comment for {id}"),
            in_reply_to: None,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced: false,
            outdated: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".to_string(),
        }
    }

    fn write_comments(er_root: &std::path::Path, comments: Vec<ai::GitHubReviewComment>) {
        let dir = er_root.join(".er");
        std::fs::create_dir_all(&dir).unwrap();
        let gc = ai::ErGitHubComments {
            version: 1,
            diff_hash: String::new(),
            github: None,
            comments,
        };
        std::fs::write(
            dir.join("github-comments.json"),
            serde_json::to_string_pretty(&gc).unwrap(),
        )
        .unwrap();
    }

    fn read_comments(er_root: &std::path::Path) -> ai::ErGitHubComments {
        let raw =
            std::fs::read_to_string(er_root.join(".er").join("github-comments.json")).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    /// App whose single tab is a remote PR tab for `slug`, with its sidecars in
    /// `er_root/.er`. No `pr_number` on the tab (the push commands take it as a
    /// hint), so `github_comments_dir()` stays in that temp dir instead of
    /// resolving a managed PR bucket.
    fn remote_pr_app(er_root: &std::path::Path, slug: &str) -> App {
        let mut app = App::new_for_test(vec![]);
        let root = er_root.to_string_lossy().into_owned();
        app.tabs[0].repo_root = root.clone();
        app.tabs[0].er_root = crate::ErRoot::RepoLocal(root);
        app.tabs[0].remote_repo = Some(slug.to_string());
        app
    }

    fn thread_push_err(comments: Vec<ai::GitHubReviewComment>, id: &str) -> String {
        let tmp = TempDir::new().unwrap();
        write_comments(tmp.path(), comments);
        let mut app = remote_pr_app(tmp.path(), "o/r");
        app.push_github_comment_thread(id, Some(7))
            .unwrap_err()
            .to_string()
    }

    fn reply_push_err(comments: Vec<ai::GitHubReviewComment>, id: &str) -> String {
        let tmp = TempDir::new().unwrap();
        write_comments(tmp.path(), comments);
        let mut app = remote_pr_app(tmp.path(), "o/r");
        app.push_github_comment_reply(id, Some(7))
            .unwrap_err()
            .to_string()
    }

    fn run_git(root: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git must be available");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repo root that resolves to `o/r` through `get_repo_info`'s process-wide
    /// cache and is then deleted. `local_pr_target` still succeeds (cache hit),
    /// while every `gh` call fails at *spawn* because its `current_dir` is gone —
    /// no network, no GitHub mutation, and the real error path is exercised.
    /// If that cache ever disappears these tests fail loudly with
    /// "No PR found for current branch" rather than silently passing.
    fn warmed_then_deleted_repo_root() -> String {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        run_git(dir.path(), &["init", "-q"]);
        run_git(
            dir.path(),
            &["remote", "add", "origin", "https://github.com/o/r.git"],
        );
        let (owner, repo) = crate::github::get_repo_info(&root).expect("origin resolves");
        assert_eq!((owner.as_str(), repo.as_str()), ("o", "r"));
        drop(dir);
        assert!(
            !std::path::Path::new(&root).exists(),
            "the repo root must really be gone, so no gh subprocess can start"
        );
        root
    }

    fn pr_overview(number: u64) -> github::PrOverviewData {
        github::PrOverviewData {
            number,
            title: "t".to_string(),
            body: String::new(),
            state: "OPEN".to_string(),
            author: "a".to_string(),
            url: String::new(),
            base_branch: "main".to_string(),
            head_branch: "feature".to_string(),
            checks: Vec::new(),
            reviewers: Vec::new(),
        }
    }

    fn sync_result(
        tab_key: (String, Option<u64>, bool),
        is_remote: bool,
        pr_data: Option<github::PrOverviewData>,
    ) -> CommentSyncResult {
        CommentSyncResult {
            gc: ai::ErGitHubComments {
                version: 1,
                diff_hash: String::new(),
                github: None,
                comments: Vec::new(),
            },
            pr_data,
            github_count: 3,
            local_count: 1,
            is_remote,
            comments_path: String::new(),
            tab_key,
        }
    }

    // ── apply_comment_sync_result ─────────────────────────────────────────

    #[test]
    fn apply_comment_sync_result_reloads_the_matching_tab_and_notifies() {
        let tmp = TempDir::new().unwrap();
        write_comments(tmp.path(), vec![gh_comment("c-1", "a.rs", Some(3))]);
        let mut app = App::new_for_test(vec![]);
        let root = tmp.path().to_string_lossy().into_owned();
        app.tabs[0].repo_root = root.clone();
        app.tabs[0].er_root = crate::ErRoot::RepoLocal(root.clone());

        app.apply_comment_sync_result(sync_result((root, None, false), false, None));

        assert_eq!(
            app.watch_message.as_deref(),
            Some("GitHub sync: 3 from GitHub, 1 local kept, PR status refreshed"),
            "counts come from the result, not the reloaded file"
        );
        assert_eq!(
            app.tabs[0]
                .ai
                .github_comments
                .as_ref()
                .map(|gc| gc.comments.len()),
            Some(1),
            "the tab re-read the comments the background sync wrote"
        );
    }

    #[test]
    fn apply_comment_sync_result_ignores_a_result_whose_tab_is_gone() {
        let tmp = TempDir::new().unwrap();
        write_comments(tmp.path(), vec![gh_comment("c-1", "a.rs", Some(3))]);
        let mut app = App::new_for_test(vec![]);
        let root = tmp.path().to_string_lossy().into_owned();
        app.tabs[0].repo_root = root.clone();
        app.tabs[0].er_root = crate::ErRoot::RepoLocal(root);

        // Identity of a tab closed while the network fetch was in flight.
        app.apply_comment_sync_result(sync_result(
            ("/some/other/checkout".to_string(), Some(4), false),
            false,
            Some(pr_overview(4)),
        ));

        assert!(
            app.watch_message.is_none(),
            "no notification for a tab that is gone"
        );
        assert!(
            app.tabs[0].ai.github_comments.is_none(),
            "the surviving tab is not reloaded on its behalf"
        );
        assert!(
            app.tabs[0].pr_data.is_none(),
            "and does not adopt the other tab's PR data"
        );
    }

    #[test]
    fn apply_comment_sync_result_updates_a_background_tab_without_notifying() {
        let tmp = TempDir::new().unwrap();
        write_comments(tmp.path(), vec![gh_comment("c-1", "a.rs", Some(3))]);
        let mut app = App::new_for_test(vec![]);
        app.tabs[0].repo_root = "/active/checkout".to_string();
        let root = tmp.path().to_string_lossy().into_owned();
        let mut background = TabState::new_for_test(vec![]);
        background.repo_root = root.clone();
        background.er_root = crate::ErRoot::RepoLocal(root.clone());
        background.remote_repo = Some("o/r".to_string());
        app.tabs.push(background);
        assert_eq!(app.active_tab, 0, "the synced tab is not the active one");

        app.apply_comment_sync_result(sync_result((root, None, true), true, Some(pr_overview(42))));

        assert_eq!(
            app.tabs[1]
                .ai
                .github_comments
                .as_ref()
                .map(|gc| gc.comments.len()),
            Some(1),
            "the result lands on the tab matching its key"
        );
        assert_eq!(
            app.tabs[1].pr_data.as_ref().map(|d| d.number),
            Some(42),
            "refreshed PR overview is attached to that tab"
        );
        assert!(
            app.tabs[0].ai.github_comments.is_none(),
            "the active tab is left alone"
        );
        assert!(
            app.watch_message.is_none(),
            "a background tab's sync stays silent"
        );
    }

    // ── sync_github_comments ──────────────────────────────────────────────

    #[test]
    fn sync_github_comments_reports_an_invalid_remote_slug() {
        let tmp = TempDir::new().unwrap();
        let mut app = remote_pr_app(tmp.path(), "ownerrepo");
        app.tabs[0].pr_number = Some(1);

        app.sync_github_comments().unwrap();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("Invalid remote repo slug"),
            "a slug without an owner is reported, not returned as an error"
        );
    }

    #[test]
    fn sync_github_comments_reports_a_remote_tab_with_no_pr_number() {
        let tmp = TempDir::new().unwrap();
        let mut app = remote_pr_app(tmp.path(), "owner/repo");

        app.sync_github_comments().unwrap();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("No PR info for remote mode")
        );
    }

    #[test]
    fn sync_github_comments_reports_a_local_tab_with_no_resolvable_pr() {
        // Not a git checkout, so `local_pr_target` cannot resolve owner/repo.
        let tmp = TempDir::new().unwrap();
        let mut app = App::new_for_test(vec![]);
        let root = tmp.path().to_string_lossy().into_owned();
        app.tabs[0].repo_root = root.clone();
        app.tabs[0].er_root = crate::ErRoot::RepoLocal(root);
        app.tabs[0].pr_number = Some(1);

        app.sync_github_comments().unwrap();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("No PR found for current branch")
        );
    }

    // ── push_github_comment_thread ────────────────────────────────────────

    #[test]
    fn push_thread_rejects_a_remote_tab_without_owner_repo_or_pr_number() {
        let tmp = TempDir::new().unwrap();
        let mut app = remote_pr_app(tmp.path(), "ownerrepo");
        let err = app
            .push_github_comment_thread("c-1", Some(7))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Invalid remote repo slug"), "{err}");

        let mut app = remote_pr_app(tmp.path(), "o/r");
        let err = app
            .push_github_comment_thread("c-1", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("No PR info for remote mode"), "{err}");
    }

    #[test]
    fn push_thread_errors_without_a_comments_file() {
        let tmp = TempDir::new().unwrap();
        let mut app = remote_pr_app(tmp.path(), "o/r");
        let err = app
            .push_github_comment_thread("c-1", Some(7))
            .unwrap_err()
            .to_string();
        assert!(err.contains("No github-comments.json found"), "{err}");
    }

    #[test]
    fn push_thread_errors_on_a_malformed_comments_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".er")).unwrap();
        std::fs::write(
            tmp.path().join(".er").join("github-comments.json"),
            "{ not json",
        )
        .unwrap();
        let mut app = remote_pr_app(tmp.path(), "o/r");
        let err = app
            .push_github_comment_thread("c-1", Some(7))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Failed to parse github-comments.json"),
            "{err}"
        );
    }

    #[test]
    fn push_thread_errors_when_the_comment_is_missing() {
        let err = thread_push_err(vec![gh_comment("c-1", "a.rs", Some(3))], "c-missing");
        assert!(err.contains("Comment not found: c-missing"), "{err}");
    }

    #[test]
    fn push_thread_refuses_a_comment_pulled_from_github() {
        let mut c = gh_comment("c-1", "a.rs", Some(3));
        c.source = "github".to_string();
        let err = thread_push_err(vec![c], "c-1");
        assert!(err.contains("Only local comments can be pushed"), "{err}");
    }

    #[test]
    fn push_thread_refuses_an_already_pushed_comment() {
        let mut c = gh_comment("c-1", "a.rs", Some(3));
        c.synced = true;
        let err = thread_push_err(vec![c], "c-1");
        assert!(err.contains("Comment already pushed"), "{err}");
    }

    #[test]
    fn push_thread_refuses_a_reply_as_the_thread_root() {
        let mut reply = gh_comment("c-2", "a.rs", Some(3));
        reply.in_reply_to = Some("c-1".to_string());
        let err = thread_push_err(vec![gh_comment("c-1", "a.rs", Some(3)), reply], "c-2");
        assert!(
            err.contains("Use Push only this on the thread root, not a reply"),
            "{err}"
        );
    }

    #[test]
    fn push_thread_requires_a_line_anchor_on_a_file_comment() {
        // A file-scoped comment with no line anchor cannot become a review
        // comment — this is caught before any GitHub call.
        let tmp = TempDir::new().unwrap();
        write_comments(tmp.path(), vec![gh_comment("c-1", "a.rs", None)]);
        let mut app = remote_pr_app(tmp.path(), "o/r");

        let err = app
            .push_github_comment_thread("c-1", Some(7))
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("Comment has no line anchor"),
            "anchor is required before pushing: {err}"
        );
        assert!(
            !read_comments(tmp.path()).comments[0].synced,
            "the rejected comment stays unpushed on disk"
        );
    }

    #[test]
    fn push_thread_leaves_the_comment_unsynced_when_the_gh_push_fails() {
        let tmp = TempDir::new().unwrap();
        write_comments(tmp.path(), vec![gh_comment("c-1", "a.rs", Some(12))]);
        let mut app = App::new_for_test(vec![]);
        app.tabs[0].repo_root = warmed_then_deleted_repo_root();
        app.tabs[0].er_root = crate::ErRoot::RepoLocal(tmp.path().to_string_lossy().into_owned());

        let err = app
            .push_github_comment_thread("c-1", Some(7))
            .unwrap_err()
            .to_string();

        assert!(err.contains("Failed to push comment"), "{err}");
        let on_disk = read_comments(tmp.path());
        assert!(
            !on_disk.comments[0].synced,
            "a failed push must not mark the comment synced"
        );
        assert!(
            on_disk.comments[0].github_id.is_none(),
            "and must not invent a GitHub id"
        );
    }

    // ── push_github_comment_reply ─────────────────────────────────────────

    #[test]
    fn push_reply_refuses_finding_validation_replies() {
        let tmp = TempDir::new().unwrap();
        let mut app = remote_pr_app(tmp.path(), "o/r");
        let err = app
            .push_github_comment_reply("fr-1", Some(7))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Finding validation replies cannot be pushed individually"),
            "{err}"
        );
    }

    #[test]
    fn push_reply_rejects_a_remote_tab_without_owner_repo_or_pr_number() {
        let tmp = TempDir::new().unwrap();
        let mut app = remote_pr_app(tmp.path(), "ownerrepo");
        let err = app
            .push_github_comment_reply("c-2", Some(7))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Invalid remote repo slug"), "{err}");

        let mut app = remote_pr_app(tmp.path(), "o/r");
        let err = app
            .push_github_comment_reply("c-2", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("No PR info for remote mode"), "{err}");
    }

    #[test]
    fn push_reply_errors_without_a_comments_file() {
        let tmp = TempDir::new().unwrap();
        let mut app = remote_pr_app(tmp.path(), "o/r");
        let err = app
            .push_github_comment_reply("c-2", Some(7))
            .unwrap_err()
            .to_string();
        assert!(err.contains("No github-comments.json found"), "{err}");
    }

    #[test]
    fn push_reply_errors_when_the_reply_is_missing() {
        let err = reply_push_err(vec![gh_comment("c-1", "a.rs", Some(3))], "c-missing");
        assert!(err.contains("Comment not found: c-missing"), "{err}");
    }

    #[test]
    fn push_reply_refuses_a_reply_pulled_from_github() {
        let mut reply = gh_comment("c-2", "a.rs", Some(3));
        reply.source = "github".to_string();
        reply.in_reply_to = Some("c-1".to_string());
        let err = reply_push_err(vec![gh_comment("c-1", "a.rs", Some(3)), reply], "c-2");
        assert!(err.contains("Only local comments can be pushed"), "{err}");
    }

    #[test]
    fn push_reply_refuses_an_already_pushed_reply() {
        let mut reply = gh_comment("c-2", "a.rs", Some(3));
        reply.synced = true;
        reply.in_reply_to = Some("c-1".to_string());
        let err = reply_push_err(vec![gh_comment("c-1", "a.rs", Some(3)), reply], "c-2");
        assert!(err.contains("Reply already pushed"), "{err}");
    }

    #[test]
    fn push_reply_refuses_a_thread_root() {
        let err = reply_push_err(vec![gh_comment("c-1", "a.rs", Some(3))], "c-1");
        assert!(
            err.contains("Push only works on replies, not thread roots"),
            "{err}"
        );
    }

    #[test]
    fn push_reply_errors_when_the_parent_is_missing() {
        let mut reply = gh_comment("c-2", "a.rs", Some(3));
        reply.in_reply_to = Some("c-gone".to_string());
        let err = reply_push_err(vec![reply], "c-2");
        assert!(err.contains("Parent comment not found"), "{err}");
    }

    #[test]
    fn push_reply_requires_the_thread_root_to_be_pushed_first() {
        let mut reply = gh_comment("c-2", "a.rs", Some(3));
        reply.in_reply_to = Some("c-1".to_string());
        let err = reply_push_err(vec![gh_comment("c-1", "a.rs", Some(3)), reply], "c-2");
        assert!(
            err.contains("Push the thread root to GitHub first"),
            "{err}"
        );
    }

    #[test]
    fn push_reply_errors_when_the_pushed_parent_has_no_github_id() {
        let mut parent = gh_comment("c-1", "a.rs", Some(3));
        parent.synced = true; // marked pushed but the id never came back
        let mut reply = gh_comment("c-2", "a.rs", Some(3));
        reply.in_reply_to = Some("c-1".to_string());
        let err = reply_push_err(vec![parent, reply], "c-2");
        assert!(err.contains("Parent comment has no GitHub id"), "{err}");
    }

    #[test]
    fn push_reply_leaves_the_reply_unsynced_when_the_gh_push_fails() {
        let tmp = TempDir::new().unwrap();
        let mut parent = gh_comment("c-1", "a.rs", Some(12));
        parent.synced = true;
        parent.github_id = Some(555);
        let mut reply = gh_comment("c-2", "a.rs", Some(12));
        reply.in_reply_to = Some("c-1".to_string());
        write_comments(tmp.path(), vec![parent, reply]);
        let mut app = App::new_for_test(vec![]);
        app.tabs[0].repo_root = warmed_then_deleted_repo_root();
        app.tabs[0].er_root = crate::ErRoot::RepoLocal(tmp.path().to_string_lossy().into_owned());

        let err = app
            .push_github_comment_reply("c-2", Some(7))
            .unwrap_err()
            .to_string();

        assert!(err.contains("Failed to push reply"), "{err}");
        let on_disk = read_comments(tmp.path());
        assert!(
            !on_disk.comments[1].synced,
            "a failed reply push must not mark the reply synced"
        );
        assert!(
            on_disk.comments[1].github_id.is_none(),
            "and must not invent a GitHub id"
        );
        assert!(
            on_disk.comments[0].synced,
            "the parent's own sync state is untouched"
        );
    }


    // ── gh push via a fake `gh` on PATH (github.rs' shared gh_support) ─────
    #[test]
    fn push_thread_pushes_a_local_file_comment_and_marks_it_synced() {
        let tmp = TempDir::new().unwrap();
        write_comments(tmp.path(), vec![gh_comment("c-1", "a.rs", Some(5))]);
        let mut app = remote_pr_app(tmp.path(), "o/r");
        // gh: `pr view` -> a HEAD oid; `api -X POST` -> the new comment's id.
        let script = r#"#!/bin/sh
case "$1" in
  pr) printf '%s' 'abc123'; exit 0 ;;
  api) printf '%s' '{"id":5}'; exit 0 ;;
esac
printf 'unexpected: %s' "$*" >&2
exit 1
"#;
        with_fake_gh(script, || app.push_github_comment_thread("c-1", Some(7))).unwrap();
        let on_disk = read_comments(tmp.path());
        let c = on_disk.comments.iter().find(|c| c.id == "c-1").unwrap();
        assert!(c.synced, "a successful push marks the comment synced");
        assert_eq!(c.github_id, Some(5));
    }



    #[test]
    fn push_thread_pushes_replies_then_marks_the_whole_thread_synced() {
        let tmp = TempDir::new().unwrap();
        let mut reply = gh_comment("c-2", "a.rs", Some(7));
        reply.in_reply_to = Some("c-1".to_string());
        write_comments(tmp.path(), vec![gh_comment("c-1", "a.rs", Some(5)), reply]);
        let mut app = remote_pr_app(tmp.path(), "o/r");
        let script = r#"#!/bin/sh
case "$1" in
  pr) printf '%s' 'abc123'; exit 0 ;;
  api) printf '%s' '{"id":5}'; exit 0 ;;
esac
printf 'unexpected: %s' "$*" >&2
exit 1
"#;
        with_fake_gh(script, || app.push_github_comment_thread("c-1", Some(7))).unwrap();
        let on_disk = read_comments(tmp.path());
        let parent = on_disk.comments.iter().find(|c| c.id == "c-1").unwrap();
        let reply = on_disk.comments.iter().find(|c| c.id == "c-2").unwrap();
        assert!(parent.synced);
        assert_eq!(parent.github_id, Some(5));
        assert!(reply.synced, "the reply is pushed and marked synced");
        assert_eq!(reply.github_id, Some(5));
    }

}
