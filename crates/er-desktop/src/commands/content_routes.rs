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
use er_service::{
    AppCommand, Command, OpenTarget, Reply, TabContent, TabContext, TabEntry, TabsView,
};

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
/// registry gave it. With `replace` it takes the active tab's place, whose
/// content is dropped. A PR that is already open is selected instead, as the
/// flag-off open does.
pub(super) fn open(
    state: &AppState,
    tab: TabDescriptor,
    replace: bool,
) -> Result<AppSnapshot, String> {
    let current = state.service.app_registry().tabs_view();
    if let Some(open) = current
        .tabs
        .iter()
        .find(|entry| same_pr(&entry.descriptor, &tab))
    {
        app_command(state, AppCommand::SelectTab { key: open.key })?;
        if state.service.content(open.key, &open.descriptor).is_none() {
            load(state, open.key, &open.descriptor)?;
        }
        return Ok(AppSnapshot::default());
    }
    let replaced = current.active_entry().filter(|_| replace).cloned();
    let command = match &replaced {
        Some(old) => AppCommand::ReplaceTab {
            key: old.key,
            tab: tab.clone(),
        },
        None => AppCommand::NewTab { tab: tab.clone() },
    };
    let view = app_command(state, command)?;
    if let Some(old) = replaced {
        drop_content(state, &old)?;
    }
    let key = view.active.ok_or("The new tab did not become active")?;
    load(state, key, &tab)?;
    Ok(AppSnapshot::default())
}

/// Two descriptors of one PR tab: the same kind, root and PR.
fn same_pr(open: &TabDescriptor, tab: &TabDescriptor) -> bool {
    tab.pr_number.is_some()
        && open.kind == tab.kind
        && open.pr_number == tab.pr_number
        && open.workspace_key() == tab.workspace_key()
}

fn app_command(state: &AppState, command: AppCommand) -> Result<TabsView, String> {
    let reply = state
        .service
        .send_app(command)
        .map_err(|error| error.to_string())?;
    match reply {
        Reply::Tabs(view) => Ok(view),
        other => Err(unexpected(&other)),
    }
}

/// Drops a closed or replaced tab's content on the actor that loaded it. An
/// actor that is not up holds no content and is not started for the drop.
fn drop_content(state: &AppState, entry: &TabEntry) -> Result<(), String> {
    let drop = Command::DropTab { key: entry.key };
    if let Some(dropped) = state
        .service
        .send_if_started(&actor_key(&entry.descriptor), drop)
    {
        dropped.map_err(|error| error.to_string())?;
    }
    Ok(())
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
    app_command(state, AppCommand::CloseTab { key: entry.key })?;
    drop_content(state, &entry)?;
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
    cleared(send(
        state,
        &active.tab,
        Command::RefreshDiff { key, force },
    )?)
}

/// The modes the workspace loads. The others stay on the App until the
/// snapshot reads the content.
pub(super) fn routed_mode(mode: &str) -> Option<DiffMode> {
    match mode {
        "branch" => Some(DiffMode::Branch),
        "unstaged" => Some(DiffMode::Unstaged),
        "staged" => Some(DiffMode::Staged),
        "pr" | "pr_diff" => Some(DiffMode::PrDiff),
        _ => None,
    }
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
    send(state, &active.tab, Command::ParseFiles { key, paths })?;
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
    annotate(state, None, build)?;
    Ok(AppSnapshot::default())
}

/// Sends an annotation command built from the active tab's published
/// context, loading the content first when it never loaded. Answers the key
/// it acted on.
///
/// `expected` is the App tab the caller read under the App lock, when the
/// command carries something taken from it: a painted view, or a document's
/// text. The App follows the registry on another thread, so while it lags
/// the registry's active tab is another one, and the command is refused
/// rather than written to that tab.
pub(super) fn annotate(
    state: &AppState,
    expected: Option<TabKey>,
    build: impl FnOnce(TabContext) -> Command,
) -> Result<TabKey, String> {
    let (active, content) = active_content(state)?;
    if expected.is_some_and(|key| key != active.key) {
        return Err(MOVED.to_string());
    }
    send(state, &active.tab, build(content.context.clone()))?;
    Ok(active.key)
}

/// The refusal for an annotation whose App tab is no longer the registry's
/// active one.
pub(super) const MOVED: &str = "The active view changed; the annotation was not written";
