//! The 30 s GitHub status loop: the active tab's PR status, fetched again
//! when it is older than 90 seconds, without holding the App while `gh`
//! runs. It shares the in-flight set with `kick_github_status_refresh`, so a
//! tab switch during a tick does not fetch the same PR twice.

use std::time::Duration;

use crate::commands::AppState;

/// Starts the loop on its own thread.
pub fn spawn(state: AppState) {
    std::thread::spawn(move || loop {
        // Sleep first, so a launch does not start with a burst of fetches.
        std::thread::sleep(Duration::from_secs(30));
        let Some((owner, repo, number)) = pick(&state) else {
            continue;
        };
        let key = (owner, repo, number);
        let registered = state
            .gh_status_in_flight
            .lock()
            .map(|mut set| set.insert(key.clone()))
            .unwrap_or(false);
        if !registered {
            continue;
        }
        if let Ok(mut flags) = state.loading.lock() {
            flags.gh_status = true;
        }
        crate::commands::fetch_and_store_github_status(
            &state.gh_status_cache,
            &state.desktop_revision,
            &key.0,
            &key.1,
            key.2,
        );
        if let Ok(mut flags) = state.loading.lock() {
            flags.gh_status = false;
        }
        if let Ok(mut set) = state.gh_status_in_flight.lock() {
            set.remove(&key);
        }
    });
}

/// The PR this tick fetches, if its status is stale. The flag path reads
/// the registry's active tab and its content, so a held App never skips a
/// tick.
pub fn pick(state: &AppState) -> Option<(String, String, u64)> {
    #[cfg(feature = "v1-service")]
    return crate::resources::pr::status_loop_target(&state.desktop);
    #[cfg(not(feature = "v1-service"))]
    {
        let key = state
            .app
            .lock()
            .ok()
            .and_then(|app| active_pr_identity(app.tab(), &state.pr_cache))?;
        let last_updated = state
            .gh_status_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).and_then(|snap| snap.last_updated.clone()));
        let fresh = crate::gh_status_cache::status_is_fresh(
            last_updated.as_deref(),
            crate::gh_status_cache::now_epoch_secs(),
            90,
        );
        (!fresh).then_some(key)
    }
}

/// The active tab's PR as `(owner, repo, number)`. A remote PR tab names it
/// directly; a working-tree or local-branch tab is looked up in the PR cache by
/// head ref, preferring an open PR.
pub fn active_pr_identity(
    tab: &er_engine::app::TabState,
    pr_cache: &crate::pr_cache::PrCacheMap,
) -> Option<(String, String, u64)> {
    if let (Some(slug), Some(n)) = (tab.remote_repo.as_ref(), tab.pr_number) {
        return slug
            .split_once('/')
            .map(|(o, r)| (o.to_string(), r.to_string(), n));
    }
    let branch = tab
        .local_branch_view
        .as_deref()
        .unwrap_or(&tab.current_branch);
    let cache = pr_cache.lock().ok()?;
    cache.iter().find_map(|(slug, prs)| {
        prs.iter()
            .filter(|p| p.head_ref == branch)
            .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
            .and_then(|p| {
                slug.split_once('/')
                    .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
            })
    })
}
