//! Tauri command behind the GitHub card's branch actions (merge, update
//! branch, close/reopen, ready/draft, delete/restore branch). The `gh` calls
//! live in `er_engine::gh_pr_actions`; this module resolves the active tab's
//! PR, runs the action, and refreshes the cached status before returning so
//! the card shows the result rather than the state before the click.

use crate::commands::{
    active_github_key, fetch_and_store_github_status, run_blocking, snap_from, AppState,
};
use crate::snapshot::{AppSnapshot, GithubStatusSnapshot};
use er_engine::gh_pr_actions::{self as actions, MergeMethod, PrAction};
use serde::Deserialize;
use tauri::State;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrActionRequest {
    Merge {
        method: MergeMethod,
        #[serde(default)]
        auto: bool,
        /// The head commit the card showed; GitHub refuses the merge if the
        /// branch moved since.
        expected_head: String,
        #[serde(default)]
        delete_branch: bool,
    },
    DisableAutoMerge,
    UpdateBranch {
        #[serde(default)]
        rebase: bool,
    },
    Close,
    Reopen,
    MarkReady,
    ConvertToDraft,
    DeleteBranch,
    RestoreBranch,
}

/// The PR the card showed when the user clicked. The action runs only if it is
/// still the active tab's PR, so a confirm left open across a tab switch never
/// lands on another PR.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PrTarget {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

impl PrTarget {
    fn matches(&self, owner: &str, repo: &str, number: u64) -> bool {
        self.number == number
            && self.owner.eq_ignore_ascii_case(owner)
            && self.repo.eq_ignore_ascii_case(repo)
    }
}

#[tauri::command]
pub async fn run_github_pr_action(
    pr: PrTarget,
    action: PrActionRequest,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || run_github_pr_action_blocking(&pr, action, &state)).await
}

fn run_github_pr_action_blocking(
    pr: &PrTarget,
    action: PrActionRequest,
    state: &AppState,
) -> Result<AppSnapshot, String> {
    let (owner, repo, number) = {
        let app = state.app.lock().map_err(|e| e.to_string())?;
        active_github_key(&app, state)
            .ok_or_else(|| "No GitHub PR detected for the active tab".to_string())?
    };
    if !pr.matches(&owner, &repo, number) {
        return Err(format!(
            "The active tab now shows {owner}/{repo}#{number}, not {}/{}#{}. Nothing was changed.",
            pr.owner, pr.repo, pr.number
        ));
    }
    let cached = state
        .gh_status_cache
        .lock()
        .ok()
        .and_then(|g| g.get(&(owner.clone(), repo.clone(), number)).cloned());

    let result = perform(&action, &owner, &repo, number, cached.as_ref());
    // Refresh even on failure: a merge that succeeded before its branch
    // delete failed has still changed the PR.
    refresh_status_now(state, &owner, &repo, number);
    result?;

    let app = state.app.lock().map_err(|e| e.to_string())?;
    Ok(snap_from(&app, state))
}

fn perform(
    action: &PrActionRequest,
    owner: &str,
    repo: &str,
    number: u64,
    cached: Option<&GithubStatusSnapshot>,
) -> Result<(), String> {
    let run =
        |a: PrAction| actions::run_pr_action(owner, repo, number, &a).map_err(|e| e.to_string());
    match action {
        PrActionRequest::Merge {
            method,
            auto,
            expected_head,
            delete_branch,
        } => {
            // An auto-merge has not happened yet, so there is nothing to
            // delete now; the box offers Delete once it has merged.
            let delete = *delete_branch && !*auto;
            // Check before merging, so a refusal never follows a merge.
            let status = if delete {
                Some(same_repo_status(cached)?)
            } else {
                None
            };
            run(PrAction::Merge {
                method: *method,
                auto: *auto,
                head_oid: expected_head.clone(),
            })?;
            if let Some(status) = status {
                let merged = merged_now(actions::gh_pr_state(owner, repo, number))?;
                if merged {
                    // The merge was pinned to `expected_head`, so that is the
                    // tip the branch must still have to be safe to delete.
                    actions::gh_delete_remote_branch(owner, repo, &status.head_ref, expected_head)
                        .map_err(|e| format!("Merged, but deleting the branch failed: {e}"))?;
                }
            }
            Ok(())
        }
        PrActionRequest::DisableAutoMerge => run(PrAction::DisableAutoMerge),
        PrActionRequest::UpdateBranch { rebase } => run(PrAction::UpdateBranch { rebase: *rebase }),
        PrActionRequest::Close => run(PrAction::Close),
        PrActionRequest::Reopen => run(PrAction::Reopen),
        PrActionRequest::MarkReady => run(PrAction::MarkReady),
        PrActionRequest::ConvertToDraft => run(PrAction::ConvertToDraft),
        PrActionRequest::DeleteBranch => {
            let status = same_repo_status(cached)?;
            // The cached state can be 30s old; a PR reopened on GitHub since
            // would be closed again by deleting its branch, so ask live too.
            not_open(&status.state)?;
            not_open(&actions::gh_pr_state(owner, repo, number).map_err(|e| {
                format!("Could not confirm the PR is closed, so the branch was kept: {e}")
            })?)?;
            actions::gh_delete_remote_branch(owner, repo, &status.head_ref, &status.head_oid)
                .map_err(|e| e.to_string())
        }
        PrActionRequest::RestoreBranch => {
            let status = same_repo_status(cached)?;
            actions::gh_restore_remote_branch(owner, repo, &status.head_ref, &status.head_oid)
                .map_err(|e| e.to_string())
        }
    }
}

/// Deleting an open PR's head branch closes the PR on GitHub.
fn not_open(state: &str) -> Result<(), String> {
    if state == "OPEN" {
        Err("Close or merge the PR before deleting its branch.".to_string())
    } else {
        Ok(())
    }
}

/// Whether a merge that `gh` accepted has actually landed. On a repo with a
/// merge queue, `gh pr merge` only enqueues the PR, and deleting its branch
/// then would drop it from the queue. A queued PR keeps its branch; the box
/// offers Delete once GitHub reports it merged.
fn merged_now(state: anyhow::Result<String>) -> Result<bool, String> {
    match state {
        Ok(s) => Ok(s == "MERGED"),
        Err(e) => Err(format!(
            "Merge sent, but its result could not be read, so the branch was kept: {e}"
        )),
    }
}

/// The cached status, for an action on the head branch. Refuses fork PRs: the
/// branch lives in the fork, and the base repo may hold an unrelated branch of
/// the same name, which a delete there would destroy (ADR 0040).
fn same_repo_status(
    cached: Option<&GithubStatusSnapshot>,
) -> Result<&GithubStatusSnapshot, String> {
    let status = cached
        .filter(|s| !s.head_ref.is_empty())
        .ok_or_else(|| "GitHub status not loaded yet. Refresh and try again.".to_string())?;
    if status.is_cross_repository {
        return Err("This PR's branch lives in a fork. Manage it on GitHub.".to_string());
    }
    Ok(status)
}

/// Synchronous status fetch. `kick_github_status_refresh` skips when a fetch
/// is already in flight, which would leave the card showing the state from
/// before the action until the next 30s poll. The ticket keeps that in-flight
/// fetch from overwriting this one when it finishes later.
fn refresh_status_now(state: &AppState, owner: &str, repo: &str, number: u64) {
    if fetch_and_store_github_status(
        &state.gh_status_cache,
        &state.desktop_revision,
        owner,
        repo,
        number,
    ) {
        crate::gh_status_cache::save_persisted_gh_status_cache(&state.gh_status_cache);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_deserialize_from_the_frontend_shape() {
        let merge: PrActionRequest = serde_json::from_str(
            r#"{"kind":"merge","method":"squash","expected_head":"abc","delete_branch":true}"#,
        )
        .unwrap();
        assert_eq!(
            merge,
            PrActionRequest::Merge {
                method: MergeMethod::Squash,
                auto: false,
                expected_head: "abc".into(),
                delete_branch: true,
            }
        );
        let update: PrActionRequest =
            serde_json::from_str(r#"{"kind":"update_branch","rebase":true}"#).unwrap();
        assert_eq!(update, PrActionRequest::UpdateBranch { rebase: true });
        for (json, want) in [
            (r#"{"kind":"close"}"#, PrActionRequest::Close),
            (r#"{"kind":"reopen"}"#, PrActionRequest::Reopen),
            (r#"{"kind":"mark_ready"}"#, PrActionRequest::MarkReady),
            (
                r#"{"kind":"convert_to_draft"}"#,
                PrActionRequest::ConvertToDraft,
            ),
            (r#"{"kind":"delete_branch"}"#, PrActionRequest::DeleteBranch),
            (
                r#"{"kind":"restore_branch"}"#,
                PrActionRequest::RestoreBranch,
            ),
            (
                r#"{"kind":"disable_auto_merge"}"#,
                PrActionRequest::DisableAutoMerge,
            ),
        ] {
            assert_eq!(serde_json::from_str::<PrActionRequest>(json).unwrap(), want);
        }
    }

    #[test]
    fn target_must_match_the_active_pr() {
        let target: PrTarget =
            serde_json::from_str(r#"{"owner":"Org","repo":"Repo","number":7}"#).unwrap();
        assert!(target.matches("org", "repo", 7));
        // Same number, another repo: the confirm was built for a different PR.
        assert!(!target.matches("org", "other", 7));
        assert!(!target.matches("org", "repo", 8));
    }

    #[test]
    fn branch_is_deleted_only_once_the_merge_has_landed() {
        assert_eq!(merged_now(Ok("MERGED".into())), Ok(true));
        // Merge queue: gh enqueued the PR; deleting now would drop it.
        assert_eq!(merged_now(Ok("OPEN".into())), Ok(false));
        let err = merged_now(Err(anyhow::anyhow!("timeout"))).unwrap_err();
        assert!(err.contains("branch was kept"), "{err}");
    }

    #[test]
    fn unknown_merge_method_is_rejected_at_the_boundary() {
        let json = r#"{"kind":"merge","method":"fast-forward","expected_head":"abc"}"#;
        assert!(serde_json::from_str::<PrActionRequest>(json).is_err());
    }

    #[test]
    fn branch_of_an_open_pr_is_never_deleted() {
        assert!(not_open("OPEN").is_err());
        assert!(not_open("MERGED").is_ok());
        assert!(not_open("CLOSED").is_ok());
    }

    #[test]
    fn merge_without_expected_head_is_rejected_at_the_boundary() {
        assert!(
            serde_json::from_str::<PrActionRequest>(r#"{"kind":"merge","method":"merge"}"#)
                .is_err()
        );
    }

    fn fork_status(state: &str) -> GithubStatusSnapshot {
        let mut s = open_status();
        s.state = state.into();
        s.is_cross_repository = true;
        s.head_oid = "abc".into();
        s
    }

    #[test]
    fn branch_actions_on_a_fork_pr_are_refused_before_any_gh_call() {
        // The base repo may hold an unrelated branch with the fork's branch
        // name; deleting it there would destroy someone else's work.
        let merged = fork_status("MERGED");
        for action in [
            PrActionRequest::DeleteBranch,
            PrActionRequest::RestoreBranch,
        ] {
            let err = perform(&action, "o", "r", 1, Some(&merged)).unwrap_err();
            assert!(err.contains("fork"), "{err}");
        }
    }

    #[test]
    fn merge_with_delete_on_a_fork_pr_is_refused_before_merging() {
        let open = fork_status("OPEN");
        let merge = PrActionRequest::Merge {
            method: MergeMethod::Merge,
            auto: false,
            expected_head: "abc".into(),
            delete_branch: true,
        };
        let err = perform(&merge, "o", "r", 1, Some(&open)).unwrap_err();
        // Not "Merged, but …": the refusal came before the merge ran.
        assert!(err.contains("fork") && !err.contains("Merged"), "{err}");
    }

    fn open_status() -> GithubStatusSnapshot {
        serde_json::from_value(serde_json::json!({
            "owner": "o", "repo": "r", "number": 1, "url": "", "state": "OPEN",
            "is_draft": false, "title": "", "body": "", "author": "",
            "head_ref": "feat/x", "base_ref": "main", "labels": [], "checks": [],
            "comments_count": 0, "reviews_count": 0,
            "recent_comments": [], "recent_reviews": [], "last_updated": null
        }))
        .unwrap()
    }

    #[test]
    fn deleting_an_open_prs_branch_is_refused_before_any_gh_call() {
        let status = open_status();
        let err = perform(&PrActionRequest::DeleteBranch, "o", "r", 1, Some(&status)).unwrap_err();
        assert!(err.contains("before deleting"), "{err}");
    }

    #[test]
    fn branch_actions_need_loaded_status() {
        let err = perform(&PrActionRequest::RestoreBranch, "o", "r", 1, None).unwrap_err();
        assert!(err.contains("not loaded"), "{err}");
    }
}
