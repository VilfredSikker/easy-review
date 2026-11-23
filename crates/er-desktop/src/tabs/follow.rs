//! The app registry's tab list at startup and after each change (ADR 0070).
//!
//! The registry loads `tabs.json` and issues every tab key. The desktop hands
//! each restored tab its saved browser state, drops the view state of tabs
//! that leave, and loads the active tab when nothing has.

use std::sync::Arc;

use er_engine::ports::Ports;
use er_engine::TabKey;
use er_service::{AppCommand, AppRegistry, TabsView};

use crate::service::ServiceState;
use crate::view_state::ViewStateHandle;

/// The keys whose descriptor no longer resolves, such as a repo that was
/// moved or deleted.
fn broken(view: &TabsView, ports: &Ports) -> Vec<TabKey> {
    view.tabs
        .iter()
        .filter(|entry| {
            let resolved =
                er_engine::tab_identity::resolve_descriptor(&*ports.git, &entry.descriptor);
            if let Err(e) = &resolved {
                log::warn!(
                    "er-desktop: closing tab {:?} ({}): {e}",
                    entry.descriptor.kind,
                    entry.descriptor.repo_root
                );
            }
            resolved.is_err()
        })
        .map(|entry| entry.key)
        .collect()
}

/// Hands each listed tab the browser URL and layout its descriptor saved.
fn apply_browser(view_state: &ViewStateHandle, view: &TabsView) {
    let Ok(mut desktop) = view_state.lock() else {
        return;
    };
    for entry in &view.tabs {
        super::apply_descriptor_browser(&mut desktop, entry.key, &entry.descriptor);
    }
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
/// that left and the active tab loads when nothing has. The active tab also
/// loads now, since the restore that runs first sends no change.
pub fn follow_service(service: &Arc<ServiceState>, view_state: ViewStateHandle) {
    let registry = Arc::clone(service.app_registry());
    service.follow_tabs(move || prune(&view_state, &registry));
    let loader = Arc::clone(service);
    service.follow_tabs(move || {
        super::load_active(&loader);
    });
    super::load_active(service);
}

/// The startup restore, run once the service is built. Saved tabs whose
/// descriptor no longer resolves are closed. When no saved tab is left, a
/// Working tab on `fallback_root` (the launch directory's repo, else the
/// active project's root) opens first, since the registry keeps its last
/// tab; with no fallback either, the last broken tab stays.
pub fn restore(
    view_state: &ViewStateHandle,
    registry: &Arc<AppRegistry>,
    ports: &Ports,
    fallback_root: Option<&str>,
) {
    let view = registry.tabs_view();
    let failed = broken(&view, ports);
    if failed.len() == view.tabs.len() {
        if let Some(root) = fallback_root.filter(|root| !root.is_empty()) {
            let tab = crate::resources::content_routes::working_tab(root.to_string());
            let _ = registry.send(AppCommand::NewTab { tab });
        }
    }
    for key in &failed {
        let _ = registry.send(AppCommand::CloseTab { key: *key });
    }
    apply_browser(view_state, &registry.tabs_view());
}
