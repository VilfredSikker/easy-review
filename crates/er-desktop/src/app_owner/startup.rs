//! The sidebar steps `main.rs` runs, kept in the library so the tests run
//! the code the app runs. They read only what the registry and the settings
//! actor publish.

use std::time::Duration;

use er_engine::config::InboxConfig;

use crate::state::DesktopState;

/// How long the metadata loop waits before its next refresh: twice as long
/// while the active tab is a remote PR.
pub fn meta_refresh_interval(state: &DesktopState) -> Duration {
    let remote = state
        .service
        .app_registry()
        .tabs_view()
        .active_entry()
        .is_some_and(|tab| tab.descriptor.kind == er_engine::tabs_record::TabKind::RemotePr);
    Duration::from_secs(if remote { 120 } else { 60 })
}

/// The root the first metadata pass reads on the service path: the
/// registry's active tab's, read once the restore has run, or `fallback` when
/// that tab has no local root (a remote PR) or no tab is open.
pub fn launch_root(registry: &er_service::AppRegistry, fallback: Option<&str>) -> String {
    use er_engine::tabs_record::TabKind;

    registry
        .tabs_view()
        .active_entry()
        .map(|tab| &tab.descriptor)
        .filter(|tab| tab.kind != TabKind::RemotePr && !tab.repo_root.is_empty())
        .map(|tab| tab.repo_root.clone())
        .or_else(|| fallback.map(str::to_string))
        .unwrap_or_default()
}

/// The active tab's PR as `(owner/repo, number)` for the 30s head probe.
/// None when the tab shows no PR or its origin cannot be resolved, which skips
/// this tick. The registry's active tab names it, so no lock skips one.
pub fn pr_head_probe_target(state: &DesktopState) -> Option<(String, u64)> {
    let (remote, number, root) = {
        let view = state.service.app_registry().tabs_view();
        let tab = &view.active_entry()?.descriptor;
        let remote = match (&tab.kind, &tab.pr_owner, &tab.pr_repo) {
            (er_engine::tabs_record::TabKind::RemotePr, Some(owner), Some(repo)) => {
                Some(format!("{owner}/{repo}"))
            }
            _ => None,
        };
        (remote, tab.pr_number?, tab.repo_root.clone())
    };
    // A remote tab names its repository; a local PR tab resolves it from origin.
    if let Some(slug) = remote {
        return slug.split_once('/').map(|_| (slug.clone(), number));
    }
    if root.is_empty() {
        return None;
    }
    let (owner, repo) = er_engine::github::get_repo_info(&*state.ports.git, &root).ok()?;
    Some((format!("{owner}/{repo}"), number))
}

/// The inbox prefs the PR sweep and the setup hook's flush read. On the flag
/// they come from the settings actor's view, which owns the config, so a held
/// App lock never holds them up; off it, from the App's config.
pub fn inbox_prefs(state: &DesktopState) -> InboxConfig {
    settings_inbox_prefs(state)
}

/// The inbox prefs as the settings actor last published them.
pub fn settings_inbox_prefs(desktop: &DesktopState) -> InboxConfig {
    desktop.service.settings().view().config.inbox.clone()
}

/// Has the sidebar's owner build `inbox` with the settings actor's prefs.
/// A publish never starts the actor: until something starts it, the owner
/// holds the inbox it has.
pub fn read_inbox_prefs_from_settings(desktop: &DesktopState) {
    let service = std::sync::Arc::clone(&desktop.service);
    desktop.app_owner.read_inbox_prefs_from(move || {
        let settings = service.settings_if_started()?;
        Some(settings.view().config.inbox.clone())
    });
}

/// The setup hook's step: shows the inbox items the sweep added before the
/// window had a handle to notify through.
pub fn flush_pending_notifications(state: &DesktopState) {
    let prefs = inbox_prefs(state);
    crate::route_helpers::flush_pending_native_notifications(
        &state.inbox,
        &state.tauri_app_handle,
        &prefs,
    );
}

/// Registers the project of every tab the registry lists, and, while no
/// project is registered at all, the active tab's repository; then
/// publishes. It reads the registry's list and never the App: before the
/// restore the App holds only its startup tab, and once the tabs follow the
/// registry the App's lock belongs to the follower.
pub fn register_open_projects(state: &DesktopState) {
    use crate::projects::{self, TabProjectRef};

    let view = state.service.app_registry().tabs_view();
    let refs: Vec<TabProjectRef> = view
        .tabs
        .iter()
        .map(|tab| TabProjectRef::from_descriptor(&tab.descriptor))
        .collect();
    let (github, git) = (&*state.ports.github, &*state.ports.git);
    projects::sync_projects_from_refs(github, git, &refs);
    if projects::load().projects.is_empty() {
        if let Some(tab) = view.active_entry() {
            let root = &tab.descriptor.repo_root;
            if !root.is_empty() {
                let _ = projects::auto_register(github, git, root);
            }
        }
    }
    state.app_owner.publish();
}
