//! The sidebar steps `main.rs` runs, kept in the library so the tests run
//! the code the app runs. They are handed the `AppState` because the default
//! build still reads the App here; the service build reads only the
//! registry's published tab list.

use std::time::Duration;

use crate::commands::AppState;

/// How long the metadata loop waits before its next refresh: twice as long
/// while the active tab is a remote PR.
pub fn meta_refresh_interval(state: &AppState) -> Duration {
    #[cfg(feature = "v1-service")]
    let remote = state
        .service
        .app_registry()
        .tabs_view()
        .active_entry()
        .is_some_and(|tab| tab.descriptor.kind == er_engine::tabs_record::TabKind::RemotePr);
    #[cfg(not(feature = "v1-service"))]
    let remote = state.app.try_lock().is_ok_and(|app| app.tab().is_remote());
    Duration::from_secs(if remote { 120 } else { 60 })
}

/// The active tab's PR as `(owner/repo, number)` for the 30s head probe.
/// None when the tab shows no PR or its origin cannot be resolved, and, off
/// the flag, while the App is busy: each skips this tick. On the flag the
/// registry's active tab names it, so a held App lock never skips one.
pub fn pr_head_probe_target(state: &AppState) -> Option<(String, u64)> {
    #[cfg(feature = "v1-service")]
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
    #[cfg(not(feature = "v1-service"))]
    let (remote, number, root) = {
        let app = state.app.try_lock().ok()?;
        let tab = app.tab();
        (
            tab.remote_repo.clone(),
            tab.pr_number?,
            tab.repo_root.clone(),
        )
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

/// Registers the project of every tab the registry lists, and, while no
/// project is registered at all, the active tab's repository; then
/// publishes. It reads the registry's list and never the App: before the
/// restore the App holds only its startup tab, and once the tabs follow the
/// registry the App's lock belongs to the follower.
#[cfg(feature = "v1-service")]
pub fn register_open_projects(state: &AppState) {
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
