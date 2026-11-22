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
use crate::state::DesktopState;

/// The app registry's active tab.
struct Active {
    key: TabKey,
    tab: TabDescriptor,
}

fn active(state: &DesktopState) -> Result<Active, String> {
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
fn send(state: &DesktopState, tab: &TabDescriptor, command: Command) -> Result<Reply, String> {
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

/// Loads `key` and waits for the reply. The claim keeps the activation
/// follower from posting a second load once this one is on its way; the
/// route loads either way, since it answers with the result. The follower
/// can still claim first, when the tab change that opened `key` reaches it
/// before this call: the actor then refuses whichever first load lands
/// second, and a refused load whose key now holds content has loaded.
fn load(state: &DesktopState, key: TabKey, tab: &TabDescriptor) -> Result<usize, String> {
    let _ = state.service.claim_load(key);
    let command = Command::LoadTab {
        key,
        tab: tab.clone(),
        mode: None,
    };
    match send(state, tab, command) {
        Ok(reply) => cleared(reply),
        Err(_) if state.service.content(key, tab).is_some() => Ok(0),
        Err(error) => Err(error),
    }
}

/// The active tab and its content, loaded on its first use.
fn active_content(state: &DesktopState) -> Result<(Active, Arc<TabContent>), String> {
    let active = active(state)?;
    let content = content_of(state, &active)?;
    Ok((active, content))
}

/// The tab the registry lists under `key`.
fn listed(state: &DesktopState, key: TabKey) -> Result<Active, String> {
    let view = state.service.app_registry().tabs_view();
    let entry = view
        .tabs
        .iter()
        .find(|entry| entry.key == key)
        .ok_or("no open tab has that key")?;
    Ok(Active {
        key,
        tab: entry.descriptor.clone(),
    })
}

/// `entry`'s content, loaded on its first use.
fn content_of(state: &DesktopState, entry: &Active) -> Result<Arc<TabContent>, String> {
    if let Some(content) = state.service.content(entry.key, &entry.tab) {
        return Ok(content);
    }
    load(state, entry.key, &entry.tab)?;
    state
        .service
        .content(entry.key, &entry.tab)
        .ok_or_else(|| "The tab's content did not load".to_string())
}

/// Opens `tab` in the registry, then loads its content under the key the
/// registry gave it. With `replace` it takes the active tab's place, whose
/// content is dropped. A PR that is already open is selected instead, as the
/// flag-off open does.
pub(crate) fn open(state: &DesktopState, tab: TabDescriptor, replace: bool) -> Result<(), String> {
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
        return Ok(());
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
    Ok(())
}

/// Two descriptors of one PR, whatever the tab kind (A15): the same PR number
/// in the same repo. The repo is `owner/repo` when both carry it, since a
/// local PR's actor is its clone and a remote PR's is the repo; a descriptor
/// without it (a local PR saved by the flag-off build) is matched by its actor.
fn same_pr(open: &TabDescriptor, tab: &TabDescriptor) -> bool {
    fn repo(d: &TabDescriptor) -> Option<(&String, &String)> {
        d.pr_owner.as_ref().zip(d.pr_repo.as_ref())
    }
    tab.pr_number.is_some()
        && open.pr_number == tab.pr_number
        && match (repo(open), repo(tab)) {
            (Some((a_owner, a_repo)), Some((b_owner, b_repo))) => {
                a_owner.eq_ignore_ascii_case(b_owner) && a_repo.eq_ignore_ascii_case(b_repo)
            }
            _ => open.workspace_key() == tab.workspace_key(),
        }
}

pub(crate) fn app_command(state: &DesktopState, command: AppCommand) -> Result<TabsView, String> {
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
fn drop_content(state: &DesktopState, entry: &TabEntry) -> Result<(), String> {
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
pub(crate) fn working_tab(repo_root: String) -> TabDescriptor {
    tab_of(TabKind::Working, repo_root)
}

/// A branch view of `branch` in the project's checkout.
pub(crate) fn branch_tab(project_id: &str, branch: String) -> Result<TabDescriptor, String> {
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
pub(crate) fn pr_tab(target: &OpenTarget) -> Result<TabDescriptor, String> {
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

/// Closes the tab at `idx` and drops its content.
pub(super) fn close(state: &DesktopState, idx: usize) -> Result<AppSnapshot, String> {
    let view = state.service.app_registry().tabs_view();
    let key = view
        .key_at(idx)
        .ok_or_else(|| format!("no tab at position {idx}"))?;
    close_key(state, key)?;
    Ok(AppSnapshot::default())
}

/// Closes the tab `key` names and drops its content. The descriptor is read
/// first, since the registry forgets it on close; an actor that is not up
/// holds no content and is not started for the drop.
pub(crate) fn close_key(state: &DesktopState, key: TabKey) -> Result<TabsView, String> {
    let view = state.service.app_registry().tabs_view();
    let entry = view
        .tabs
        .iter()
        .find(|tab| tab.key == key)
        .cloned()
        .ok_or("no open tab has that key")?;
    let closed = app_command(state, AppCommand::CloseTab { key: entry.key })?;
    drop_content(state, &entry)?;
    Ok(closed)
}

/// Refreshes the active tab's diff, or loads it if it never loaded. Answers
/// how many reviewed marks the refresh cleared.
pub(super) fn refresh(state: &AppState, force: bool) -> Result<usize, String> {
    let active = active(state)?;
    refresh_tab(state, active.key, force)
}

/// Refreshes `key`'s diff, or loads it if it never loaded, which is how a
/// tab whose first load failed tries again. Answers how many reviewed marks
/// the refresh cleared.
pub(crate) fn refresh_tab(state: &DesktopState, key: TabKey, force: bool) -> Result<usize, String> {
    let active = listed(state, key)?;
    if state.service.content(active.key, &active.tab).is_none() {
        return load(state, active.key, &active.tab);
    }
    let command = Command::RefreshDiff {
        key,
        force,
        adopt_head_oid: None,
    };
    cleared(send(state, &active.tab, command)?)
}

/// What a sync leaves to the caller's background refreshes.
pub(crate) struct Synced {
    pub reviewed_cleared: usize,
    /// The synced tab's root, whose branch metadata the caller refreshes.
    pub repo_root: String,
    /// The PR whose GitHub status the caller refreshes. The merge box compares
    /// that status with the synced head, so a stale one blocks the merge.
    pub status_key: Option<(String, String, u64)>,
}

/// Refetches the active tab's diff as a sync does.
pub(super) fn sync(state: &AppState) -> Result<Synced, String> {
    let active = active(state)?;
    sync_tab(state, active.key)
}

/// Refetches `key`'s diff as a sync does, and lines the stale pill's
/// baselines up with what the pill compares: a PR tab adopts the PR list's
/// head, and a branch view records the base it just fetched as the probed one.
pub(crate) fn sync_tab(state: &DesktopState, key: TabKey) -> Result<Synced, String> {
    let active = listed(state, key)?;
    let content = content_of(state, &active)?;
    let adopt_head_oid = content
        .tab
        .pr_number
        .and_then(|number| super::pr_cache_head_oid_for_pr(state, number));
    let command = Command::RefreshDiff {
        key,
        force: true,
        adopt_head_oid,
    };
    let reviewed_cleared = cleared(send(state, &active.tab, command)?)?;
    // The refresh may have resolved the base again.
    let synced = state.service.content(key, &active.tab).unwrap_or(content);
    let tab = &synced.tab;
    let id = tab.identity();
    if !id.is_remote() && tab.pr_number.is_none() && tab.mode == DiffMode::Branch {
        super::align_branch_base_probe(state, &tab.repo_root, &tab.base_branch);
    }
    let status_key = state
        .pr_cache
        .lock()
        .ok()
        .and_then(|cache| crate::snapshot::resolve_github_status_key_for(&id, &cache));
    Ok(Synced {
        reviewed_cleared,
        repo_root: tab.repo_root.clone(),
        status_key,
    })
}

/// The views the workspace loads, by the names the page sends.
pub(crate) fn routed_mode(mode: &str) -> Option<DiffMode> {
    match mode {
        "branch" => Some(DiffMode::Branch),
        "unstaged" => Some(DiffMode::Unstaged),
        "staged" => Some(DiffMode::Staged),
        "history" => Some(DiffMode::History),
        "conflicts" => Some(DiffMode::Conflicts),
        "hidden" => Some(DiffMode::Hidden),
        "pr" | "pr_diff" => Some(DiffMode::PrDiff),
        "tour" => Some(DiffMode::Tour),
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
    let active = active(state)?;
    set_tab_mode(state, active.key, mode, diff_mode, pr_number)?;
    Ok(AppSnapshot::default())
}

/// Shows `key` in `mode`, under the same feature gate as the active tab's
/// switch.
pub(crate) fn set_tab_mode(
    state: &DesktopState,
    key: TabKey,
    name: &str,
    mode: DiffMode,
    pr_number: Option<u64>,
) -> Result<(), String> {
    let features = state.service.settings().view().config.features.clone();
    if !super::feature_allows_mode_str(&features, name) {
        return Err(format!("'{name}' view is disabled in settings"));
    }
    let entry = listed(state, key)?;
    let content = content_of(state, &entry)?;
    if content.tab.mode != mode {
        send(
            state,
            &entry.tab,
            Command::SetDiffMode {
                key,
                mode,
                pr_number,
            },
        )?;
    }
    Ok(())
}

/// Shows commit `sha` of `key`'s History, under History's feature gate.
pub(crate) fn select_tab_commit(
    state: &DesktopState,
    key: TabKey,
    sha: &str,
) -> Result<(), String> {
    let features = state.service.settings().view().config.features.clone();
    if !super::feature_allows_mode_str(&features, "history") {
        return Err("'history' view is disabled in settings".to_string());
    }
    let entry = listed(state, key)?;
    content_of(state, &entry)?;
    let sha = sha.to_string();
    send(state, &entry.tab, Command::SelectCommit { key, sha })?;
    Ok(())
}

/// Shows `key` in the view its History was entered from.
pub(crate) fn exit_tab_history(state: &DesktopState, key: TabKey) -> Result<(), String> {
    let entry = listed(state, key)?;
    content_of(state, &entry)?;
    send(state, &entry.tab, Command::ExitHistory { key })?;
    Ok(())
}

/// The active tab's History, for the old commands with the flag on.
pub(super) fn select_commit(state: &AppState, sha: &str) -> Result<AppSnapshot, String> {
    let active = active(state)?;
    select_tab_commit(state, active.key, sha)?;
    Ok(AppSnapshot::default())
}

pub(super) fn exit_history(state: &AppState) -> Result<AppSnapshot, String> {
    let active = active(state)?;
    exit_tab_history(state, active.key)?;
    Ok(AppSnapshot::default())
}

/// Diffs `key` against `base` and keeps it as the tab's saved base, as the
/// App's base change persists the tab.
pub(crate) fn change_tab_base(state: &DesktopState, key: TabKey, base: &str) -> Result<(), String> {
    let entry = listed(state, key)?;
    content_of(state, &entry)?;
    let base = base.to_string();
    send(state, &entry.tab, Command::ChangeBase { key, base })?;
    let changed = state
        .service
        .content(key, &entry.tab)
        .ok_or("The tab's content did not load")?;
    let base_ref = changed.tab.base_branch.clone();
    app_command(state, AppCommand::SetBaseRef { key, base_ref })?;
    Ok(())
}

/// Expands `path` in `key`'s diff in full, or compacts it again. Either
/// choice is kept across the tab's later loads.
pub(crate) fn set_file_compacted(
    state: &DesktopState,
    key: TabKey,
    path: &str,
    compacted: bool,
) -> Result<(), String> {
    let entry = listed(state, key)?;
    content_of(state, &entry)?;
    let paths = vec![path.to_string()];
    let command = if compacted {
        Command::CompactFiles { key, paths }
    } else {
        Command::ParseFiles {
            key,
            paths,
            expand: true,
        }
    };
    send(state, &entry.tab, command)?;
    Ok(())
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
    send(
        state,
        &active.tab,
        Command::ParseFiles { key, paths, expand },
    )?;
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

/// Sends a reviewed write the page painted on `key` with the context of
/// that tab's content. Refused when `key` is not the registry's active tab
/// (decision A), and, for a write that names the view it was painted on,
/// when the content shows another one.
pub(crate) fn write_keyed(
    state: &DesktopState,
    key: TabKey,
    view: Option<&er_protocol::PaintedView>,
    build: impl FnOnce(TabContext) -> Command,
) -> Result<(), String> {
    let (active, content) = active_content(state)?;
    if active.key != key {
        return Err(MOVED.to_string());
    }
    if view.is_some_and(|view| !er_service::painted_view_matches(&content, view)) {
        return Err(MOVED.to_string());
    }
    send(state, &active.tab, build(content.context.clone()))?;
    Ok(())
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
