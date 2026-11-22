//! The flag path's loads that no route asks for. The tab the registry makes
//! active loads on that change, so a followed or restored stub shows its diff
//! without waiting on a fetch. The warmer loads the active repo's other tabs
//! at idle, as the flag-off warmer upgrades its stubs.

use std::sync::Arc;
use std::time::Duration;

use er_service::{TabEntry, TabsView};

use crate::service::ServiceState;

/// Posts a load of the registry's active tab when its actor has published
/// nothing for it. Never waits. Answers whether it posted.
pub fn load_active(service: &ServiceState) -> bool {
    let view = service.app_registry().tabs_view();
    let Some(entry) = view.active_entry() else {
        return false;
    };
    !service.has_published(entry.key, &entry.descriptor)
        && service.request_load(entry.key, &entry.descriptor)
}

/// The tabs the warmer loads, in list order: those on the active tab's repo
/// root, less the active tab, which activation loads. With no active tab, or
/// one with no repo root, it warms nothing, so it never loads another
/// project's tabs.
pub fn warm_targets(view: &TabsView) -> Vec<TabEntry> {
    let Some(active) = view.active_entry() else {
        return Vec::new();
    };
    let root = &active.descriptor.repo_root;
    if root.is_empty() {
        return Vec::new();
    }
    view.tabs
        .iter()
        .filter(|entry| entry.key != active.key && entry.descriptor.repo_root == *root)
        .cloned()
        .collect()
}

/// Loads each warm target its actor has published nothing for, one at a
/// time, waiting for each reply and pausing between loads so the page's own
/// work goes first. A tab closed or made active meanwhile is skipped.
/// Answers how many it loaded.
pub fn warm(service: &ServiceState, pause: Duration) -> usize {
    let mut loaded = 0;
    for target in warm_targets(&service.app_registry().tabs_view()) {
        let view = service.app_registry().tabs_view();
        let listed = view.tabs.iter().any(|entry| entry.key == target.key);
        if !listed || view.active == Some(target.key) {
            continue;
        }
        if service.has_published(target.key, &target.descriptor) {
            continue;
        }
        if service.load_and_wait(target.key, &target.descriptor) {
            loaded += 1;
        }
        std::thread::sleep(pause);
    }
    loaded
}

/// Runs the warmer once on a thread of its own, after `grace`, so the active
/// tab's diff and the first frame land first.
pub fn spawn_warmer(service: Arc<ServiceState>, grace: Duration) {
    let _ = std::thread::Builder::new()
        .name("er-tab-warmer".into())
        .spawn(move || {
            std::thread::sleep(grace);
            let loaded = warm(&service, Duration::from_millis(150));
            log::debug!("er-desktop: the warmer loaded {loaded} tab(s)");
        });
}
