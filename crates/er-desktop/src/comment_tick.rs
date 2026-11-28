//! The 45 s comment tick: the same head-OID throttle decides whether to
//! sync the active tab's PR comments. With the flag on the sync is a command
//! to the tab's workspace actor, which writes the sidecar and publishes the
//! change for the relay to emit, and the PR comes from the registry, its
//! content and the PR cache, so a held App never skips a tick.

use std::collections::HashMap;
use std::time::Instant;

/// Each PR's head OID at its last sync, and when.
pub type Synced = HashMap<(String, String, u64), (String, Instant)>;

/// The PR a tick sent a sync for, and the workspace's reply.
pub type Ticked = (
    (String, String, u64),
    Result<er_service::Reply, er_service::WorkspaceError>,
);

/// Starts the tick on its own thread.
pub fn spawn(state: crate::state::DesktopState) {
    std::thread::spawn(move || {
        let mut synced = Synced::new();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(45));
            if let Some((_, Err(e))) = tick(&state, &mut synced) {
                log::error!("github comment sync failed: {e}");
            }
        }
    });
}

/// One tick: sends the active tab's PR a background sync unless it synced
/// against the same head in the last 90 seconds. Answers the PR it sent the
/// sync for and the workspace's reply; none when it sent nothing.
pub fn tick(state: &crate::state::DesktopState, synced: &mut Synced) -> Option<Ticked> {
    let desktop = state;
    let tabs = desktop.service.app_registry().tabs_view();
    let entry = tabs.active_entry()?;
    let pr = crate::resources::pr::status_key(desktop, entry.key)?;
    let content = desktop.service.content(entry.key, &entry.descriptor)?;
    let head_oid = cached_head_oid(desktop.app_owner.pr_cache(), &pr.0, &pr.1, pr.2);
    let (now, ttl) = (Instant::now(), std::time::Duration::from_secs(90));
    if comment_sync_recently_synced(synced.get(&pr), &head_oid, now, ttl) {
        return None;
    }
    let sent = desktop.service.send(
        &crate::service::actor_key(&entry.descriptor),
        er_service::Command::SyncGithubComments {
            context: content.context.clone(),
            source: er_service::SyncSource::Background,
        },
    );
    if sent.is_ok() {
        synced.insert(pr.clone(), (head_oid, Instant::now()));
    }
    Some((pr, sent))
}

/// Look up the cached head commit OID for `(owner, repo, number)` in the
/// shared PR cache. Returns `""` when the remote/PR isn't cached yet —
/// callers must treat that as "unknown" (fail open, never skip on it), since
/// an empty OID is also `PrInfo::head_oid`'s `#[serde(default)]` value, not a
/// real "no head" signal.
pub fn cached_head_oid(
    pr_cache: &er_service::app_records::pr_cache::PrCacheMap,
    owner: &str,
    repo: &str,
    number: u64,
) -> String {
    pr_cache
        .lock()
        .ok()
        .and_then(|cache| {
            cache
                .get(&format!("{owner}/{repo}"))
                .and_then(|prs| prs.iter().find(|p| p.number == number))
                .map(|p| p.head_oid.clone())
        })
        .unwrap_or_default()
}

/// Whether the background comment-sync loop should SKIP this tick: this PR's
/// head OID is unchanged since the last successful sync AND that sync was less
/// than `ttl` ago. This is a poll THROTTLE, not change-detection — PR comments
/// and reviews are posted independently of any push, so an unchanged head OID
/// does NOT mean "nothing new". `ttl` therefore bounds how stale the comment
/// panel can get on a push-idle PR; the OID check only ACCELERATES resync after
/// a push (a new head OID drops through the throttle so freshly-pushed review
/// threads appear promptly). An empty/unknown `head_oid` never skips (fail open —
/// keep syncing every tick until the probe loop populates a real OID). Pure so
/// the skip decision is unit-testable without the surrounding thread/loop.
pub fn comment_sync_recently_synced(
    last_entry: Option<&(String, std::time::Instant)>,
    head_oid: &str,
    now: std::time::Instant,
    ttl: std::time::Duration,
) -> bool {
    !head_oid.is_empty()
        && last_entry.is_some_and(|(last_oid, last_at)| {
            last_oid == head_oid && now.saturating_duration_since(*last_at) < ttl
        })
}
