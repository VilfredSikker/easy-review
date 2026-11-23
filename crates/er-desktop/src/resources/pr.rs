//! `pr(tab)` and `pr_status(remote, number)` on the flag path, and the two
//! PR card commands.
//!
//! `pr(tab)` follows `diff(tab)`'s rule: a loaded key answers its content's PR
//! section, a listed key nothing loaded posts `LoadTab` and answers empty at
//! version 0. `pr_status` is the app owner's, stamped from the status cache.
//! Which PR a tab's status is read for comes from the registry, the tab's
//! published content and the PR cache, never the App.

use er_engine::TabKey;
use er_protocol::{
    Ack, Command, ErError, ErErrorKind, Resource, ResourceChanged, ResourceKey, ResourceVersion,
    TabPr,
};
use er_service::{pr_resource, pr_version, resolve_tab_key, TabEntry};

use super::diff::{listed, published, Published};
use super::tabs::Routed;
use crate::app_owner::SidebarOwner;
use crate::commands::{self, content_routes};
use crate::service::actor_key;
use crate::state::DesktopState;

/// `wire`'s PR card and stack at the version its PR section was published at.
pub fn pr(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<(u64, Resource), ErError> {
    let entry = listed(desktop, wire)?;
    let (version, pr) = match published(desktop, &entry) {
        Published::Content(content) => (pr_version(&content), pr_resource(&content)),
        Published::Failed { revision, .. } => (revision, TabPr::default()),
        Published::Nothing => {
            desktop.service.request_load(entry.key, &entry.descriptor);
            (0, TabPr::default())
        }
    };
    Ok((version, Resource::Pr(Box::new(pr))))
}

/// The version `pr(key)` answers at, for the relay. Zero while nothing
/// loaded or failed.
pub fn published_version(desktop: &DesktopState, entry: &TabEntry) -> u64 {
    match published(desktop, entry) {
        Published::Content(content) => pr_version(&content),
        Published::Failed { revision, .. } => revision,
        Published::Nothing => 0,
    }
}

/// One PR's status as the owner last stamped it. A PR the cache holds no
/// status for answers none at version 0 and starts one fetch in the
/// background, which the push then brings.
pub fn pr_status(desktop: &DesktopState, remote: &str, number: u64) -> (u64, Resource) {
    let (version, mut status) = desktop.app_owner.pr_status(remote, number);
    if status.status.is_none() {
        if let Some((owner, repo)) = remote.split_once('/') {
            kick_status(desktop, owner.to_string(), repo.to_string(), number);
        }
    }
    let version = status_version(desktop, version);
    status.fetching = fetching(desktop, remote, number);
    (version, Resource::PrStatus(Box::new(status)))
}

/// `pr_status`'s version: the owner's for the stored status, plus each start
/// and end of a status fetch, which `fetching` follows.
pub fn status_version(desktop: &DesktopState, stored: u64) -> u64 {
    stored + desktop.status_fetch_clock.revision()
}

/// Whether a status fetch of `remote`'s PR `number` is running.
fn fetching(desktop: &DesktopState, remote: &str, number: u64) -> bool {
    let Some((owner, repo)) = remote.split_once('/') else {
        return false;
    };
    let key = (owner.to_string(), repo.to_string(), number);
    desktop
        .gh_status_in_flight
        .lock()
        .is_ok_and(|in_flight| in_flight.contains(&key))
}

/// The statuses being fetched, as `pr_status` keys.
pub fn fetched_keys(desktop: &DesktopState) -> Vec<ResourceKey> {
    let Ok(in_flight) = desktop.gh_status_in_flight.lock() else {
        return Vec::new();
    };
    in_flight
        .iter()
        .map(|(owner, repo, number)| ResourceKey::PrStatus {
            remote: format!("{owner}/{repo}"),
            number: *number,
        })
        .collect()
}

/// The `(owner, repo, number)` whose status `key`'s card shows: the tab's own
/// PR, or the PR the cache lists for its branch. None while the tab's
/// content has not loaded.
pub fn status_key(desktop: &DesktopState, key: TabKey) -> Option<(String, String, u64)> {
    let tabs = desktop.service.app_registry().tabs_view();
    let entry = tabs.tabs.iter().find(|entry| entry.key == key)?;
    let content = desktop.service.content(entry.key, &entry.descriptor)?;
    let cache = desktop.pr_cache.lock().ok()?;
    crate::snapshot::resolve_github_status_key_for(&content.tab.identity(), &cache)
}

/// [`status_key`] of the registry's active tab.
pub fn active_status_key(desktop: &DesktopState) -> Option<(String, String, u64)> {
    let active = desktop.service.app_registry().tabs_view().active?;
    status_key(desktop, active)
}

fn kick_status(desktop: &DesktopState, owner: String, repo: String, number: u64) {
    commands::kick_github_status_refresh(desktop, owner, repo, number);
}

/// Applies `command` when it is a PR card command.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    match command {
        Command::RefreshStack { tab } => Routed::Applied(refresh_stack(desktop, tab)),
        Command::RefreshGithubStatus { tab } => Routed::Applied(refresh_status(desktop, tab)),
        other => Routed::Other(other),
    }
}

/// Reads the tab's stack again and answers `pr(tab)` at the version the
/// lookup published.
fn refresh_stack(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<Ack, ErError> {
    let key = resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)?;
    let (_, tab) = content_routes::keyed_content(desktop, key, None).map_err(refused)?;
    desktop
        .service
        .send(&actor_key(&tab), er_service::Command::RefreshStack { key })
        .map_err(|error| refused(error.to_string()))?;
    let entry = listed(desktop, wire)?;
    Ok(Ack {
        changed: vec![ResourceChanged {
            key: ResourceKey::Pr { tab: wire },
            version: ResourceVersion(published_version(desktop, &entry)),
        }],
        created: None,
    })
}

/// Starts a fetch of the status the tab's card shows. Answers at once; the
/// push brings `pr_status`.
fn refresh_status(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<Ack, ErError> {
    let key = resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)?;
    if let Some((owner, repo, number)) = status_key(desktop, key) {
        kick_status(desktop, owner, repo, number);
    }
    Ok(Ack::default())
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}
