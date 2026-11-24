//! The run commands on the flag path: review lenses, the tour, a diagram,
//! the importance ranking and scoped reviews. Each names a tab, or takes the
//! registry's active one, reads its inputs from that tab's published content
//! and the settings view, and queues its agent on the desktop's task host,
//! so no run waits on the App. The bucket a review flow owns is refused, as
//! on the App (`check_bucket_admission`).

use er_engine::ai::{prompts, ReviewerKind, ScopedRunStamp};
use er_engine::task_target::BackgroundTaskTarget;
use er_protocol::ErError;
use er_service::TabContent;

use crate::state::DesktopState;
use crate::task_hooks::{self as tasks, refused};
use er_service::Launch;

/// Runs the general review lens on `scope`.
pub fn review(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    scope: String,
) -> Result<(), ErError> {
    let tab = crate::arena_service::tab(desktop, wire)?;
    let content = &tab.content;
    let scope = review_scope(content, &scope)?;
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    let raw = filtered(review_raw(desktop, content, &scope)?, content);
    if raw.trim().is_empty() {
        return Err(refused("Nothing to review"));
    }
    std::fs::create_dir_all(&er_dir)
        .map_err(|e| refused(format!("Failed to create branch managed directory: {e}")))?;
    let diff_hash = review_inputs(desktop, content, &er_dir, &raw)?;
    let target = target(content, er_dir, scope);
    let prompt = prompts::build_review_prompt_prepared_diff(
        &target.scope,
        &target.er_dir,
        &target.base_branch,
        &target.branch_label,
        &diff_hash,
        None,
    );
    queue(desktop, "review", "review", target, prompt, None)
}

/// Generates the guided tour into the tour bucket of the view the tab shows.
pub fn tour(desktop: &DesktopState, wire: Option<er_protocol::TabKey>) -> Result<(), ErError> {
    let tab = crate::arena_service::tab(desktop, wire)?;
    let content = &tab.content;
    let id = content.tab.identity();
    let er_dir = id
        .tour_bucket_er_dir(&*desktop.ports.git)
        .unwrap_or_else(|| content.context.er_dir.clone());
    // No admission check: a tour prepares its own diff and writes only
    // `tour.json`, so it can run beside a review flow.
    let diff_dir = er_engine::ai::prepared_diff::tour_input_dir(&er_dir);
    std::fs::create_dir_all(&diff_dir)
        .map_err(|e| refused(format!("Failed to create tour managed directory: {e}")))?;
    let raw = filtered(review_raw(desktop, content, "branch")?, content);
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
    queue(desktop, "tour", "tour", target, prompt, None)
}

/// Has the agent propose the repo's importance rules. The prompt names the
/// repo, not a diff.
pub fn importance(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
) -> Result<(), ErError> {
    let tab = crate::arena_service::tab(desktop, wire)?;
    let content = &tab.content;
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
        desktop,
        prompts::IMPORTANCE_TASK_KIND,
        "importance",
        target,
        prompt,
        None,
    )
}

/// Generates one diagram; the host writes the agent's JSON into the
/// bucket's `diagrams/`.
pub fn diagram(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    kind: String,
    custom_prompt: Option<String>,
) -> Result<(), ErError> {
    if !er_engine::ai::is_valid_diagram_kind(&kind) {
        return Err(refused(format!("Unknown diagram kind: {kind}")));
    }
    let custom_prompt = custom_prompt
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    if kind == er_engine::ai::DIAGRAM_KIND_CUSTOM && custom_prompt.is_none() {
        return Err(refused("A custom diagram needs a prompt"));
    }
    let tab = crate::arena_service::tab(desktop, wire)?;
    let content = &tab.content;
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    std::fs::create_dir_all(format!("{er_dir}/diagrams"))
        .map_err(|e| refused(format!("Failed to create diagrams directory: {e}")))?;
    let raw = filtered(review_raw(desktop, content, "branch")?, content);
    if raw.trim().is_empty() {
        return Err(refused("Nothing to diagram"));
    }
    let diff_hash =
        er_engine::ai::prepared_diff::ensure_diff_artifacts(&er_dir, &raw).map_err(refused)?;
    let output_file = er_engine::ai::diagram_output_file(&kind);
    let scope_label = if content.tab.identity().tour_context_is_pr() {
        "PR diff"
    } else {
        "branch diff"
    };
    let prompt = prompts::build_diagram_prompt_prepared_diff(
        scope_label,
        &er_dir,
        &output_file,
        &diff_hash,
        &kind,
        custom_prompt.as_deref(),
    );
    let output_path = er_engine::ai::diagram_sidecar_path(&er_dir, &output_file)
        .ok_or_else(|| refused(format!("Invalid diagram output file: {output_file}")))?;
    let host_write = er_engine::stage_job::HostWriteDiagram {
        output_path,
        kind: kind.clone(),
        diff_hash,
        custom_prompt,
    };
    let mut launch = launch(
        desktop,
        &er_engine::ai::diagram_task_kind(&kind),
        "diagram",
        target(content, er_dir, "branch".to_string()),
        prompt,
        None,
    );
    launch.host_write_diagram = Some(host_write);
    tasks::enqueue(desktop, launch).map(|_| ()).map_err(refused)
}

/// Runs one expert lens on `scope`.
pub fn expert(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    scope: String,
    expert_id: String,
) -> Result<(), ErError> {
    if er_engine::ai::expert_by_id(&expert_id).is_none() {
        return Err(refused(format!("Unknown expert: {expert_id}")));
    }
    let tab = crate::arena_service::tab(desktop, wire)?;
    let content = &tab.content;
    let scope = review_scope(content, &scope)?;
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    std::fs::create_dir_all(&er_dir)
        .map_err(|e| refused(format!("Failed to create branch managed directory: {e}")))?;
    let raw = review_raw(desktop, content, &scope)?;
    if raw.trim().is_empty() {
        return Err(refused("Nothing to review"));
    }
    let raw = filtered(raw, content);
    let diff_hash = review_inputs(desktop, content, &er_dir, &raw)?;
    let prompt = prompts::build_expert_review_prompt_prepared_diff(
        &scope, &er_dir, &expert_id, &diff_hash, None,
    );
    let reviewer = ReviewerKind::Expert(expert_id);
    spawn_reviewer(
        desktop,
        &reviewer,
        target(content, er_dir, scope),
        prompt,
        None,
        false,
    )
    .map_err(refused)
}

/// Runs `reviewers` over `paths`, or over the whole diff when `paths` is
/// empty, and tells the person who started.
pub fn scoped(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    scope: String,
    paths: Vec<String>,
    reviewers: Vec<String>,
    focus_prompt: Option<String>,
) -> Result<(), ErError> {
    if reviewers.is_empty() {
        return Err(refused("No reviewers selected"));
    }
    let tab = crate::arena_service::tab(desktop, wire)?;
    let content = &tab.content;
    let scope = review_scope(content, &scope)?;
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    std::fs::create_dir_all(&er_dir)
        .map_err(|e| refused(format!("Failed to create branch managed directory: {e}")))?;
    let raw = review_raw(desktop, content, &scope)?;
    if raw.trim().is_empty() {
        return Err(refused("Nothing to review"));
    }
    let id = content.tab.identity();
    let (scoped_files, scope_paths, diff_body) =
        crate::route_helpers::scoped_diff(&er_dir, raw, paths, id.repo_root, id.remote_repo)
            .map_err(refused)?;
    let diff_hash = review_inputs(desktop, content, &er_dir, &diff_body)?;
    let stamps = if scoped_files {
        crate::route_helpers::scope_stamps(&er_dir, &scope_paths, &reviewers)
    } else {
        Default::default()
    };
    let (started, skipped) = crate::route_helpers::spawn_scoped_reviewers(
        &mut |reviewer, target, prompt, stamp| {
            // The run's own message below is its one notice, as on the App.
            spawn_reviewer(desktop, reviewer, target, prompt, stamp, true)
        },
        target(content, er_dir, scope),
        &reviewers,
        focus_prompt.as_deref(),
        &diff_hash,
        &stamps,
    )
    .map_err(refused)?;
    if started.is_empty() && !skipped.is_empty() {
        return Err(refused(format!(
            "All selected reviewers already running: {}",
            skipped.join(", ")
        )));
    }
    let message = crate::route_helpers::scoped_message(
        &started,
        &skipped,
        scoped_files.then_some(scope_paths.len()),
    );
    crate::resources::notice::tell(desktop, message, false);
    Ok(())
}

/// Validates the review's findings and the eligible GitHub comments on
/// `scope`, each through the agent launcher as the App's tab commands run.
/// The comments are the tab's own: its actor relocated them at its last load.
pub fn validate(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    scope: String,
) -> Result<(), ErError> {
    let tab = crate::arena_service::tab(desktop, wire)?;
    let content = &tab.content;
    let scope = review_scope(content, &scope)?;
    if content.context.is_remote {
        return Err(refused(
            "Validate review is local-only. Check out the PR locally first.",
        ));
    }
    let er_dir = content.context.er_dir.clone();
    admit(&er_dir)?;
    let has_review = er_engine::ai::review_file_exists(&er_dir);
    let comment_count = crate::route_helpers::github_comment_count(
        content.ai.state.github_comments.as_ref(),
        &content.context.comments_dir,
    );
    if !has_review && comment_count == 0 {
        return Err(refused(
            "Nothing to validate. Run AI review or add GitHub comments first.",
        ));
    }
    let raw = review_raw(desktop, content, &scope)?;
    if raw.trim().is_empty() {
        return Err(refused("Nothing to validate"));
    }
    let diff_hash =
        er_engine::ai::prepared_diff::ensure_diff_artifacts(&er_dir, &raw).map_err(refused)?;
    let source =
        er_engine::review_flow_driver::review_md_source(&desktop.ports, &content.tab.identity());
    let review_md = er_engine::ai::review_md::collect(&desktop.ports, &source, &raw);
    er_engine::ai::review_md::write_review_md(&er_dir, review_md.as_deref()).map_err(refused)?;
    let target = target(content, er_dir.clone(), scope.clone());
    if has_review {
        let prompt = prompts::build_validate_prompt_prepared_diff(&scope, &er_dir, &diff_hash);
        agent(desktop, content, target.clone(), "validate", &prompt)?;
    }
    if comment_count > 0 {
        let prompt =
            prompts::build_validate_github_comments_prompt_prepared_diff(&er_dir, &diff_hash);
        agent(desktop, content, target, "validate-comments", &prompt)?;
    }
    let message = match (has_review, comment_count) {
        (true, n) if n > 0 => format!("Validation started (review + {n} comments)"),
        (true, _) => "Validation started (review)".to_string(),
        (false, n) => format!("Validation started ({n} comments)"),
    };
    crate::resources::notice::tell(desktop, message, false);
    Ok(())
}

/// Hands note `note_id` and its replies to the agent to carry out in the
/// tab's working tree (ADR 0031), the note read from the tab's AI section.
pub fn send_note(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    note_id: String,
) -> Result<(), ErError> {
    if !note_id.starts_with("n-") {
        return Err(refused("Send to agent works on notes only"));
    }
    let tab = crate::arena_service::tab(desktop, wire)?;
    let content = &tab.content;
    let resolved = &content.tab;
    // A plain local tab is the main checkout, as `local_checkout_root` says
    // on the App, even on a detached HEAD whose branch resolves no checkout.
    // A PR or branch view is one only where its branch is checked out.
    let checked_out = if resolved.pr_number.is_some() || resolved.local_branch_view.is_some() {
        resolved.checkout_root.as_deref()
    } else {
        Some(resolved.repo_root.as_str())
    };
    crate::route_helpers::send_note_blocked(
        content.context.is_remote,
        checked_out,
        &resolved.repo_root,
    )
    .map_err(refused)?;
    let name = crate::route_helpers::SEND_NOTE_COMMAND;
    let er_dir = content.context.er_dir.clone();
    let running = desktop
        .tasks
        .rows()
        .iter()
        .any(|row| row.kind == name && row.er_dir == er_dir && row.status == "running");
    if running {
        return Err(refused(
            "An agent is already working on a note in this tab. Wait for it to finish.",
        ));
    }
    let (prompt, toast) = crate::route_helpers::note_prompt(
        content.ai.state.notes.as_ref(),
        &note_id,
        &resolved.repo_root,
        &resolved.base_branch,
        &resolved.current_branch,
    )
    .map_err(refused)?;
    let target = target(content, er_dir, content.context.scope.clone());
    agent(desktop, content, target, name, &prompt)?;
    crate::resources::notice::tell(desktop, toast, false);
    Ok(())
}

/// Starts tab command `name` through the agent launcher on the settings
/// view's config and saved selection (A18), held on the task host.
fn agent(
    desktop: &DesktopState,
    content: &TabContent,
    target: BackgroundTaskTarget,
    name: &str,
    prompt: &str,
) -> Result<(), ErError> {
    let config = crate::arena_service::config(desktop);
    let (provider_id, model_id, effort) = crate::ai_ask::selection(&config);
    let (repo_root, er_dir) = (target.repo_root.clone(), target.er_dir.clone());
    tasks::hold_agent(desktop, target, name, |log_tx| {
        er_engine::agent_prompt::launch_agent_prompt(er_engine::agent_prompt::AgentPromptRequest {
            name,
            prompt,
            repo_root,
            er_dir,
            is_remote: content.context.is_remote,
            remote_repo: content.tab.remote_repo.clone(),
            config: &config,
            selection: er_engine::config::AiSelection {
                provider_id,
                model_id,
                effort,
            },
            ports: &desktop.ports,
            log_tx,
        })
    })
    .map(|_| ())
    .map_err(refused)
}

/// Queues one finding lens, under the task kind and command name the App
/// gives it; `quiet` leaves its start for the caller to tell.
pub fn spawn_reviewer(
    desktop: &DesktopState,
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
        ..launch(desktop, &kind, &name, target, prompt, stamp)
    };
    tasks::enqueue(desktop, launch).map(|_| ())
}

fn queue(
    desktop: &DesktopState,
    kind: &str,
    name: &str,
    target: BackgroundTaskTarget,
    prompt: String,
    stamp: Option<ScopedRunStamp>,
) -> Result<(), ErError> {
    let launch = launch(desktop, kind, name, target, prompt, stamp);
    tasks::enqueue(desktop, launch).map(|_| ()).map_err(refused)
}

/// A launch on the settings view's config and saved selection (A18).
pub fn launch(
    desktop: &DesktopState,
    kind: &str,
    name: &str,
    target: BackgroundTaskTarget,
    prompt: String,
    scoped_stamp: Option<ScopedRunStamp>,
) -> Launch {
    let config = crate::arena_service::config(desktop);
    let (provider_id, model_id, effort) = crate::ai_ask::selection(&config);
    Launch {
        kind: kind.to_string(),
        command_name: name.to_string(),
        target,
        prompt,
        prepared_diff: true,
        host_write_diagram: None,
        scoped_stamp,
        config,
        selection: er_engine::config::AiSelection {
            provider_id,
            model_id,
            effort,
        },
        quiet: false,
    }
}

/// The target a run on `content`'s tab files under.
fn target(content: &TabContent, er_dir: String, scope: String) -> BackgroundTaskTarget {
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

fn review_scope(content: &TabContent, scope: &str) -> Result<String, ErError> {
    crate::route_helpers::resolve_review_scope_in(scope, content.tab.mode).map_err(refused)
}

fn admit(er_dir: &str) -> Result<(), ErError> {
    er_engine::arena::check_bucket_admission(er_dir).map_err(|e| refused(e.to_string()))
}

/// The diff a review of `scope` reads, by the rule the App reads its own
/// tab's by.
fn review_raw(
    desktop: &DesktopState,
    content: &TabContent,
    scope: &str,
) -> Result<String, ErError> {
    er_service::review_raw_diff(&desktop.ports, content, scope).map_err(|e| refused(e.to_string()))
}

/// `raw` without the files the project's review-ignore globs name.
fn filtered(raw: String, content: &TabContent) -> String {
    let id = content.tab.identity();
    let ignore =
        er_engine::projects_pins::review_ignore_globs_for_repo(id.repo_root, id.remote_repo);
    if ignore.is_empty() {
        raw
    } else {
        er_engine::git::filter_raw_diff_exclude_globs(&raw, &ignore)
    }
}

/// Writes the prepared diff, the change facts and the base branch's
/// `REVIEW.md` files into `er_dir`; answers the diff's hash.
fn review_inputs(
    desktop: &DesktopState,
    content: &TabContent,
    er_dir: &str,
    raw: &str,
) -> Result<String, ErError> {
    let config = crate::arena_service::config(desktop);
    let rules = er_service::review_rules(content, &config);
    let source =
        er_engine::review_flow_driver::review_md_source(&desktop.ports, &content.tab.identity());
    let review_md = er_engine::ai::review_md::collect(&desktop.ports, &source, raw);
    er_engine::ai::prepared_diff::ensure_review_inputs(
        er_dir,
        raw,
        rules.as_rules(),
        review_md.as_deref(),
    )
    .map_err(refused)
}
