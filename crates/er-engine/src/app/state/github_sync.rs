use anyhow::Result;

use crate::ai;

// The pure sync core (no App dependency) lives in `crate::sync`; re-exported
// here so existing `crate::app::...` paths keep working.
pub use crate::sync::{
    fetch_comment_sync_data, fetch_comment_sync_data_cached, CommentSyncContext, CommentSyncResult,
};

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
    ) -> Result<CommentSyncContext> {
        let tab = self.tab();
        let comments_dir = tab.github_comments_dir();
        let store = crate::store_registry::store();
        let scope = er_store::Scope::dir(comments_dir.as_str())?;
        let comments_path =
            crate::ai::annotation_store::path_of::<crate::ai::ErGithubComments>(&store, &scope)?
                .to_string_lossy()
                .into_owned();
        Ok(CommentSyncContext {
            owner,
            repo_name,
            pr_number,
            is_remote: tab.is_remote(),
            repo_root: tab.repo_root.clone(),
            comments_path,
            diff_hash: tab.branch_diff_hash.clone(),
            anchor_hash: tab.diff_hash.clone(),
            files: tab.files.clone(),
            pr_number_for_overview: tab.pr_number,
            github: tab.ports.github.clone(),
        })
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
        } else if let Some(number) = explicit_pr_number {
            match self
                .ports
                .github
                .get_repo_info(&*self.ports.git, &repo_root)
            {
                Ok((owner, repo)) => (owner, repo, number),
                Err(e) => {
                    self.notify(&format!(
                        "Could not read the repository for this pull request: {e:#}"
                    ));
                    return Ok(());
                }
            }
        } else {
            let lookup = match self
                .ports
                .github
                .lookup_pr_for_current_branch(&*self.ports.git, &repo_root)
            {
                Ok(lookup) => lookup,
                Err(e) => {
                    self.notify(&format!("Could not read this branch's PR: {e}"));
                    return Ok(());
                }
            };
            match lookup {
                crate::github::PrLookup::Present {
                    owner,
                    repo,
                    number,
                } => {
                    self.tab_mut().pr_absent = false;
                    (owner, repo, number)
                }
                crate::github::PrLookup::Absent => {
                    self.tab_mut().pr_absent = true;
                    self.notify("No PR found for current branch");
                    return Ok(());
                }
                // The handle reports a failed lookup as `Err`; the variant is
                // here for the wrapper's old shape and never produced by it.
                crate::github::PrLookup::Unknown(e) => {
                    self.notify(&format!("Could not read this branch's PR: {e}"));
                    return Ok(());
                }
            }
        };

        // The TUI's manual sync always hits GitHub: a hover-prefetch bundle can
        // be stale by the time the user asks for a pull.
        let repo_root_arg = if is_remote {
            None
        } else {
            Some(repo_root.as_str())
        };
        let bundle =
            self.ports
                .github
                .fetch_pr_comment_bundle(&owner, &repo_name, pr_number, repo_root_arg);
        let (gh_comments, thread_state) = match bundle {
            Ok(b) => (b.comments, b.threads),
            Err(e) => {
                self.notify(&format!("GitHub sync error: {}", e));
                return Ok(());
            }
        };

        let ctx = self.snapshot_for_comment_sync(owner.clone(), repo_name.clone(), pr_number)?;
        let comments_path = std::path::Path::new(&ctx.comments_path);
        let comments_dir = comments_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid comments path: {}", ctx.comments_path))?;
        let store = crate::store_registry::store();
        let scope = er_store::Scope::dir(comments_dir)?;
        let updated = ai::annotation_store::update_github_comments(&store, &scope, |current| {
            Ok::<_, std::convert::Infallible>(crate::sync::rebuild_comment_store(
                &ctx,
                current.unwrap_or_else(|| ai::ErGithubComments {
                    version: 1,
                    diff_hash: ctx.diff_hash.clone(),
                    github: None,
                    comments: Vec::new(),
                }),
                &gh_comments,
                &thread_state,
            ))
        })
        .map_err(|error| anyhow::anyhow!("{error}"))?;
        let gc = updated.value;
        let github_count = gc.comments.iter().filter(|c| c.source == "github").count();
        let local_count = gc
            .comments
            .iter()
            .filter(|c| crate::sync::keep_on_pull(c))
            .count();

        if is_remote {
            self.tab_mut().reload_remote_comments();
        } else {
            self.tab_mut().reload_ai_state();
        }

        // Refresh PR overview data (CI checks + reviewer status)
        let pr_number_for_overview = self.tab().pr_number;
        if is_remote {
            if let Some(pr_data) = self
                .ports
                .github
                .overview_remote(
                    &owner,
                    &repo_name,
                    pr_number_for_overview.unwrap_or(pr_number),
                )
                .ok()
                .flatten()
            {
                self.tab_mut().pr_data = Some(pr_data);
            }
        } else if let Some(pr_data) = self
            .ports
            .github
            .overview(&repo_root, pr_number_for_overview)
            .ok()
            .flatten()
        {
            self.tab_mut().pr_data = Some(pr_data);
        }

        self.notify(&format!(
            "GitHub sync: {} from GitHub, {} local kept, PR status refreshed",
            github_count, local_count
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use er_github::{FakeGitHub, GhError};
    use std::sync::Arc;

    #[test]
    fn comment_sync_reads_the_bundle_through_the_handle_and_surfaces_the_error() {
        let mut app = App::new_for_test(vec![]);
        app.tab_mut().remote_repo = Some("acme/widgets".into());
        app.tab_mut().pr_number = Some(7);
        let github = Arc::new(FakeGitHub::default().with_fetch_pr_comment_bundle(Err(
            GhError::Forbidden {
                message: "nope".into(),
            },
        )));
        app.ports.github = github.clone();

        app.sync_github_comments().unwrap();

        assert_eq!(
            github.calls(),
            vec![
                "fetch_pr_comment_bundle(owner=\"acme\", repo=\"widgets\", pr=7, \
                 repo_root=None)"
                    .to_string()
            ]
        );
        let notification = app.notification.as_ref().expect("a sync error is shown");
        assert!(
            notification.message.contains("GitHub sync error: nope"),
            "{}",
            notification.message
        );
    }

    #[test]
    fn the_tui_sync_keeps_the_checks_refresh() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::new_for_test(vec![]);
        {
            let tab = app.tab_mut();
            tab.repo_root = tmp.path().to_string_lossy().into_owned();
            tab.er_root = crate::paths::ErRoot::managed(
                tmp.path().join(".er").to_string_lossy().into_owned(),
            );
            tab.pr_number = Some(7);
        }
        let github = Arc::new(
            FakeGitHub::default()
                .with_get_repo_info(Ok(("acme".into(), "widgets".into())))
                .with_fetch_pr_comment_bundle(Ok(er_github::PrCommentBundle {
                    comments: Vec::new(),
                    threads: std::collections::HashMap::new(),
                }))
                .with_overview(Ok(Some(er_github::PrOverviewData {
                    number: 7,
                    title: "T".into(),
                    body: String::new(),
                    state: "OPEN".into(),
                    author: "a".into(),
                    url: "u".into(),
                    base_branch: "main".into(),
                    head_branch: "f".into(),
                    checks: Vec::new(),
                    reviewers: Vec::new(),
                }))),
        );
        app.ports.github = github.clone();

        app.sync_github_comments().unwrap();

        let calls = github.calls();
        assert!(
            calls.iter().any(|call| call.starts_with("overview(")),
            "the manual TUI sync refreshes CI checks: {calls:?}"
        );
        assert!(
            !calls
                .iter()
                .any(|call| call.starts_with("overview_no_checks")),
            "the manual TUI sync must not use the checks-free variant: {calls:?}"
        );
    }
}
