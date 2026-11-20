//! `diff(tab)` on the flag path: a listed tab's file list and diff summary,
//! answered from the content its workspace last published.
//!
//! A fetch never waits on an actor (ADR 0067). A loaded key answers its
//! content; a listed key nothing loaded posts `LoadTab` and answers an empty
//! diff at version 0, below any published section, so the push after the
//! load brings the rows; a key whose first load failed answers why.

use std::sync::Arc;

use er_engine::config::ErConfig;
use er_engine::TabKey;
use er_protocol::{
    Ack, Command, ErError, ErErrorKind, NoticeLevel, Resource, ResourceChanged, ResourceKey,
    ResourceVersion, TabDiff,
};
use er_service::{
    diff_load_error, diff_resource, diff_version, resolve_tab_key, wire_tab_key, DiffInputs,
    TabContent, TabEntry,
};

use super::send::Effects;
use super::tabs::Routed;
use crate::commands::{self, content_routes};
use crate::service::actor_key;
use crate::state::DesktopState;
use crate::view_state::TabFilter;

/// What a tab's workspace has published for it.
enum Published {
    Content(Arc<TabContent>),
    Failed { revision: u64, message: String },
    Nothing,
}

/// `wire`'s diff at the version its sections were published at.
pub fn diff(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<(u64, Resource), ErError> {
    let entry = listed(desktop, wire)?;
    let (version, diff) = match published(desktop, &entry) {
        Published::Content(content) => {
            let config = config(desktop);
            let filter = filter(desktop, entry.key);
            let inputs = inputs(&content, config.as_deref(), &filter);
            let version = content_version(desktop, &content, &filter);
            (version, diff_resource(&content, &inputs))
        }
        Published::Failed { revision, message } => (revision, diff_load_error(&message)),
        Published::Nothing => {
            desktop.service.request_load(entry.key, &entry.descriptor);
            (0, TabDiff::default())
        }
    };
    Ok((version, Resource::Diff(Box::new(diff))))
}

/// `wire`'s diff version, read without building it or loading anything.
pub fn version(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<u64, ErError> {
    let entry = listed(desktop, wire)?;
    Ok(published_version(desktop, entry.key, &entry.descriptor))
}

/// The version `diff(key)` answers at, for the relay. Zero while nothing
/// loaded or failed.
pub fn published_version(
    desktop: &DesktopState,
    key: TabKey,
    tab: &er_engine::tabs_record::TabDescriptor,
) -> u64 {
    let entry = TabEntry {
        key,
        descriptor: tab.clone(),
    };
    match published(desktop, &entry) {
        Published::Content(content) => content_version(desktop, &content, &filter(desktop, key)),
        Published::Failed { revision, .. } => revision,
        Published::Nothing => 0,
    }
}

fn listed(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<TabEntry, ErError> {
    let tabs = desktop.service.app_registry().tabs_view();
    let key = resolve_tab_key(&tabs, wire)?;
    tabs.tabs
        .iter()
        .find(|entry| entry.key == key)
        .cloned()
        .ok_or_else(|| ErError {
            kind: ErErrorKind::NotFound,
            message: "no open tab has that key".into(),
        })
}

fn published(desktop: &DesktopState, entry: &TabEntry) -> Published {
    let Some(view) = desktop
        .service
        .published_view(&actor_key(&entry.descriptor))
    else {
        return Published::Nothing;
    };
    if let Some(content) = view.tab(entry.key) {
        return Published::Content(Arc::clone(content));
    }
    match view.load_errors.get(&entry.key) {
        Some(error) => Published::Failed {
            revision: error.revision,
            message: error.message.clone(),
        },
        None => Published::Nothing,
    }
}

/// The rule tables come from the settings actor's config, when it is up; a
/// diff read never starts it. Its revision is part of the version, so a rule
/// edit moves every tab's diff.
fn config(desktop: &DesktopState) -> Option<Arc<er_service::SettingsView>> {
    desktop.service.settings_if_started().map(|s| s.view())
}

fn settings_revision(desktop: &DesktopState) -> u64 {
    config(desktop).map_or(0, |view| view.revision)
}

/// Three owners write what a diff shows: the workspace (its sections), the
/// settings actor (the rule tables) and the desktop (the filter). Each
/// revision only rises, so their sum does too.
fn content_version(desktop: &DesktopState, content: &TabContent, filter: &TabFilter) -> u64 {
    diff_version(content) + settings_revision(desktop) + filter.tick
}

fn filter(desktop: &DesktopState, key: TabKey) -> TabFilter {
    desktop
        .view
        .lock()
        .map(|view| view.filter(key))
        .unwrap_or_default()
}

fn inputs<'a>(
    content: &TabContent,
    config: Option<&er_service::SettingsView>,
    filter: &'a TabFilter,
) -> DiffInputs<'a> {
    let meta = &content.diff.meta;
    let fallback = ErConfig::default();
    let config = config.map_or(&fallback, |view| &view.config);
    DiffInputs {
        file_kinds: config
            .file_kinds
            .repo(&meta.rules_key)
            .cloned()
            .unwrap_or_default(),
        risk_rules: config.risk_rules_merged(&meta.rules_key, meta.risk_artifact.clone()),
        filter: &filter.expr,
        filter_history: &filter.history,
        sort_by_mtime: false,
    }
}

/// Applies `command` when it is a diff command.
pub fn apply(desktop: &DesktopState, command: Command, effects: &Effects<'_>) -> Routed {
    let applied = match command {
        Command::SetDiffMode {
            tab,
            mode,
            pr_number,
        } => resolve(desktop, tab).and_then(|key| {
            let Some(diff_mode) = content_routes::routed_mode(&mode) else {
                return Err(refused(format!("the {mode} view keeps its own command")));
            };
            content_routes::set_tab_mode(desktop, key, &mode, diff_mode, pr_number)
                .map_err(refused)?;
            Ok(moved(desktop, &[key]))
        }),
        Command::ChangeBase { tab, base } => resolve(desktop, tab).and_then(|key| {
            content_routes::change_tab_base(desktop, key, &base).map_err(refused)?;
            Ok(moved(desktop, &[key]))
        }),
        Command::SetFilter { tab, query } => resolve(desktop, tab).map(|key| {
            if let Ok(mut view) = desktop.view.lock() {
                view.set_filter(key, &query);
            }
            desktop.session.wake();
            moved(desktop, &[key])
        }),
        Command::RefreshDiff { tab, sync } => resolve(desktop, tab).and_then(|key| {
            refresh(desktop, key, sync, effects)?;
            Ok(moved(desktop, &[key]))
        }),
        Command::SyncPr {
            project_id,
            pr_number,
        } => sync_pr(desktop, &project_id, pr_number, effects),
        Command::SyncBranch { project_id, branch } => {
            sync_branch(desktop, &project_id, &branch, effects)
        }
        other => return Routed::Other(other),
    };
    Routed::Applied(applied)
}

/// A refresh, or a sync, which also fetches the tab's refs, lines the stale
/// mark up with the PR list and refreshes what the sync moved: the branch
/// metadata and the PR's GitHub status. A sync says where it landed and which
/// reviewed marks it cleared.
fn refresh(
    desktop: &DesktopState,
    key: TabKey,
    sync: bool,
    effects: &Effects<'_>,
) -> Result<(), ErError> {
    if !sync {
        content_routes::refresh_tab(desktop, key, false).map_err(refused)?;
        return Ok(());
    }
    let synced = content_routes::sync_tab(desktop, key).map_err(refused)?;
    commands::kick_meta_refresh(desktop, synced.repo_root);
    commands::refresh_status_after_sync(desktop, synced.status_key);
    let cleared = synced.reviewed_cleared;
    let landed = "Diff refreshed".to_string();
    let message = match cleared {
        0 => landed,
        1 => format!("{landed} · Reviewed cleared on 1 changed file"),
        n => format!("{landed} · Reviewed cleared on {n} changed files"),
    };
    (effects.notify)(NoticeLevel::Success, message);
    Ok(())
}

/// Reads the PR again, then syncs every open tab of it: a tab of the
/// project's clone, or of the PR's repo with no clone.
fn sync_pr(
    desktop: &DesktopState,
    project_id: &str,
    pr_number: u64,
    effects: &Effects<'_>,
) -> Result<Ack, ErError> {
    let project = crate::projects::load()
        .projects
        .into_iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| not_found("Project not found"))?;
    let remote = project
        .remote
        .clone()
        .ok_or_else(|| refused("Project has no remote configured".into()))?;
    commands::cache_single_pr_for_remote(desktop, &remote, pr_number).map_err(refused)?;
    let keys: Vec<TabKey> = open_tabs(desktop)
        .filter(|entry| {
            let tab = &entry.descriptor;
            tab.pr_number == Some(pr_number)
                && (descriptor_remote(tab).as_deref() == Some(remote.as_str())
                    || tab.repo_root == project.root_path)
        })
        .map(|entry| entry.key)
        .collect();
    sync_each(desktop, &keys, effects)
}

/// Syncs every open tab of the project's clone that shows `branch`: a view
/// of the branch, or a plain tab on it.
fn sync_branch(
    desktop: &DesktopState,
    project_id: &str,
    branch: &str,
    effects: &Effects<'_>,
) -> Result<Ack, ErError> {
    let root = crate::projects::load()
        .projects
        .into_iter()
        .find(|p| p.id == project_id)
        .map(|p| p.root_path)
        .ok_or_else(|| not_found("Project not found"))?;
    let keys: Vec<TabKey> = open_tabs(desktop)
        .filter(|entry| {
            let tab = &entry.descriptor;
            let shows = match tab.branch.as_deref() {
                Some(viewed) => viewed == branch,
                None => desktop
                    .service
                    .content(entry.key, tab)
                    .is_some_and(|content| content.tab.current_branch == branch),
            };
            tab.kind != er_engine::tabs_record::TabKind::RemotePr
                && tab.pr_number.is_none()
                && tab.repo_root == root
                && shows
        })
        .map(|entry| entry.key)
        .collect();
    sync_each(desktop, &keys, effects)
}

/// Syncs each tab; one that fails is logged and the rest go on, as the App's
/// syncs do.
fn sync_each(desktop: &DesktopState, keys: &[TabKey], effects: &Effects<'_>) -> Result<Ack, ErError> {
    for &key in keys {
        if let Err(error) = refresh(desktop, key, true, effects) {
            log::warn!("er-desktop: a sync of a tab failed: {}", error.message);
        }
    }
    Ok(moved(desktop, keys))
}

fn open_tabs(desktop: &DesktopState) -> impl Iterator<Item = TabEntry> {
    desktop
        .service
        .app_registry()
        .tabs_view()
        .tabs
        .clone()
        .into_iter()
}

fn descriptor_remote(tab: &er_engine::tabs_record::TabDescriptor) -> Option<String> {
    Some(format!("{}/{}", tab.pr_owner.as_ref()?, tab.pr_repo.as_ref()?))
}

/// The `Ack` of a command that moved `keys`' diffs.
fn moved(desktop: &DesktopState, keys: &[TabKey]) -> Ack {
    let view = desktop.service.app_registry().tabs_view();
    let changed = keys
        .iter()
        .filter_map(|&key| view.tabs.iter().find(|entry| entry.key == key))
        .map(|entry| ResourceChanged {
            key: ResourceKey::Diff {
                tab: wire_tab_key(entry.key),
            },
            version: ResourceVersion(published_version(desktop, entry.key, &entry.descriptor)),
        })
        .collect();
    Ack {
        changed,
        created: None,
    }
}

fn resolve(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<TabKey, ErError> {
    resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}

fn not_found(message: &str) -> ErError {
    ErError {
        kind: ErErrorKind::NotFound,
        message: message.to_string(),
    }
}
