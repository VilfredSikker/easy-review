//! Manual per-project triage for open PRs and branches.
//!
//! Triage starts from the sidebar (`run_pr_triage` / `run_branch_triage`); the
//! PR cache refresh never queues a run.

use std::collections::HashSet;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use er_engine::ai::prompts;
use er_engine::task_target::BackgroundTaskTarget;

pub type AutoTriageInFlight = Arc<Mutex<HashSet<String>>>;

/// Queues a prepared triage task, with its target and prompt.
pub type SpawnTriage =
    Arc<dyn Fn(BackgroundTaskTarget, String) -> Result<(), String> + Send + Sync>;

/// What a triage run reaches: the ports it reads the diff with, the host it
/// queues its task on, and the request set whose moves `jobs` shows.
#[derive(Clone)]
pub struct AutoTriageContext {
    pub ports: er_engine::ports::Ports,
    pub spawn: SpawnTriage,
    /// Tells the person once.
    pub notify: Arc<dyn Fn(String) + Send + Sync>,
    pub in_flight: AutoTriageInFlight,
    /// Called after each insert into or removal from `in_flight`.
    pub queue_moved: Arc<dyn Fn() + Send + Sync>,
    pub desktop_revision: Arc<AtomicU64>,
}

impl AutoTriageContext {
    /// Claims `key` in the request set; false when it is already there.
    fn claim(&self, key: &str) -> bool {
        let claimed = self
            .in_flight
            .lock()
            .is_ok_and(|mut guard| guard.insert(key.to_string()));
        if claimed {
            (self.queue_moved)();
        }
        claimed
    }

    fn release(&self, key: &str) {
        if let Ok(mut guard) = self.in_flight.lock() {
            guard.remove(key);
        }
        (self.queue_moved)();
    }
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

fn flight_key(req: &AutoTriageRequest) -> String {
    format!("{}#{}#{}", req.remote, req.pr_number, req.head_oid)
}

/// Split a project's remote text into an `owner/repo` pair, naming the remote
/// when it does not parse.
///
/// A project record holds a bare slug, but legacy and hand-edited records can
/// carry any remote form git writes, so the split goes through the shared
/// project parser rather than the first `/`.
fn remote_owner_repo(remote: &str) -> Result<(String, String), String> {
    crate::projects::remote_owner_repo(remote)
        .ok_or_else(|| format!("Invalid remote slug: {remote}"))
}

fn fetch_pr_diff(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    remote: &str,
    pr_number: u64,
) -> Result<String, String> {
    if !repo_root.is_empty() {
        return er_engine::github::gh_pr_diff(git, pr_number, repo_root).map_err(|e| e.to_string());
    }
    let (owner, repo) = remote_owner_repo(remote)?;
    er_engine::github::gh_pr_diff_remote(git, &owner, &repo, pr_number).map_err(|e| e.to_string())
}

fn resolve_er_dir(
    git: &dyn er_engine::git::Git,
    remote: &str,
    repo_root: &str,
    pr_number: u64,
) -> Result<String, String> {
    let slug = er_engine::storage::repo_storage_slug_for(
        git,
        repo_root,
        (!remote.is_empty()).then_some(remote),
    );
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
        if !ctx.claim(&key) {
            continue;
        }

        let ctx = ctx.clone();

        std::thread::spawn(move || {
            let result = run_auto_triage_once(&ctx, &req);
            if let Err(e) = &result {
                log::warn!(
                    "auto_triage failed project={} pr=#{}: {e}",
                    req.project_id,
                    req.pr_number
                );
            }
            ctx.release(&key);
        });
    }
}

fn run_auto_triage_once(ctx: &AutoTriageContext, req: &AutoTriageRequest) -> Result<(), String> {
    let ports = &ctx.ports;
    let mut raw_diff = fetch_pr_diff(&*ports.git, &req.repo_root, &req.remote, req.pr_number)?;
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

    let er_dir = resolve_er_dir(&*ports.git, &req.remote, &req.repo_root, req.pr_number)?;
    std::fs::create_dir_all(&er_dir).map_err(|e| format!("mkdir {er_dir}: {e}"))?;
    // A remote-only project has no clone to name the repo, so key off `owner/repo`.
    let rules_key = if req.repo_root.is_empty() {
        er_engine::storage::rules_key(&*ports.git, "", Some(&req.remote))
    } else {
        er_engine::storage::rules_key(&*ports.git, &req.repo_root, None)
    };
    let base_branch = if req.base_ref.is_empty() {
        "main".to_string()
    } else {
        req.base_ref.clone()
    };
    let review_md_source = if req.repo_root.is_empty() {
        er_engine::ai::review_md::ReviewMdSource::Gh {
            repo_slug: req.remote.clone(),
            base_ref: base_branch.clone(),
        }
    } else {
        er_engine::ai::review_md::ReviewMdSource::Git {
            repo_root: req.repo_root.clone(),
            base_ref: format!("origin/{base_branch}"),
        }
    };
    let review_md = er_engine::ai::review_md::collect(ports, &review_md_source, &raw_diff);
    let diff_hash = er_engine::ai::prepared_diff::ensure_review_inputs_from_config(
        &er_dir,
        &raw_diff,
        &rules_key,
        review_md.as_deref(),
    )?;

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

    let prompt = prompts::build_triage_review_prompt_prepared_diff("branch", &er_dir, &diff_hash);
    (ctx.spawn)(target, prompt)?;
    (ctx.notify)(format!(
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
    if !ctx.claim(&key) {
        return;
    }

    let ctx = ctx.clone();
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
        ctx.release(&key);
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
    let ports = &ctx.ports;
    let base_branch = ports
        .git
        .detect_base_branch(repo_root)
        .map_err(|e| e.to_string())?;
    let mut raw_diff = ports
        .git
        .diff_raw_range(&base_branch, branch, repo_root)
        .map_err(|e| e.to_string())?;
    if !review_ignore_globs.is_empty() {
        raw_diff = er_engine::git::filter_raw_diff_exclude_globs(&raw_diff, review_ignore_globs);
    }
    if raw_diff.trim().is_empty() {
        return Err("Nothing to triage on this branch".to_string());
    }

    let slug = er_engine::storage::repo_storage_slug_for(
        &*ports.git,
        repo_root,
        (!remote.is_empty()).then_some(remote),
    );
    let branch_slug = er_engine::storage::slug_branch(branch);
    let er_dir =
        er_engine::storage::resolve_managed_root_for_view_bucket(&slug, &branch_slug, "branch")
            .er_dir();
    if er_dir.is_empty() {
        return Err("Failed to resolve branch storage".to_string());
    }
    std::fs::create_dir_all(&er_dir).map_err(|e| format!("mkdir {er_dir}: {e}"))?;
    let review_md = er_engine::ai::review_md::collect(
        ports,
        &er_engine::ai::review_md::ReviewMdSource::Git {
            repo_root: repo_root.to_string(),
            base_ref: base_branch.clone(),
        },
        &raw_diff,
    );
    let diff_hash = er_engine::ai::prepared_diff::ensure_review_inputs_from_config(
        &er_dir,
        &raw_diff,
        &er_engine::storage::rules_key(&*ports.git, repo_root, None),
        review_md.as_deref(),
    )?;

    let target = BackgroundTaskTarget {
        repo_root: repo_root.to_string(),
        er_dir: er_dir.clone(),
        branch_label: branch.to_string(),
        base_branch,
        scope: "branch".to_string(),
        pr_number: None,
        remote_repo: Some(remote.to_string()),
        managed_local: true,
    };

    let prompt = prompts::build_triage_review_prompt_prepared_diff("branch", &er_dir, &diff_hash);
    (ctx.spawn)(target, prompt)?;
    (ctx.notify)(format!("Triage started for branch {branch}"));
    crate::profile_log::bump_desktop_revision(&ctx.desktop_revision, "manual_triage_started");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_owner_repo_reads_every_form_a_project_can_hold() {
        for (remote, owner, repo) in [
            // The form the project writers store.
            ("reshapebiotech/discovery", "reshapebiotech", "discovery"),
            // A legacy or hand-edited record can keep the spelling and suffix.
            (
                "ReshapeBiotech/Discovery.git",
                "ReshapeBiotech",
                "Discovery",
            ),
            (
                "https://github.com/reshapebiotech/discovery",
                "reshapebiotech",
                "discovery",
            ),
            (
                "https://github.com/reshapebiotech/discovery.git",
                "reshapebiotech",
                "discovery",
            ),
            (
                "https://github.com/reshapebiotech/discovery/",
                "reshapebiotech",
                "discovery",
            ),
            (
                "git@github.com:reshapebiotech/discovery.git",
                "reshapebiotech",
                "discovery",
            ),
            (
                "ssh://git@github.com/reshapebiotech/discovery.git",
                "reshapebiotech",
                "discovery",
            ),
            (
                "git://github.com/reshapebiotech/discovery",
                "reshapebiotech",
                "discovery",
            ),
        ] {
            assert_eq!(
                remote_owner_repo(remote),
                Ok((owner.to_string(), repo.to_string())),
                "{remote}"
            );
        }
    }

    #[test]
    fn remote_owner_repo_rejects_text_that_is_not_a_repository() {
        for remote in [
            "",
            "reshapebiotech",
            "/Users/me/discovery",
            "C:/Users/me/discovery",
            "a b/c",
        ] {
            assert!(remote_owner_repo(remote).is_err(), "{remote:?}");
        }
    }
}
