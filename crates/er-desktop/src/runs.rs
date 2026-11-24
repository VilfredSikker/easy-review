//! The run commands on the flag path: review lenses, the tour, a diagram,
//! the importance ranking and scoped reviews. Each names a tab, or takes the
//! registry's active one, and is planned from that tab's published content
//! and the settings view onto the task host, so no run waits on the App.
//! er-service plans the runs every front end has; the diagram, the scoped
//! review and a note sent to the agent are planned here, since only the
//! desktop runs them (ADR 0074).

use er_engine::ai::{prompts, ReviewerKind, ScopedRunStamp};
use er_engine::task_target::BackgroundTaskTarget;
use er_protocol::ErError;
use er_service::runs::{admit, filtered, review_scope, target, RunTab};
use er_service::{Launch, TabContent};

use crate::state::DesktopState;
use crate::task_hooks::{self as tasks, refused};

/// Runs the general review lens on `scope`.
pub fn review(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    scope: String,
) -> Result<(), ErError> {
    on(desktop, wire, |run| er_service::runs::review(run, &scope))
}

/// Generates the guided tour into the tour bucket of the view the tab shows.
pub fn tour(desktop: &DesktopState, wire: Option<er_protocol::TabKey>) -> Result<(), ErError> {
    on(desktop, wire, er_service::runs::tour)
}

/// Has the agent propose the repo's importance rules.
pub fn importance(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
) -> Result<(), ErError> {
    on(desktop, wire, er_service::runs::importance)
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
    on(desktop, wire, |run| {
        er_service::runs::expert(run, &scope, &expert_id)
    })
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
/// `scope`.
pub fn validate(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    scope: String,
) -> Result<(), ErError> {
    on(desktop, wire, |run| er_service::runs::validate(run, &scope))
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

/// Starts tab command `name` through the agent launcher, held on the task
/// host.
fn agent(
    desktop: &DesktopState,
    content: &TabContent,
    target: BackgroundTaskTarget,
    name: &str,
    prompt: &str,
) -> Result<(), ErError> {
    let hooks = tasks::hooks(desktop);
    let run = RunTab {
        host: &desktop.tasks,
        hooks: &hooks,
        content,
    };
    er_service::runs::agent(&run, target, name, prompt)
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
    let hooks = tasks::hooks(desktop);
    er_service::runs::spawn_reviewer(
        &desktop.tasks,
        &hooks,
        reviewer,
        target,
        prompt,
        stamp,
        quiet,
    )
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
    er_service::runs::launch(config, kind, name, target, prompt, scoped_stamp)
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

/// Writes the prepared diff, the change facts and the base branch's
/// `REVIEW.md` files into `er_dir`; answers the diff's hash.
fn review_inputs(
    desktop: &DesktopState,
    content: &TabContent,
    er_dir: &str,
    raw: &str,
) -> Result<String, ErError> {
    let config = crate::arena_service::config(desktop);
    er_service::runs::review_inputs(&desktop.ports, content, &config, er_dir, raw)
}

/// Runs `plan` on the tab `wire` names, else the registry's active one, on
/// the desktop's task host.
fn on(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
    plan: impl FnOnce(&RunTab<'_>) -> Result<(), ErError>,
) -> Result<(), ErError> {
    let tab = crate::arena_service::tab(desktop, wire)?;
    let hooks = tasks::hooks(desktop);
    plan(&RunTab {
        host: &desktop.tasks,
        hooks: &hooks,
        content: &tab.content,
    })
}
