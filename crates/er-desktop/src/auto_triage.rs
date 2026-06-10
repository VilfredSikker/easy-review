//! Manual and (legacy) per-project auto-triage for open PRs.
//!
//! Auto-dispatch on PR cache refresh is disabled — triage is started from the
//! sidebar (`run_pr_triage` / `run_branch_triage`). The `auto_triage` project flag
//! is kept for settings compatibility but no longer queues work on refresh.

use std::collections::HashSet;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use er_engine::ai::prompts;
use er_engine::app::{App, BackgroundTaskTarget};

use crate::projects::ProjectRecord;
use crate::snapshot::PrInfo;

pub type AutoTriageInFlight = Arc<Mutex<HashSet<String>>>;

#[derive(Clone)]
pub struct AutoTriageContext {
    pub app: Arc<Mutex<App>>,
    pub in_flight: AutoTriageInFlight,
    pub desktop_revision: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
pub struct AutoTriageQueueContext<'a> {
    pub is_my_pr: bool,
    pub requested_me: bool,
    pub is_new_pr: bool,
    pub triaged_head_oid: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct AutoTriageRequest {
    pub project_id: String,
    pub remote: String,
    pub repo_root: String,
    pub pr_number: u64,
    pub head_oid: String,
    pub base_ref: String,
    pub review_ignore_globs: Vec<String>,
    pub auto_triage_max_diff_kb: u32,
}

pub fn should_queue_auto_triage(
    project: &ProjectRecord,
    pr: &PrInfo,
    ctx: AutoTriageQueueContext<'_>,
) -> bool {
    if !project.auto_triage {
        return false;
    }
    if project.remote.is_none() {
        return false;
    }
    if pr.is_draft || pr.state != "OPEN" {
        return false;
    }
    if ctx.is_my_pr && !project.auto_triage_own_prs {
        return false;
    }
    if pr.head_oid.is_empty() {
        return false;
    }
    if ctx.triaged_head_oid == Some(pr.head_oid.as_str()) {
        return false;
    }

    match project.auto_triage_when.as_str() {
        "new-only" => ctx.is_new_pr,
        "review-requested" => ctx.requested_me,
        _ => true, // new-and-push (default)
    }
}

fn flight_key(req: &AutoTriageRequest) -> String {
    format!("{}#{}#{}", req.remote, req.pr_number, req.head_oid)
}

fn fetch_pr_diff(repo_root: &str, remote: &str, pr_number: u64) -> Result<String, String> {
    if !repo_root.is_empty() {
        return er_engine::github::gh_pr_diff(pr_number, repo_root).map_err(|e| e.to_string());
    }
    let Some((owner, repo)) = remote.split_once('/') else {
        return Err(format!("Invalid remote slug: {remote}"));
    };
    er_engine::github::gh_pr_diff_remote(owner, repo, pr_number).map_err(|e| e.to_string())
}

fn resolve_er_dir(remote: &str, repo_root: &str, pr_number: u64) -> Result<String, String> {
    let slug = if !repo_root.is_empty() {
        er_engine::github::canonical_owner_repo_slug(repo_root)
            .unwrap_or_else(|| er_engine::storage::slug_branch(&remote.to_lowercase()))
    } else {
        er_engine::storage::slug_branch(&remote.to_lowercase())
    };
    let er_root = er_engine::storage::resolve_managed_root_for_pr_bucket(&slug, pr_number);
    let er_dir = er_root.er_dir();
    if er_dir.is_empty() {
        return Err("Failed to resolve managed PR storage".to_string());
    }
    Ok(er_dir)
}

pub fn dispatch_auto_triage(ctx: &AutoTriageContext, requests: Vec<AutoTriageRequest>) {
    for req in requests {
        let key = flight_key(&req);
        {
            let mut guard = match ctx.in_flight.lock() {
                Ok(g) => g,
                Err(_) => continue,
            };
            if !guard.insert(key.clone()) {
                continue;
            }
        }

        let ctx = AutoTriageContext {
            app: Arc::clone(&ctx.app),
            in_flight: Arc::clone(&ctx.in_flight),
            desktop_revision: Arc::clone(&ctx.desktop_revision),
        };

        std::thread::spawn(move || {
            let result = run_auto_triage_once(&ctx, &req);
            if let Err(e) = &result {
                log::warn!(
                    "auto_triage failed project={} pr=#{}: {e}",
                    req.project_id,
                    req.pr_number
                );
            }
            if let Ok(mut guard) = ctx.in_flight.lock() {
                guard.remove(&key);
            }
        });
    }
}

fn run_auto_triage_once(ctx: &AutoTriageContext, req: &AutoTriageRequest) -> Result<(), String> {
    let mut raw_diff = fetch_pr_diff(&req.repo_root, &req.remote, req.pr_number)?;
    if !req.review_ignore_globs.is_empty() {
        raw_diff =
            er_engine::git::filter_raw_diff_exclude_globs(&raw_diff, &req.review_ignore_globs);
    }
    if req.auto_triage_max_diff_kb > 0
        && raw_diff.len() > (req.auto_triage_max_diff_kb as usize) * 1024
    {
        log::info!(
            "auto_triage skip {}#{}: diff {}KB > limit {}KB",
            req.remote,
            req.pr_number,
            raw_diff.len() / 1024,
            req.auto_triage_max_diff_kb
        );
        return Ok(());
    }
    if raw_diff.trim().is_empty() {
        return Ok(());
    }

    let er_dir = resolve_er_dir(&req.remote, &req.repo_root, req.pr_number)?;
    std::fs::create_dir_all(&er_dir).map_err(|e| format!("mkdir {er_dir}: {e}"))?;
    std::fs::write(format!("{er_dir}/diff-tmp"), &raw_diff)
        .map_err(|e| format!("write diff-tmp: {e}"))?;

    let base_branch = if req.base_ref.is_empty() {
        "main".to_string()
    } else {
        req.base_ref.clone()
    };

    let target = BackgroundTaskTarget {
        repo_root: req.repo_root.clone(),
        er_dir: er_dir.clone(),
        branch_label: format!("pr-{}", req.pr_number),
        base_branch,
        scope: "branch".to_string(),
        pr_number: Some(req.pr_number),
        remote_repo: Some(req.remote.clone()),
        managed_local: !req.repo_root.is_empty(),
    };

    let prompt = prompts::build_triage_review_prompt_prepared_diff("branch", &er_dir);
    let mut app = ctx.app.lock().map_err(|e| e.to_string())?;
    app.spawn_background_triage_review(target, prompt, true)
        .map_err(|e| e.to_string())?;
    app.notify(&format!(
        "Triage started for {}#{}",
        req.remote, req.pr_number
    ));
    crate::profile_log::bump_desktop_revision(&ctx.desktop_revision, "manual_triage_started");
    Ok(())
}

/// Triage a local branch diff when no open PR matches `branch` (uses branch view-bucket).
pub fn dispatch_branch_triage(
    ctx: &AutoTriageContext,
    _project_id: &str,
    remote: &str,
    repo_root: &str,
    branch: &str,
    review_ignore_globs: &[String],
) {
    let key = format!("branch:{remote}:{branch}");
    {
        let mut guard = match ctx.in_flight.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if !guard.insert(key.clone()) {
            return;
        }
    }

    let ctx = AutoTriageContext {
        app: Arc::clone(&ctx.app),
        in_flight: Arc::clone(&ctx.in_flight),
        desktop_revision: Arc::clone(&ctx.desktop_revision),
    };
    let remote = remote.to_string();
    let repo_root = repo_root.to_string();
    let branch = branch.to_string();
    let review_ignore_globs = review_ignore_globs.to_vec();
    std::thread::spawn(move || {
        let result =
            run_branch_triage_once(&ctx, &remote, &repo_root, &branch, &review_ignore_globs);
        if let Err(e) = &result {
            log::warn!("branch_triage failed branch={branch}: {e}");
        }
        if let Ok(mut guard) = ctx.in_flight.lock() {
            guard.remove(&key);
        }
    });
}

fn run_branch_triage_once(
    ctx: &AutoTriageContext,
    remote: &str,
    repo_root: &str,
    branch: &str,
    review_ignore_globs: &[String],
) -> Result<(), String> {
    if repo_root.is_empty() {
        return Err("Branch triage requires a local clone".to_string());
    }
    let base_branch =
        er_engine::git::detect_base_branch_in(repo_root).map_err(|e| e.to_string())?;
    let mut raw_diff = er_engine::git::git_diff_raw_range(&base_branch, branch, repo_root)
        .map_err(|e| e.to_string())?;
    if !review_ignore_globs.is_empty() {
        raw_diff = er_engine::git::filter_raw_diff_exclude_globs(&raw_diff, review_ignore_globs);
    }
    if raw_diff.trim().is_empty() {
        return Err("Nothing to triage on this branch".to_string());
    }

    let slug = er_engine::github::canonical_owner_repo_slug(repo_root)
        .unwrap_or_else(|| er_engine::storage::slug_branch(&remote.to_lowercase()));
    let branch_slug = er_engine::storage::slug_branch(branch);
    let er_dir =
        er_engine::storage::resolve_managed_root_for_view_bucket(&slug, &branch_slug, "branch")
            .er_dir();
    if er_dir.is_empty() {
        return Err("Failed to resolve branch storage".to_string());
    }
    std::fs::create_dir_all(&er_dir).map_err(|e| format!("mkdir {er_dir}: {e}"))?;
    std::fs::write(format!("{er_dir}/diff-tmp"), &raw_diff)
        .map_err(|e| format!("write diff-tmp: {e}"))?;

    let target = BackgroundTaskTarget {
        repo_root: repo_root.to_string(),
        er_dir: er_dir.clone(),
        branch_label: branch.to_string(),
        base_branch: base_branch.clone(),
        scope: "branch".to_string(),
        pr_number: None,
        remote_repo: Some(remote.to_string()),
        managed_local: true,
    };

    let prompt = prompts::build_triage_review_prompt_prepared_diff("branch", &er_dir);
    let mut app = ctx.app.lock().map_err(|e| e.to_string())?;
    app.spawn_background_triage_review(target, prompt, true)
        .map_err(|e| e.to_string())?;
    app.notify(&format!("Triage started for branch {branch}"));
    crate::profile_log::bump_desktop_revision(&ctx.desktop_revision, "manual_triage_started");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::ProjectRecord;

    fn project(auto: bool) -> ProjectRecord {
        ProjectRecord {
            id: "discovery".to_string(),
            name: "discovery".to_string(),
            root_path: "/tmp/discovery".to_string(),
            remote: Some("reshapebiotech/discovery".to_string()),
            dismissed_prs: Vec::new(),
            tracked_prs: Vec::new(),
            tracked_branches: Vec::new(),
            dismissed_branches: Vec::new(),
            recent_prs: Vec::new(),
            saved_prs: Vec::new(),
            auto_triage: auto,
            auto_triage_own_prs: false,
            auto_triage_when: "new-and-push".to_string(),
            auto_triage_max_diff_kb: 0,
            review_ignore_globs: Vec::new(),
        }
    }

    fn pr(head_oid: &str) -> PrInfo {
        PrInfo {
            number: 42,
            title: "Test".to_string(),
            head_ref: "feat".to_string(),
            state: "OPEN".to_string(),
            is_draft: false,
            author: "other".to_string(),
            assignees: Vec::new(),
            reviewers: Vec::new(),
            checks_state: None,
            review_decision: None,
            merged_at: None,
            approved_by_me: false,
            base_ref: "main".to_string(),
            head_oid: head_oid.to_string(),
            updated_at: String::new(),
            latest_reviewer_states: Vec::new(),
        }
    }

    fn ctx<'a>(
        is_my_pr: bool,
        requested_me: bool,
        is_new_pr: bool,
        triaged: Option<&'a str>,
    ) -> AutoTriageQueueContext<'a> {
        AutoTriageQueueContext {
            is_my_pr,
            requested_me,
            is_new_pr,
            triaged_head_oid: triaged,
        }
    }

    #[test]
    fn queues_when_enabled_and_not_yet_triaged() {
        assert!(should_queue_auto_triage(
            &project(true),
            &pr("abc"),
            ctx(false, false, true, None)
        ));
    }

    #[test]
    fn skips_when_disabled_or_own_pr_or_already_triaged() {
        assert!(!should_queue_auto_triage(
            &project(false),
            &pr("abc"),
            ctx(false, false, true, None)
        ));
        assert!(!should_queue_auto_triage(
            &project(true),
            &pr("abc"),
            ctx(true, false, true, None)
        ));
        assert!(!should_queue_auto_triage(
            &project(true),
            &pr("abc"),
            ctx(false, false, true, Some("abc"))
        ));
    }

    #[test]
    fn queues_own_pr_when_project_allows() {
        let mut p = project(true);
        p.auto_triage_own_prs = true;
        assert!(should_queue_auto_triage(
            &p,
            &pr("abc"),
            ctx(true, false, true, None)
        ));
    }

    #[test]
    fn new_only_skips_push_updates() {
        let mut p = project(true);
        p.auto_triage_when = "new-only".to_string();
        assert!(!should_queue_auto_triage(
            &p,
            &pr("abc"),
            ctx(false, false, false, None)
        ));
        assert!(should_queue_auto_triage(
            &p,
            &pr("abc"),
            ctx(false, false, true, None)
        ));
    }

    #[test]
    fn review_requested_requires_reviewer() {
        let mut p = project(true);
        p.auto_triage_when = "review-requested".to_string();
        assert!(!should_queue_auto_triage(
            &p,
            &pr("abc"),
            ctx(false, false, true, None)
        ));
        assert!(should_queue_auto_triage(
            &p,
            &pr("abc"),
            ctx(false, true, true, None)
        ));
    }
}
