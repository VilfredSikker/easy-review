//! Automatic cheap-model triage for PRs in the To Review sidebar bucket.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use er_engine::ai::prompts::build_triage_prompt_prepared_diff;
use er_engine::ai::review::ErReview;
use er_engine::ai::{compute_diff_hash, load_triage};
use er_engine::app::App;
use er_engine::app::BackgroundTaskTarget;

use crate::commands::AppState;
use crate::er_storage;
use crate::inbox::{InboxHandle, ObservedPrState};
use crate::snapshot::pr_is_to_review;
use crate::snapshot::PrInfo;

fn triage_running_count(app: &App) -> u32 {
    app.background_tasks
        .values()
        .filter(|h| {
            h.task.kind == "triage"
                && matches!(h.task.status, er_engine::app::CommandStatus::Running)
        })
        .count() as u32
}

fn triage_running_for_pr(app: &App, remote: &str, pr_number: u64) -> bool {
    app.background_tasks.values().any(|h| {
        h.task.kind == "triage"
            && matches!(h.task.status, er_engine::app::CommandStatus::Running)
            && h.task.target.remote_repo.as_deref() == Some(remote)
            && h.task.target.pr_number == Some(pr_number)
    })
}

fn fresh_review_exists(er_dir: &str, diff_hash: &str) -> bool {
    let path = format!("{er_dir}/review.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(review) = serde_json::from_str::<ErReview>(&content) else {
        return false;
    };
    review.diff_hash == diff_hash
}

fn needs_triage_scan(
    pr: &PrInfo,
    prev: Option<&ObservedPrState>,
    gh_user: &str,
    er_dir: &str,
) -> bool {
    if !pr_is_to_review(pr, Some(gh_user)) || pr.is_draft {
        return false;
    }
    if pr.head_oid.is_empty() {
        return false;
    }
    if load_triage(er_dir).is_some() {
        return false;
    }
    if prev.map(|p| p.triage_done).unwrap_or(false) {
        return false;
    }
    let in_to_review = prev.map(|p| p.in_to_review).unwrap_or(false);
    let triage_failed = prev.map(|p| p.triage_failed).unwrap_or(false);
    // One-time per PR: only on first entry into To Review, or retry after failure.
    if in_to_review && !triage_failed {
        return false;
    }
    true
}

/// Schedule preemptive triage scans for eligible To Review PRs. Called from `poll()`.
pub fn maybe_schedule_preemptive_scans(app: &mut App, state: &AppState) {
    let cfg = &app.config.automation.preemptive;
    if !cfg.enabled {
        return;
    }

    let gh_user = match state.gh_user.lock().ok().and_then(|g| g.clone()) {
        Some(u) => u,
        None => return,
    };

    let pr_cache = match state.pr_cache.lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };

    let projects = crate::projects::load();
    let mut project_by_remote: HashMap<String, (String, String)> = HashMap::new();
    for p in projects.projects {
        if let Some(remote) = p.remote {
            project_by_remote.insert(remote, (p.id, p.root_path));
        }
    }

    let max = cfg.max_concurrent;
    let mut running = triage_running_count(app);
    if running >= max {
        return;
    }

    let mut mark_done: Vec<String> = Vec::new();
    let mut clear_failed: Vec<String> = Vec::new();

    'outer: for (remote, prs) in &pr_cache {
        let Some((project_id, repo_root)) = project_by_remote.get(remote) else {
            continue;
        };
        for pr in prs {
            if running >= max {
                break 'outer;
            }
            let key = format!("{remote}#{}", pr.number);
            let prev = state
                .inbox
                .lock()
                .ok()
                .and_then(|inbox| inbox.observed_pr.get(&key).cloned());

            let er_dir = er_storage::pr_review_er_dir(remote, pr.number);

            if !needs_triage_scan(pr, prev.as_ref(), &gh_user, &er_dir) {
                continue;
            }
            if triage_running_for_pr(app, remote, pr.number) {
                continue;
            }

            let _ = std::fs::create_dir_all(&er_dir);

            let raw_diff = match er_engine::github::gh_pr_diff_remote(
                remote.split_once('/').map(|(o, _)| o).unwrap_or(""),
                remote.split_once('/').map(|(_, r)| r).unwrap_or(""),
                pr.number,
            ) {
                Ok(d) if !d.trim().is_empty() => d,
                _ => continue,
            };

            let diff_hash = compute_diff_hash(&raw_diff);
            if cfg.skip_if_review_exists && fresh_review_exists(&er_dir, &diff_hash) {
                mark_done.push(key);
                continue;
            }

            if let Err(e) = std::fs::write(format!("{er_dir}/diff-tmp"), &raw_diff) {
                log::warn!("preemptive triage: failed to write diff-tmp for {key}: {e}");
                continue;
            }

            let prompt = build_triage_prompt_prepared_diff(&er_dir, &pr.head_oid);
            let target = BackgroundTaskTarget {
                repo_root: repo_root.clone(),
                er_dir: er_dir.clone(),
                branch_label: pr.head_ref.clone(),
                base_branch: pr.base_ref.clone(),
                scope: "branch".to_string(),
                pr_number: Some(pr.number),
                remote_repo: Some(remote.clone()),
                managed_local: !repo_root.is_empty(),
            };

            if app.spawn_background_triage(target, prompt, true).is_err() {
                continue;
            }

            running += 1;
            clear_failed.push(key);
            log::info!(
                "preemptive triage scheduled project={project_id} {remote}#{} head={}",
                pr.number,
                pr.head_oid
            );
        }
    }

    if !mark_done.is_empty() || !clear_failed.is_empty() {
        if let Ok(mut inbox) = state.inbox.lock() {
            for key in mark_done {
                if let Some(obs) = inbox.observed_pr.get_mut(&key) {
                    obs.triage_done = true;
                    obs.triage_failed = false;
                }
            }
            for key in clear_failed {
                if let Some(obs) = inbox.observed_pr.get_mut(&key) {
                    obs.triage_failed = false;
                }
            }
            crate::inbox::save_inbox_state(&state.inbox);
        }
        state.desktop_revision.fetch_add(1, Ordering::Relaxed);
    }
}

/// Revert triage tracking when a triage task fails so one retry is allowed.
pub fn revert_failed_triage_heads(app: &App, inbox_handle: &InboxHandle) {
    let failed: Vec<(Option<String>, u64)> = app
        .background_task_snapshots()
        .into_iter()
        .filter(|t| t.kind == "triage" && t.status == "failed")
        .filter_map(|t| Some((t.remote_repo.clone(), t.pr_number?)))
        .collect();

    if failed.is_empty() {
        return;
    }

    if let Ok(mut inbox) = inbox_handle.lock() {
        let mut changed = false;
        for (remote, pr_number) in failed {
            let Some(remote) = remote else {
                continue;
            };
            let key = format!("{remote}#{pr_number}");
            if let Some(obs) = inbox.observed_pr.get_mut(&key) {
                obs.triage_done = false;
                obs.triage_failed = true;
                changed = true;
            }
        }
        if changed {
            crate::inbox::save_inbox_state(inbox_handle);
        }
    }
}

/// On successful triage, mark the PR as permanently triaged.
pub fn sync_successful_triage_heads(app: &App, inbox_handle: &InboxHandle) {
    let done: Vec<(String, u64)> = app
        .background_task_snapshots()
        .into_iter()
        .filter(|t| t.kind == "triage" && t.status == "done")
        .filter_map(|t| {
            let remote = t.remote_repo.clone()?;
            let pr_number = t.pr_number?;
            Some((remote, pr_number))
        })
        .collect();

    if done.is_empty() {
        return;
    }

    if let Ok(mut inbox) = inbox_handle.lock() {
        let mut changed = false;
        for (remote, pr_number) in done {
            let key = format!("{remote}#{pr_number}");
            if let Some(obs) = inbox.observed_pr.get_mut(&key) {
                obs.triage_done = true;
                obs.triage_failed = false;
                changed = true;
            }
        }
        if changed {
            crate::inbox::save_inbox_state(inbox_handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::pr_is_to_review;
    use std::fs;

    fn sample_pr() -> PrInfo {
        PrInfo {
            number: 1,
            title: "t".into(),
            head_ref: "feat".into(),
            state: "OPEN".into(),
            is_draft: false,
            author: "other".into(),
            assignees: vec![],
            reviewers: vec![],
            checks_state: None,
            review_decision: None,
            merged_at: None,
            approved_by_me: false,
            base_ref: "main".into(),
            head_oid: "abc".into(),
            updated_at: String::new(),
            latest_reviewer_states: vec![],
        }
    }

    fn sample_prev(in_to_review: bool, triage_done: bool, triage_failed: bool) -> ObservedPrState {
        ObservedPrState {
            in_to_review,
            triage_done,
            triage_failed,
            ..Default::default()
        }
    }

    #[test]
    fn pr_is_to_review_excludes_drafts_and_own_prs() {
        let mut pr = sample_pr();
        assert!(pr_is_to_review(&pr, Some("me")));
        pr.is_draft = true;
        assert!(!pr_is_to_review(&pr, Some("me")));
        pr.is_draft = false;
        pr.author = "me".into();
        assert!(!pr_is_to_review(&pr, Some("me")));
    }

    #[test]
    fn pr_is_to_review_excludes_already_reviewed() {
        let mut pr = sample_pr();
        pr.latest_reviewer_states
            .push(("me".into(), "APPROVED".into()));
        assert!(!pr_is_to_review(&pr, Some("me")));
    }

    #[test]
    fn needs_triage_skips_when_triage_done() {
        let pr = sample_pr();
        let prev = sample_prev(true, true, false);
        let dir = tempfile::tempdir().unwrap();
        assert!(!needs_triage_scan(
            &pr,
            Some(&prev),
            "me",
            dir.path().to_str().unwrap()
        ));
    }

    #[test]
    fn needs_triage_skips_when_triage_json_exists() {
        let pr = sample_pr();
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();
        fs::write(
            format!("{er_dir}/triage.json"),
            r#"{"version":1,"diff_hash":"x","verdict":"skip"}"#,
        )
        .unwrap();
        assert!(!needs_triage_scan(&pr, None, "me", er_dir));
    }

    #[test]
    fn needs_triage_first_entry_only_when_already_in_bucket() {
        let pr = sample_pr();
        let prev = sample_prev(true, false, false);
        let dir = tempfile::tempdir().unwrap();
        assert!(!needs_triage_scan(
            &pr,
            Some(&prev),
            "me",
            dir.path().to_str().unwrap()
        ));
    }

    #[test]
    fn needs_triage_allows_retry_after_failure() {
        let pr = sample_pr();
        let prev = sample_prev(true, false, true);
        let dir = tempfile::tempdir().unwrap();
        assert!(needs_triage_scan(
            &pr,
            Some(&prev),
            "me",
            dir.path().to_str().unwrap()
        ));
    }

    #[test]
    fn needs_triage_head_change_does_not_retrigger_after_done() {
        let mut pr = sample_pr();
        pr.head_oid = "def".into();
        let prev = sample_prev(true, true, false);
        let dir = tempfile::tempdir().unwrap();
        assert!(!needs_triage_scan(
            &pr,
            Some(&prev),
            "me",
            dir.path().to_str().unwrap()
        ));
    }
}
