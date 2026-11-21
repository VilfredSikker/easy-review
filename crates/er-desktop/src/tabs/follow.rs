//! The flag-on App follows the app registry's tab list by key (ADR 0070).
//!
//! On the flag path the registry issues every tab key. The App keeps the tabs
//! the registry lists, gains a lazy stub for each one it lacks under the
//! registry's key, and drops the rest, so every reader of `app.tab()` sees the
//! tab the registry calls active. Stubs are built with no App lock held, so a
//! tab command never waits on one.

use std::sync::{Arc, Mutex};

use er_engine::app::{App, TabState};
use er_engine::ports::Ports;
use er_engine::TabKey;
use er_service::{AppCommand, AppRegistry, Reply, TabsView};

use crate::service::ServiceState;
use crate::view_state::ViewStateHandle;

/// The tabs a view lists that `have` lacks, built under the registry's keys,
/// and the keys whose descriptor failed to build. Only `eager` loads its diff
/// now; the rest stay lazy stubs, since nothing on the flag path loads a
/// stub's diff yet.
fn build_missing(
    have: &[TabKey],
    view: &TabsView,
    ports: &Ports,
    eager: Option<TabKey>,
) -> (Vec<TabState>, Vec<TabKey>) {
    let mut built = Vec::new();
    let mut failed = Vec::new();
    for entry in &view.tabs {
        if have.contains(&entry.key) {
            continue;
        }
        let lazy = eager != Some(entry.key);
        match super::rebuild_tab_with(&entry.descriptor, lazy, ports) {
            Ok(mut tab) => {
                tab.adopt_key(entry.key);
                built.push(tab);
            }
            Err(e) => {
                log::warn!(
                    "er-desktop: closing tab {:?} ({}): {e}",
                    entry.descriptor.kind,
                    entry.descriptor.repo_root
                );
                failed.push(entry.key);
            }
        }
    }
    (built, failed)
}

/// Makes the App's tab list the view's: the listed tabs it has, plus `built`,
/// in the view's order, with the view's active tab active. A view that lists
/// no tab the App can show leaves the App as it is, since the App always
/// holds one tab.
pub fn follow_registry(app: &mut App, view: &TabsView, built: Vec<TabState>) {
    let held = app.tabs.len();
    let mut pool: Vec<TabState> = std::mem::take(&mut app.tabs);
    pool.extend(built);
    let mut tabs = Vec::with_capacity(view.tabs.len());
    for entry in &view.tabs {
        if let Some(idx) = pool.iter().position(|tab| tab.key() == entry.key) {
            tabs.push(pool.swap_remove(idx));
        }
    }
    if tabs.is_empty() {
        pool.truncate(held);
        app.tabs = pool;
        return;
    }
    let active = view
        .active
        .and_then(|key| tabs.iter().position(|tab| tab.key() == key))
        .unwrap_or(0);
    app.tabs = tabs;
    app.active_tab = active;
    app.sync_repo_rules_to_tabs();
    app.sync_config_from_active_tab();
}

/// Applies `view` to the App and hands each new tab its saved browser state.
/// The view lock is taken after the engine lock (ADR 0068).
fn apply(app: &Mutex<App>, view_state: &ViewStateHandle, view: &TabsView, built: Vec<TabState>) {
    let Ok(mut app) = app.lock() else {
        return;
    };
    let Ok(mut desktop) = view_state.lock() else {
        return;
    };
    for tab in &built {
        if let Some(entry) = view.tabs.iter().find(|entry| entry.key == tab.key()) {
            super::apply_descriptor_browser(&mut desktop, tab, &entry.descriptor);
        }
    }
    follow_registry(&mut app, view, built);
}

/// Drops the view state of every tab the registry no longer lists.
pub fn prune(view_state: &ViewStateHandle, registry: &AppRegistry) {
    let live: Vec<TabKey> = registry
        .tabs_view()
        .tabs
        .iter()
        .map(|tab| tab.key)
        .collect();
    if let Ok(mut view) = view_state.lock() {
        view.retain_tabs(&live);
    }
}

/// From now on, after each tab-list change: the view state drops the tabs
/// that left, the active tab loads when nothing has, and the App follows the
/// list. Each runs on a thread of its own, so neither the prune nor the load
/// waits on the App lock. The active tab also loads now, since the restore
/// that runs first sends no change.
pub fn follow_service(
    service: &Arc<ServiceState>,
    app: Arc<Mutex<App>>,
    view_state: ViewStateHandle,
    ports: Ports,
) {
    let registry = Arc::clone(service.app_registry());
    let (pruned, pruning) = (Arc::clone(&view_state), Arc::clone(&registry));
    service.follow_tabs(move || prune(&pruned, &pruning));
    let loader = Arc::clone(service);
    service.follow_tabs(move || {
        super::load_active(&loader);
    });
    service.follow_tabs(move || follow(&app, &view_state, &registry, &ports));
    super::load_active(service);
}

/// Follows the registry once. A tab whose descriptor no longer builds is
/// closed in the registry, as the flag-off save drops it.
pub fn follow(
    app: &Mutex<App>,
    view_state: &ViewStateHandle,
    registry: &Arc<AppRegistry>,
    ports: &Ports,
) {
    let view = registry.tabs_view();
    let have: Vec<TabKey> = match app.lock() {
        Ok(app) => app.tabs.iter().map(TabState::key).collect(),
        Err(_) => return,
    };
    let (built, failed) = build_missing(&have, &view, ports, None);
    let view = close_failed(registry, &failed).unwrap_or(view);
    apply(app, view_state, &view, built);
}

/// The flag-on restore, run once the service is built. The registry loads
/// `tabs.json` and issues the keys; the App gives up its startup tab and
/// follows, loading the active tab's diff now. The other tabs stay lazy
/// stubs: on the flag path a select only sends `SelectTab`, and nothing calls
/// `kick_deferred_tab_refresh` for them, so they show no diff until 2c. With no
/// saved tab, the App's startup tab joins the registry under the key the
/// registry gives it. When no saved tab builds, a Working tab on
/// `fallback_root` (the launch directory's repo, else the active project's
/// root) opens first, since the registry keeps its last tab; with no
/// fallback either, the last broken tab stays.
pub fn restore(
    app: &Mutex<App>,
    view_state: &ViewStateHandle,
    registry: &Arc<AppRegistry>,
    ports: &Ports,
    fallback_root: Option<&str>,
) {
    let view = registry.tabs_view();
    if view.tabs.is_empty() {
        join_startup_tab(app, view_state, registry);
        return;
    }
    let (mut built, failed) = build_missing(&[], &view, ports, view.active);
    if built.is_empty() {
        if let Some(root) = fallback_root.filter(|root| !root.is_empty()) {
            let tab = crate::commands::content_routes::working_tab(root.to_string());
            if registry.send(AppCommand::NewTab { tab }).is_ok() {
                let opened = registry.tabs_view();
                built = build_missing(&failed, &opened, ports, opened.active).0;
            }
        }
    }
    let view = close_failed(registry, &failed).unwrap_or(view);
    apply(app, view_state, &view, built);
}

fn join_startup_tab(app: &Mutex<App>, view_state: &ViewStateHandle, registry: &Arc<AppRegistry>) {
    let Ok(mut app) = app.lock() else {
        return;
    };
    if app.tab().placeholder {
        return;
    }
    let Ok(desktop) = view_state.lock() else {
        return;
    };
    let tab = super::descriptor_from_tab(app.tab(), &desktop);
    drop(desktop);
    if let Ok(Reply::Tabs(view)) = registry.send(AppCommand::NewTab { tab }) {
        if let Some(key) = view.active {
            app.tab_mut().adopt_key(key);
        }
    }
}

/// Closes each failed key and answers the list after the last close.
fn close_failed(registry: &Arc<AppRegistry>, failed: &[TabKey]) -> Option<Arc<TabsView>> {
    for key in failed {
        let _ = registry.send(AppCommand::CloseTab { key: *key });
    }
    (!failed.is_empty()).then(|| registry.tabs_view())
}
