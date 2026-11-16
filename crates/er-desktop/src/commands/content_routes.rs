//! The flag-on routes for a tab's content: open a tab, refresh its diff,
//! switch its mode, select or parse its files, mark them reviewed, close it.
//!
//! Each route takes its key from the app registry's active tab and its context
//! from that tab's published content (ADR 0071), so none waits on the App
//! lock. The flag-on desktop still renders the App's last state until the
//! snapshot reads the content (D21), so each answers with an empty snapshot.

use std::sync::Arc;

use er_engine::git::DiffFile;
use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_service::{AppCommand, Command, OpenTarget, Reply, TabContent, TabContext};

use super::{projects, AppSnapshot, AppState};
use crate::service::actor_key;

/// The app registry's active tab.
struct Active {
    key: TabKey,
    tab: TabDescriptor,
}

fn active(state: &AppState) -> Result<Active, String> {
    let view = state.service.app_registry().tabs_view();
    let entry = view.active_entry().ok_or("No tab is open")?;
    Ok(Active {
        key: entry.key,
        tab: entry.descriptor.clone(),
    })
}

fn unexpected(reply: &Reply) -> String {
    format!("The workspace answered with an unexpected reply: {reply:?}")
}

/// Sends `command` to the actor that holds `tab`'s content.
fn send(state: &AppState, tab: &TabDescriptor, command: Command) -> Result<Reply, String> {
    state
        .service
        .send(&actor_key(tab), command)
        .map_err(|error| error.to_string())
}

/// Answers how many reviewed marks a load or refresh cleared.
fn cleared(reply: Reply) -> Result<usize, String> {
    match reply {
        Reply::ContentLoaded {
            reviewed_cleared, ..
        } => Ok(reviewed_cleared),
        other => Err(unexpected(&other)),
    }
}

fn load(state: &AppState, key: TabKey, tab: &TabDescriptor) -> Result<usize, String> {
    let tab = tab.clone();
    let command = Command::LoadTab {
        key,
        tab: tab.clone(),
        mode: None,
    };
    cleared(send(state, &tab, command)?)
}

/// The active tab and its content, loaded on its first use.
fn active_content(state: &AppState) -> Result<(Active, Arc<TabContent>), String> {
    let active = active(state)?;
    if let Some(content) = state.service.content(active.key, &active.tab) {
        return Ok((active, content));
    }
    load(state, active.key, &active.tab)?;
    let content = state
        .service
        .content(active.key, &active.tab)
        .ok_or("The tab's content did not load")?;
    Ok((active, content))
}

/// Opens `tab` in the registry, then loads its content under the key the
/// registry gave it.
pub(super) fn open(state: &AppState, tab: TabDescriptor) -> Result<AppSnapshot, String> {
    let reply = state
        .service
        .send_app(AppCommand::NewTab { tab: tab.clone() })
        .map_err(|error| error.to_string())?;
    let Reply::Tabs(view) = reply else {
        return Err(unexpected(&reply));
    };
    let key = view.active.ok_or("The new tab did not become active")?;
    load(state, key, &tab)?;
    Ok(AppSnapshot::default())
}

/// A plain tab on `repo_root`.
pub(super) fn working_tab(repo_root: String) -> TabDescriptor {
    tab_of(TabKind::Working, repo_root)
}

/// A branch view of `branch` in the project's checkout.
pub(super) fn branch_tab(project_id: &str, branch: String) -> Result<TabDescriptor, String> {
    let file = projects::load();
    let project = file
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| format!("Project not found: {project_id}"))?;
    Ok(TabDescriptor {
        branch: Some(branch),
        ..tab_of(TabKind::LocalBranch, project.root_path.clone())
    })
}

/// The tab a PR open shows: a local PR review when the caller resolved a
/// project, a remote PR otherwise.
pub(super) fn pr_tab(target: &OpenTarget) -> Result<TabDescriptor, String> {
    let (owner, repo, number) = match target {
        OpenTarget::PrUrl { url, .. } => {
            let pr = er_engine::github::parse_github_pr_url(url)
                .ok_or_else(|| format!("Not a valid GitHub PR URL: {url}"))?;
            (pr.owner, pr.repo, pr.number)
        }
        OpenTarget::RemotePr {
            owner,
            repo,
            number,
            ..
        } => (owner.clone(), repo.clone(), *number),
        OpenTarget::PrReview {
            remote, pr_number, ..
        } => {
            let (owner, repo) = remote.split_once('/').unwrap_or_default();
            (owner.to_string(), repo.to_string(), *pr_number)
        }
    };
    let root = target.repo_root().to_string();
    let kind = if root.is_empty() {
        TabKind::RemotePr
    } else {
        TabKind::LocalPr
    };
    let remote = !owner.is_empty() && !repo.is_empty();
    Ok(TabDescriptor {
        pr_owner: remote.then_some(owner),
        pr_repo: remote.then_some(repo),
        pr_number: Some(number),
        ..tab_of(kind, root)
    })
}

fn tab_of(kind: TabKind, repo_root: String) -> TabDescriptor {
    TabDescriptor {
        kind,
        repo_root,
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref: None,
        extra: serde_json::Map::new(),
    }
}

/// Closes the tab at `idx` and drops its content. The descriptor is read
/// first, since the registry forgets it on close; an actor that is not up
/// holds no content and is not started for the drop.
pub(super) fn close(state: &AppState, idx: usize) -> Result<AppSnapshot, String> {
    let view = state.service.app_registry().tabs_view();
    let entry = view
        .tabs
        .get(idx)
        .cloned()
        .ok_or_else(|| format!("no tab at position {idx}"))?;
    state
        .service
        .send_app(AppCommand::CloseTab { key: entry.key })
        .map_err(|error| error.to_string())?;
    let drop = Command::DropTab { key: entry.key };
    if let Some(dropped) = state
        .service
        .send_if_started(&actor_key(&entry.descriptor), drop)
    {
        dropped.map_err(|error| error.to_string())?;
    }
    Ok(AppSnapshot::default())
}

/// Refreshes the active tab's diff, or loads it if it never loaded. Answers
/// how many reviewed marks the refresh cleared.
pub(super) fn refresh(state: &AppState, force: bool) -> Result<usize, String> {
    let active = active(state)?;
    if state.service.content(active.key, &active.tab).is_none() {
        return load(state, active.key, &active.tab);
    }
    let key = active.key;
    let command = Command::RefreshDiff {
        key,
        force,
        adopt_head_oid: None,
    };
    cleared(send(state, &active.tab, command)?)
}

/// The modes the workspace loads. The others stay on the App until the
/// snapshot reads the content.
pub(super) fn routed_mode(mode: &str) -> Option<DiffMode> {
    match mode {
        "branch" => Some(DiffMode::Branch),
        "unstaged" => Some(DiffMode::Unstaged),
        "staged" => Some(DiffMode::Staged),
        "pr" | "pr_diff" => Some(DiffMode::PrDiff),
        "tour" => Some(DiffMode::Tour),
        _ => None,
    }
}

/// The views the App still shows, which the workspace records on the
/// active tab's content without loading anything.
pub(super) fn recorded_mode(mode: &str) -> Option<DiffMode> {
    match mode {
        "history" => Some(DiffMode::History),
        "conflicts" => Some(DiffMode::Conflicts),
        "hidden" => Some(DiffMode::Hidden),
        _ => None,
    }
}

/// Records `mode` on the active tab's content, when it is loaded, under the
/// same feature gate and painted-tab check as a routed switch. The App path
/// runs after it either way, so a failure here is only logged.
pub(super) fn record_mode(state: &AppState, name: &str, mode: DiffMode, tab_idx: Option<usize>) {
    let features = state.service.settings().view().config.features.clone();
    if !super::feature_allows_mode_str(&features, name) {
        return;
    }
    let view = state.service.app_registry().tabs_view();
    if tab_idx.is_some_and(|idx| view.key_at(idx) != view.active) {
        return;
    }
    let Ok(active) = active(state) else {
        return;
    };
    let loaded = state.service.content(active.key, &active.tab);
    if loaded.is_none_or(|content| content.tab.mode == mode) {
        return;
    }
    let key = active.key;
    let command = Command::SetDiffMode {
        key,
        mode,
        pr_number: None,
    };
    if let Err(error) = send(state, &active.tab, command) {
        log::warn!("er-desktop: recording the {mode:?} view failed: {error}");
    }
}

/// Asks the active tab's actor to read its sidecars again, without waiting
/// for the answer and without starting an actor or loading a tab. The poll
/// tick calls it, so a sidecar an outside writer moved reaches the content.
pub(super) fn reload_sidecars(state: &AppState) -> bool {
    let Ok(active) = active(state) else {
        return false;
    };
    if state.service.content(active.key, &active.tab).is_none() {
        return false;
    }
    let key = active.key;
    state
        .service
        .post_if_started(&actor_key(&active.tab), Command::ReloadSidecars { key })
}

/// Shows the active tab in `mode`. A click painted on a tab that is no longer
/// active changes nothing, as on the App path. The feature gate reads the
/// settings actor's config. `pr_number` is the PR the header detected, for a
/// tab that has none.
pub(super) fn set_mode(
    state: &AppState,
    mode: &str,
    diff_mode: DiffMode,
    pr_number: Option<u64>,
    tab_idx: Option<usize>,
) -> Result<AppSnapshot, String> {
    let features = state.service.settings().view().config.features.clone();
    if !super::feature_allows_mode_str(&features, mode) {
        return Err(format!("'{mode}' view is disabled in settings"));
    }
    if let Some(idx) = tab_idx {
        let view = state.service.app_registry().tabs_view();
        if view.key_at(idx) != view.active {
            return Ok(AppSnapshot::default());
        }
    }
    let (active, content) = active_content(state)?;
    if content.tab.mode != diff_mode {
        let key = active.key;
        let command = Command::SetDiffMode {
            key,
            mode: diff_mode,
            pr_number,
        };
        send(state, &active.tab, command)?;
    }
    Ok(AppSnapshot::default())
}

/// Selects the file at `idx` in the active tab's view state (D20) and parses
/// it when it is a lazy stub.
pub(super) fn select_file(state: &AppState, idx: usize) -> Result<AppSnapshot, String> {
    let (active, content) = active_content(state)?;
    if idx >= content.diff.files.len() {
        return Ok(AppSnapshot::default());
    }
    {
        let mut view = state.view.lock().map_err(|error| error.to_string())?;
        let nav = view.nav_mut(active.key);
        nav.selected_file = idx;
        nav.current_hunk = 0;
        nav.current_line = None;
        nav.diff_scroll = 0;
        nav.h_scroll = 0;
    }
    parse(state, &active, &content, &[idx])?;
    Ok(AppSnapshot::default())
}

/// Parses the named lazy stubs of the active tab. The parsed files reach the
/// page once the snapshot reads the content.
pub(super) fn request_file_content(state: &AppState, indices: &[usize]) -> Result<(), String> {
    let (active, content) = active_content(state)?;
    parse(state, &active, &content, indices)
}

/// Sends a parse for the stubs among `indices` only: a parse publishes a new
/// diff section, so a click on a parsed file must not send one.
fn parse(
    state: &AppState,
    active: &Active,
    content: &TabContent,
    indices: &[usize],
) -> Result<(), String> {
    let lazy = content.diff.meta.lazy;
    let paths: Vec<String> = indices
        .iter()
        .filter_map(|&idx| content.diff.files.get(idx))
        .filter(|file| lazy && is_stub(file))
        .map(|file| file.path.clone())
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    let key = active.key;
    let expand = false;
    send(state, &active.tab, Command::ParseFiles { key, paths, expand })?;
    Ok(())
}

/// A file a lazy diff left unparsed. A compacted file stays a stub until the
/// user expands it.
fn is_stub(file: &DiffFile) -> bool {
    file.hunks.is_empty() && !file.compacted
}

/// Sends a reviewed write with the context the active tab's content carries.
pub(super) fn write_reviewed(
    state: &AppState,
    build: impl FnOnce(TabContext) -> Command,
) -> Result<AppSnapshot, String> {
    let (active, content) = active_content(state)?;
    send(state, &active.tab, build(content.context.clone()))?;
    Ok(AppSnapshot::default())
}
