//! The owner's fetches: the PR-list refresh, one PR fetched on its own, and
//! the GitHub status fetch, with the bookkeeping `jobs` and `pr_status`
//! read. Each refresh or fetch that may wait on `gh` starts through a guard
//! and runs on a thread of its own, so its caller never waits.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use er_engine::github::GitHub;

use super::{lock, AppOwner};
use crate::app_records::sidebar::PrInfo;
use crate::app_records::{gh_status_cache, pr_cache, status_fetch};

/// A status's key: `(owner, repo, number)`.
pub type StatusKey = (String, String, u64);

#[derive(Default)]
pub(super) struct Fetches {
    /// Whether a PR-list refresh is running, which `jobs` shows.
    pr_list_running: AtomicBool,
    /// Moves when a PR-list refresh starts or ends, so `jobs` pushes it.
    pr_list_clock: AtomicU64,
    /// Moves when a status fetch starts or ends, so `pr_status` pushes it.
    /// One count serves every PR.
    status_clock: AtomicU64,
    /// Keys with a status fetch running; a second fetch of one is refused.
    in_flight: Mutex<HashSet<StatusKey>>,
    /// Every key a status fetch has started for. The relay keeps watching
    /// them after the fetch ends, so the end of a first fetch that stored
    /// nothing still reaches the page as `fetching: false`.
    fetched: Mutex<HashSet<StatusKey>>,
}

impl AppOwner {
    /// The PR cache, for readers; the owner is its one writer.
    pub fn pr_cache(&self) -> &pr_cache::PrCacheMap {
        &self.pr_cache
    }

    /// The GitHub login, once known.
    pub fn gh_user(&self) -> &crate::app_records::sidebar::GhUser {
        &self.gh_user
    }

    /// Each PR's last fetched GitHub status.
    pub fn status_cache(&self) -> &crate::app_records::sidebar::GhStatusCache {
        &self.gh_status
    }

    /// Marks a PR-list refresh as running and moves `jobs`; None when one
    /// already runs. The check and the mark are one swap. Dropping the guard
    /// ends the refresh, so a panic or an early return cannot leave it on.
    pub fn begin_pr_list_refresh(self: &Arc<Self>) -> Option<PrListRefresh> {
        if self.fetches.pr_list_running.swap(true, Ordering::AcqRel) {
            return None;
        }
        self.tick(&self.fetches.pr_list_clock);
        Some(PrListRefresh(Arc::clone(self)))
    }

    /// Whether a PR-list refresh is running.
    pub fn pr_list_running(&self) -> bool {
        self.fetches.pr_list_running.load(Ordering::Acquire)
    }

    /// How many times a PR-list refresh started or ended.
    pub fn pr_list_revision(&self) -> u64 {
        self.fetches.pr_list_clock.load(Ordering::Acquire)
    }

    /// Starts a PR-list refresh on a thread of its own, of every project
    /// remote or of `remote` alone, then runs `after` with the remotes whose
    /// fetch failed, before the refresh ends. A refresh already running
    /// absorbs it.
    pub fn kick_pr_list_refresh(
        self: &Arc<Self>,
        github: Arc<dyn GitHub>,
        remote: Option<String>,
        after: impl FnOnce(Vec<String>) + Send + 'static,
    ) {
        let Some(refresh) = self.begin_pr_list_refresh() else {
            return;
        };
        let owner = Arc::clone(self);
        std::thread::spawn(move || {
            let failed = match &remote {
                None => owner.refresh_prs(github),
                Some(remote) if owner.refresh_remote_prs(github, remote) => Vec::new(),
                Some(remote) => vec![remote.clone()],
            };
            after(failed);
            drop(refresh);
        });
    }

    /// One PR of `remote`: the cached row, or a fetch that the cache then
    /// holds, so a prefetched open pays no network on the click.
    pub fn cache_single_pr_for_remote(
        &self,
        github: &dyn GitHub,
        remote: &str,
        number: u64,
    ) -> Result<PrInfo, String> {
        let cached = lock(&self.pr_cache)
            .get(remote)
            .and_then(|prs| prs.iter().find(|pr| pr.number == number).cloned());
        if let Some(pr) = cached {
            return Ok(pr);
        }
        let fetched = pr_cache::fetch_single_pr_for_remote(github, remote, number)?;
        self.cache_pr(remote, fetched.clone());
        Ok(fetched)
    }

    /// Marks the status fetch of `key` as running and moves `pr_status`;
    /// None when it already runs. Dropping the guard ends the fetch.
    pub fn begin_status_fetch(self: &Arc<Self>, key: &StatusKey) -> Option<StatusFetch> {
        if !lock(&self.fetches.in_flight).insert(key.clone()) {
            return None;
        }
        lock(&self.fetches.fetched).insert(key.clone());
        self.tick(&self.fetches.status_clock);
        Some(StatusFetch {
            owner: Arc::clone(self),
            key: key.clone(),
        })
    }

    /// Whether the status fetch of `key` is running.
    pub fn status_fetching(&self, key: &StatusKey) -> bool {
        lock(&self.fetches.in_flight).contains(key)
    }

    /// Every key a status fetch has started for.
    pub fn status_fetched(&self) -> Vec<StatusKey> {
        lock(&self.fetches.fetched).iter().cloned().collect()
    }

    /// How many times a status fetch started or ended.
    pub fn status_fetch_revision(&self) -> u64 {
        self.fetches.status_clock.load(Ordering::Acquire)
    }

    /// Fetches one PR's GitHub status and stores it under a ticket, which
    /// keeps an older fetch from overwriting a newer one, then publishes.
    /// Persisting is left to the caller. Answers whether it stored.
    pub fn fetch_and_store_github_status(&self, owner: &str, repo: &str, number: u64) -> bool {
        let ticket = gh_status_cache::begin_fetch();
        let hook = self.change_hook();
        let Some(status) = status_fetch::fetch_github_status(owner, repo, number, hook) else {
            return false;
        };
        let key = (owner.to_string(), repo.to_string(), number);
        let stored = gh_status_cache::store_fetched(&self.gh_status, key, ticket, status);
        if stored {
            self.publish();
        }
        stored
    }

    /// Starts a status fetch of `(owner, repo, number)` on a thread of its
    /// own and saves the cache when it stored. A fetch of the key already
    /// running absorbs it.
    pub fn kick_github_status_refresh(self: &Arc<Self>, owner: String, repo: String, number: u64) {
        let key = (owner.clone(), repo.clone(), number);
        let Some(fetch) = self.begin_status_fetch(&key) else {
            return;
        };
        let this = Arc::clone(self);
        std::thread::spawn(move || {
            let _fetch = fetch;
            if this.fetch_and_store_github_status(&owner, &repo, number) {
                gh_status_cache::save_persisted_gh_status_cache(&this.gh_status);
            }
        });
    }

    /// Moves `clock` and wakes the relay.
    fn tick(&self, clock: &AtomicU64) {
        clock.fetch_add(1, Ordering::AcqRel);
        if let Some(wake) = lock(&self.wake).as_ref() {
            let _ = wake.send(());
        }
    }
}

/// A running PR-list refresh. Dropping it marks the refresh as done and
/// moves `jobs`.
#[must_use = "dropping the guard ends the refresh"]
pub struct PrListRefresh(Arc<AppOwner>);

impl Drop for PrListRefresh {
    fn drop(&mut self) {
        let fetches = &self.0.fetches;
        fetches.pr_list_running.store(false, Ordering::Release);
        self.0.tick(&fetches.pr_list_clock);
    }
}

/// A running status fetch. Dropping it marks the fetch as done and moves
/// `pr_status`.
#[must_use = "dropping the guard ends the fetch"]
pub struct StatusFetch {
    owner: Arc<AppOwner>,
    key: StatusKey,
}

impl Drop for StatusFetch {
    fn drop(&mut self) {
        lock(&self.owner.fetches.in_flight).remove(&self.key);
        self.owner.tick(&self.owner.fetches.status_clock);
    }
}
