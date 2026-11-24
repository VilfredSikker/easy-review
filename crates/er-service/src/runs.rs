//! The run planners. Each reads one tab's published content and the settings
//! view, prepares the bucket and queues the agent on the task host. Planning
//! runs git, so a front end calls these off its loop (ADR 0074).

use er_engine::ai::{prompts, ReviewerKind, ScopedRunStamp};
use std::sync::Arc;

use er_engine::config::{AiSelection, ErConfig};
use er_engine::ports::Ports;
use er_engine::tab_identity::DiffMode;
use er_engine::task_target::BackgroundTaskTarget;
use er_protocol::ErError;

use crate::content::TabContent;
use crate::tasks::{self, refused, Launch, TaskHooks, TaskHost};

/// A run on one tab's content, queued on `host`.
pub struct RunTab<'a> {
    pub host: &'a TaskHost,
    pub hooks: &'a TaskHooks,
    pub content: &'a TabContent,
}

pub fn review(run: &RunTab<'_>, scope: &str) -> Result<(), ErError> {
    let content = run.content;
    let scope = review_scope(content, scope)?;
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    let raw = filtered(review_raw(&run.hooks.ports, content, &scope)?, content);
    if raw.trim().is_empty() {
        return Err(refused("Nothing to review"));
    }
    std::fs::create_dir_all(&er_dir)
        .map_err(|e| refused(format!("Failed to create branch managed directory: {e}")))?;
    let diff_hash = review_inputs(
        &run.hooks.ports,
        content,
        &(run.hooks.config)(),
        &er_dir,
        &raw,
    )?;
    let target = target(content, er_dir, scope);
    let prompt = prompts::build_review_prompt_prepared_diff(
        &target.scope,
        &target.er_dir,
        &target.base_branch,
        &target.branch_label,
        &diff_hash,
        None,
    );
    queue(run, "review", "review", target, prompt, None)
}

/// Writes into the tour bucket of the view the tab shows.
pub fn tour(run: &RunTab<'_>) -> Result<(), ErError> {
    let content = run.content;
    let id = content.tab.identity();
    let er_dir = id
        .tour_bucket_er_dir(&*run.hooks.ports.git)
        .unwrap_or_else(|| content.context.er_dir.clone());
    // No admission check: a tour prepares its own diff and writes only
    // `tour.json`, so it can run beside a review flow.
    let diff_dir = er_engine::ai::prepared_diff::tour_input_dir(&er_dir);
    std::fs::create_dir_all(&diff_dir)
        .map_err(|e| refused(format!("Failed to create tour managed directory: {e}")))?;
    let raw = filtered(review_raw(&run.hooks.ports, content, "branch")?, content);
    if raw.trim().is_empty() {
        return Err(refused("Nothing to tour"));
    }
    let diff_hash =
        er_engine::ai::prepared_diff::ensure_diff_artifacts(&diff_dir, &raw).map_err(refused)?;
    let scope_label = if id.tour_context_is_pr() {
        "PR diff"
    } else {
        "branch diff"
    };
    let prompt = prompts::build_tour_prompt_split_dirs(
        scope_label,
        &diff_dir,
        &er_dir,
        "tour.json",
        &diff_hash,
    );
    let target = target(content, er_dir, "branch".to_string());
    queue(run, "tour", "tour", target, prompt, None)
}

/// The prompt names the repo, not a diff.
pub fn importance(run: &RunTab<'_>) -> Result<(), ErError> {
    let content = run.content;
    if content.tab.identity().is_remote() {
        return Err(refused(
            "Ranking importance needs a local clone of this repo",
        ));
    }
    if content.tab.repo_root.is_empty() {
        return Err(refused(
            "Open a repository first — there is nothing to rank",
        ));
    }
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    let prompt =
        prompts::build_importance_prompt(&content.diff.meta.rules_key, &content.tab.repo_root);
    let target = target(content, er_dir, "branch".to_string());
    queue(
        run,
        prompts::IMPORTANCE_TASK_KIND,
        "importance",
        target,
        prompt,
        None,
    )
}

pub fn expert(run: &RunTab<'_>, scope: &str, expert_id: &str) -> Result<(), ErError> {
    if er_engine::ai::expert_by_id(expert_id).is_none() {
        return Err(refused(format!("Unknown expert: {expert_id}")));
    }
    let content = run.content;
    let scope = review_scope(content, scope)?;
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    std::fs::create_dir_all(&er_dir)
        .map_err(|e| refused(format!("Failed to create branch managed directory: {e}")))?;
    let raw = review_raw(&run.hooks.ports, content, &scope)?;
    if raw.trim().is_empty() {
        return Err(refused("Nothing to review"));
    }
    let raw = filtered(raw, content);
    let diff_hash = review_inputs(
        &run.hooks.ports,
        content,
        &(run.hooks.config)(),
        &er_dir,
        &raw,
    )?;
    let prompt = prompts::build_expert_review_prompt_prepared_diff(
        &scope, &er_dir, expert_id, &diff_hash, None,
    );
    let reviewer = ReviewerKind::Expert(expert_id.to_string());
    spawn_reviewer(
        run.host,
        run.hooks,
        &reviewer,
        target(content, er_dir, scope),
        prompt,
        None,
        false,
    )
    .map_err(refused)
}

/// The professor lens on the view the tab shows, by the App's rules: local
/// only, in Branch, Unstaged or Staged.
pub fn professor(run: &RunTab<'_>) -> Result<(), ErError> {
    let mode = run.content.tab.mode;
    if run.content.context.is_remote {
        return Err(refused(
            "Professor is local-only in v1 — checkout the PR first",
        ));
    }
    if !matches!(
        mode,
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged
    ) {
        return Err(refused("Professor not available in this mode"));
    }
    let (er_dir, diff_hash) = prepared_view(run)?;
    let prompt = prompts::build_professor_review_prompt_prepared_diff(
        mode.git_mode(),
        &er_dir,
        None,
        None,
        &diff_hash,
    );
    lens(run, &ReviewerKind::Professor, er_dir, prompt)
}

/// The triage lens on the view the tab shows; local only.
pub fn triage(run: &RunTab<'_>) -> Result<(), ErError> {
    if run.content.context.is_remote {
        return Err(refused(
            "Triage is local-only in v1 — checkout the PR first",
        ));
    }
    let (er_dir, diff_hash) = prepared_view(run)?;
    let mode = run.content.tab.mode;
    let prompt =
        prompts::build_triage_review_prompt_prepared_diff(mode.git_mode(), &er_dir, &diff_hash);
    lens(run, &ReviewerKind::Triage, er_dir, prompt)
}

/// The tab's view, filtered and prepared in its bucket, as the App prepares
/// it for a lens; answers the bucket and the diff's hash.
fn prepared_view(run: &RunTab<'_>) -> Result<(String, String), ErError> {
    let content = run.content;
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    std::fs::create_dir_all(&er_dir)
        .map_err(|e| refused(format!("Failed to create branch managed directory: {e}")))?;
    let scope = content.tab.mode.fetch_scope();
    let raw = filtered(review_raw(&run.hooks.ports, content, scope)?, content);
    if raw.trim().is_empty() {
        return Err(refused("Nothing to review"));
    }
    let config = (run.hooks.config)();
    let hash = review_inputs(&run.hooks.ports, content, &config, &er_dir, &raw)?;
    Ok((er_dir, hash))
}

fn lens(
    run: &RunTab<'_>,
    reviewer: &ReviewerKind,
    er_dir: String,
    prompt: String,
) -> Result<(), ErError> {
    let scope = run.content.tab.mode.fetch_scope().to_string();
    let target = target(run.content, er_dir, scope);
    spawn_reviewer(run.host, run.hooks, reviewer, target, prompt, None, false).map_err(refused)
}

/// Through the agent launcher, as the App's tab commands run. The comments
/// are the tab's own: its actor relocated them at its last load.
pub fn validate(run: &RunTab<'_>, scope: &str) -> Result<(), ErError> {
    let content = run.content;
    let scope = review_scope(content, scope)?;
    if content.context.is_remote {
        return Err(refused(
            "Validate review is local-only. Check out the PR locally first.",
        ));
    }
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    let has_review = er_engine::ai::review_file_exists(&er_dir);
    let comment_count = github_comment_count(
        content.ai.state.github_comments.as_ref(),
        &content.context.comments_dir,
    );
    if !has_review && comment_count == 0 {
        return Err(refused(
            "Nothing to validate. Run AI review or add GitHub comments first.",
        ));
    }
    let raw = review_raw(&run.hooks.ports, content, &scope)?;
    if raw.trim().is_empty() {
        return Err(refused("Nothing to validate"));
    }
    let diff_hash =
        er_engine::ai::prepared_diff::ensure_diff_artifacts(&er_dir, &raw).map_err(refused)?;
    let ports = &run.hooks.ports;
    let source = er_engine::review_flow_driver::review_md_source(ports, &content.tab.identity());
    let review_md = er_engine::ai::review_md::collect(ports, &source, &raw);
    er_engine::ai::review_md::write_review_md(&er_dir, review_md.as_deref()).map_err(refused)?;
    let target = target(content, er_dir.clone(), scope.clone());
    if has_review {
        let prompt = prompts::build_validate_prompt_prepared_diff(&scope, &er_dir, &diff_hash);
        agent(run, target.clone(), "validate", &prompt)?;
    }
    if comment_count > 0 {
        let prompt =
            prompts::build_validate_github_comments_prompt_prepared_diff(&er_dir, &diff_hash);
        agent(run, target, "validate-comments", &prompt)?;
    }
    let message = match (has_review, comment_count) {
        (true, n) if n > 0 => format!("Validation started (review + {n} comments)"),
        (true, _) => "Validation started (review)".to_string(),
        (false, n) => format!("Validation started ({n} comments)"),
    };
    (run.hooks.notify)(message, false);
    Ok(())
}

/// The saved selection (A18), resolved over the configured providers.
pub fn selection(config: &ErConfig) -> AiSelection {
    let hub = &config.ai_hub;
    let saved = hub.resolve_default_selection(&config.agent);
    let provider = hub.resolve_provider_id(saved.provider_id.as_deref());
    let model = provider
        .as_deref()
        .and_then(|provider| hub.resolve_model_id(provider, saved.model_id.as_deref()));
    let effort = er_engine::config::resolve_effort_for_model(
        hub,
        &config.agent,
        provider.as_deref(),
        model.as_deref(),
        saved.effort.as_deref(),
        None,
    );
    AiSelection {
        provider_id: provider,
        model_id: model,
        effort,
    }
}

/// Starts tab command `name` through the agent launcher, held on the host.
pub fn agent(
    run: &RunTab<'_>,
    target: BackgroundTaskTarget,
    name: &str,
    prompt: &str,
) -> Result<(), ErError> {
    let content = run.content;
    let config = (run.hooks.config)();
    let (repo_root, er_dir) = (target.repo_root.clone(), target.er_dir.clone());
    tasks::hold_agent(run.host, run.hooks, target, name, |log_tx| {
        er_engine::agent_prompt::launch_agent_prompt(er_engine::agent_prompt::AgentPromptRequest {
            name,
            prompt,
            repo_root,
            er_dir,
            is_remote: content.context.is_remote,
            remote_repo: content.tab.remote_repo.clone(),
            config: &config,
            selection: selection(&config),
            ports: &run.hooks.ports,
            log_tx,
        })
    })
    .map(|_| ())
    .map_err(refused)
}

/// Queues one finding lens, under the task kind and command name the App
/// gives it; `quiet` leaves its start for the caller to tell.
pub fn spawn_reviewer(
    host: &TaskHost,
    hooks: &TaskHooks,
    reviewer: &ReviewerKind,
    target: BackgroundTaskTarget,
    prompt: String,
    stamp: Option<ScopedRunStamp>,
    quiet: bool,
) -> Result<(), String> {
    let (kind, name, stamp) = match reviewer {
        ReviewerKind::Triage => (
            er_engine::ai::triage_task_kind(),
            "triage".to_string(),
            None,
        ),
        ReviewerKind::General => ("review".to_string(), "review".to_string(), stamp),
        ReviewerKind::Professor => (
            er_engine::ai::professor_task_kind(),
            "professor".to_string(),
            stamp,
        ),
        ReviewerKind::Expert(id) => {
            let def =
                er_engine::ai::expert_by_id(id).ok_or_else(|| format!("unknown expert: {id}"))?;
            (
                er_engine::ai::expert_task_kind(id),
                format!("expert-{}", def.id),
                stamp,
            )
        }
    };
    er_engine::arena::check_bucket_admission(&target.er_dir).map_err(|e| e.to_string())?;
    let launch = Launch {
        quiet,
        ..launch((hooks.config)(), &kind, &name, target, prompt, stamp)
    };
    tasks::enqueue(host, hooks, launch).map(|_| ())
}

fn queue(
    run: &RunTab<'_>,
    kind: &str,
    name: &str,
    target: BackgroundTaskTarget,
    prompt: String,
    stamp: Option<ScopedRunStamp>,
) -> Result<(), ErError> {
    let launch = launch((run.hooks.config)(), kind, name, target, prompt, stamp);
    tasks::enqueue(run.host, run.hooks, launch)
        .map(|_| ())
        .map_err(refused)
}

/// A launch on the settings view's config and saved selection (A18).
pub fn launch(
    config: Arc<ErConfig>,
    kind: &str,
    name: &str,
    target: BackgroundTaskTarget,
    prompt: String,
    scoped_stamp: Option<ScopedRunStamp>,
) -> Launch {
    Launch {
        kind: kind.to_string(),
        command_name: name.to_string(),
        target,
        prompt,
        prepared_diff: true,
        host_write_diagram: None,
        scoped_stamp,
        selection: selection(&config),
        config,
        quiet: false,
    }
}

/// The target a run on `content`'s tab files under.
pub fn target(content: &TabContent, er_dir: String, scope: String) -> BackgroundTaskTarget {
    let tab = &content.tab;
    BackgroundTaskTarget {
        repo_root: tab.repo_root.clone(),
        er_dir,
        branch_label: tab
            .local_branch_view
            .clone()
            .unwrap_or_else(|| tab.current_branch.clone()),
        base_branch: tab.base_branch.clone(),
        scope,
        pr_number: tab.pr_number,
        remote_repo: tab.remote_repo.clone(),
        managed_local: tab.remote_repo.is_none(),
    }
}

pub fn review_scope(content: &TabContent, scope: &str) -> Result<String, ErError> {
    resolve_review_scope_in(scope, content.tab.mode).map_err(refused)
}

pub fn admit(er_dir: &str) -> Result<(), ErError> {
    er_engine::arena::check_bucket_admission(er_dir).map_err(|e| refused(e.to_string()))
}

pub fn review_raw(ports: &Ports, content: &TabContent, scope: &str) -> Result<String, ErError> {
    crate::flow_view::review_raw_diff(ports, content, scope).map_err(|e| refused(e.to_string()))
}

/// `raw` without the files the project's review-ignore globs name.
pub fn filtered(raw: String, content: &TabContent) -> String {
    let id = content.tab.identity();
    let ignore =
        er_engine::projects_pins::review_ignore_globs_for_repo(id.repo_root, id.remote_repo);
    if ignore.is_empty() {
        raw
    } else {
        er_engine::git::filter_raw_diff_exclude_globs(&raw, &ignore)
    }
}

/// Writes the prepared diff, the change facts and `REVIEW.md` into `er_dir`.
pub fn review_inputs(
    ports: &Ports,
    content: &TabContent,
    config: &ErConfig,
    er_dir: &str,
    raw: &str,
) -> Result<String, ErError> {
    let rules = crate::flow_view::review_rules(content, config);
    let source = er_engine::review_flow_driver::review_md_source(ports, &content.tab.identity());
    let review_md = er_engine::ai::review_md::collect(ports, &source, raw);
    er_engine::ai::prepared_diff::ensure_review_inputs(
        er_dir,
        raw,
        rules.as_rules(),
        review_md.as_deref(),
    )
    .map_err(refused)
}

/// The git scope a review of `scope` reads on a tab showing `mode`.
pub fn resolve_review_scope_in(scope: &str, mode: DiffMode) -> Result<String, String> {
    let resolved = if scope == "current" {
        match mode {
            DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => mode.git_mode().to_string(),
            // Guide (tour) reviews the branch diff it was built from.
            DiffMode::PrDiff | DiffMode::Tour => "branch".to_string(),
            _ => {
                return Err(format!(
                    "AI review not available in {} view — switch to All changes, PR Diff, Unstaged, or Staged",
                    mode.git_mode()
                ));
            }
        }
    } else if scope == "pr" {
        "branch".to_string()
    } else if matches!(scope, "branch" | "unstaged" | "staged") {
        if matches!(scope, "unstaged" | "staged") && mode == DiffMode::PrDiff {
            return Err(
                "Unstaged/Staged review not available in PR Diff — switch to Unstaged or Staged view"
                    .to_string(),
            );
        }
        scope.to_string()
    } else {
        return Err(format!("Invalid review scope: {scope}"));
    };
    Ok(resolved)
}

/// From the loaded sidecar, else read from `comments_dir`.
fn github_comment_count(
    loaded: Option<&er_engine::ai::ErGithubComments>,
    comments_dir: &str,
) -> usize {
    if let Some(gc) = loaded {
        return er_engine::ai::count_eligible_github_comments(gc);
    }
    let store = er_engine::store_registry::store();
    if let Ok(scope) = er_store::Scope::dir(comments_dir) {
        if let Ok(er_store::Loaded::Found(stored)) =
            er_engine::ai::annotation_store::load_github_comments(&store, &scope)
        {
            return er_engine::ai::count_eligible_github_comments(&stored.value);
        }
    }
    0
}
