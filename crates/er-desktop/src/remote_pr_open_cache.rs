//! Remote-only PR open cache.
//!
//! Remote-only projects (no local git clone) have no `pr_open_cache` (that
//! one is keyed by a local repo root and filled by the sidebar hover
//! prefetch). Opening a remote PR runs three `gh` network calls
//! (`gh pr view`-metadata, `gh pr diff --repo`, commits) synchronously —
//! the slowest open path in the app.
//!
//! This module adds a small in-memory cache for those inputs plus the
//! in-flight claim set, so:
//! - the sidebar hover prefetch (`prefetch_remote_pr_open`) warms entries
//!   before the click, and
//! - a cache hit opens the PR with zero `gh` calls (the tab is built via
//!   `TabState::new_remote_with_data`, no network).
//!
//! Transient by design: re-fetched on every app start (remote opens are
//! infrequent and freshness matters more than persistence here).
//! Worst-case resident memory: LIMIT × MAX_REMOTE_DIFF_BYTES ≈ 240 MB
//! (realistic PR diffs are <2 MB; the 20 MB guard only bounds a single
//! pathological diff).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use er_engine::git::CommitInfo;
use er_engine::github::PrOverviewData;

/// Everything needed to build a remote PR tab without network I/O.
#[derive(Debug, Clone)]
pub struct RemotePrOpenEntry {
    pub base_branch: String,
    pub head_branch: String,
    pub raw_diff: String,
    pub pr_data: Option<PrOverviewData>,
    pub pr_commits: Vec<CommitInfo>,
    /// Head oid the cached diff was fetched at — the staleness baseline
    /// (review-fix-loop R1: without it, an advanced head would never light
    /// the stale pill because the baseline is seeded from the *current* head).
    pub head_oid: Option<String>,
    /// Monotonic LRU recency tick (see [`RemotePrOpenCache`]).
    pub last_touched: u64,
}

pub type RemotePrOpenCache = Arc<Mutex<HashMap<(String, String, u64), RemotePrOpenEntry>>>;
pub type RemotePrOpenInFlight = Arc<Mutex<std::collections::HashSet<(String, String, u64)>>>;

const REMOTE_PR_OPEN_CACHE_LIMIT: usize = 12;
const MAX_REMOTE_DIFF_BYTES: usize = 20_000_000;

/// Look up a cached remote-PR open entry, bumping its recency on a hit.
pub fn get_remote_pr_open_entry(
    cache: &RemotePrOpenCache,
    owner: &str,
    repo: &str,
    number: u64,
) -> Option<RemotePrOpenEntry> {
    let mut guard = cache.lock().ok()?;
    let key = (owner.to_string(), repo.to_string(), number);
    let entry = guard.get(&key)?.clone();
    if let Some(e) = guard.get_mut(&key) {
        e.last_touched = now_tick();
    }
    Some(entry)
}

/// Insert a fetched entry, evicting the least-recently-used entry when over
/// the cap. Skips oversized diffs (guards the tab parse budget).
pub fn insert_remote_pr_open_entry(
    cache: &RemotePrOpenCache,
    owner: &str,
    repo: &str,
    number: u64,
    mut entry: RemotePrOpenEntry,
) {
    if entry.raw_diff.len() > MAX_REMOTE_DIFF_BYTES {
        log::warn!(
            "remote_pr_open: skipping oversized diff for {owner}/{repo}#{number} ({} bytes)",
            entry.raw_diff.len()
        );
        return;
    }
    entry.last_touched = now_tick();
    let Ok(mut guard) = cache.lock() else {
        return;
    };
    guard.insert((owner.to_string(), repo.to_string(), number), entry);
    if guard.len() > REMOTE_PR_OPEN_CACHE_LIMIT {
        let lru = guard
            .iter()
            .min_by_key(|(_, e)| e.last_touched)
            .map(|(k, _)| k.clone());
        if let Some(lru) = lru {
            guard.remove(&lru);
        }
    }
}

/// Claim an in-flight prefetch slot. Returns `false` when one is already
/// running for this PR (caller skips).
pub fn claim_remote_pr_open(
    in_flight: &RemotePrOpenInFlight,
    owner: &str,
    repo: &str,
    number: u64,
) -> bool {
    let Ok(mut guard) = in_flight.lock() else {
        return false;
    };
    guard.insert((owner.to_string(), repo.to_string(), number))
}

/// Release an in-flight prefetch slot (call in a `finally`-style path).
pub fn release_remote_pr_open(
    in_flight: &RemotePrOpenInFlight,
    owner: &str,
    repo: &str,
    number: u64,
) {
    if let Ok(mut guard) = in_flight.lock() {
        guard.remove(&(owner.to_string(), repo.to_string(), number));
    }
}

/// Monotonic LRU tick — strictly increasing, so same-ms inserts still order
/// correctly (wall-clock ms timestamps would tie and make eviction arbitrary).
fn now_tick() -> u64 {
    static TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(raw: &str) -> RemotePrOpenEntry {
        RemotePrOpenEntry {
            base_branch: "main".to_string(),
            head_branch: "feature".to_string(),
            raw_diff: raw.to_string(),
            pr_data: None,
            pr_commits: Vec::new(),
            head_oid: None,
            last_touched: 0,
        }
    }

    fn cache() -> RemotePrOpenCache {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn round_trip_get_and_insert() {
        let c = cache();
        assert!(get_remote_pr_open_entry(&c, "o", "r", 1).is_none());
        insert_remote_pr_open_entry(&c, "o", "r", 1, entry("diff"));
        let got = get_remote_pr_open_entry(&c, "o", "r", 1).expect("present");
        assert_eq!(got.raw_diff, "diff");
        assert_eq!(got.base_branch, "main");
    }

    #[test]
    fn lru_evicts_least_recently_touched() {
        let c = cache();
        for i in 0..REMOTE_PR_OPEN_CACHE_LIMIT + 2 {
            insert_remote_pr_open_entry(&c, "o", "r", i as u64, entry(&format!("d{i}")));
        }
        let guard = c.lock().unwrap();
        assert_eq!(guard.len(), REMOTE_PR_OPEN_CACHE_LIMIT);
        // 0 and 1 were evicted (never touched after insert, lowest ticks).
        assert!(!guard.contains_key(&("o".into(), "r".into(), 0)));
        assert!(!guard.contains_key(&("o".into(), "r".into(), 1)));
        assert!(guard.contains_key(&(
            "o".into(),
            "r".into(),
            REMOTE_PR_OPEN_CACHE_LIMIT as u64 + 1
        )));
    }

    #[test]
    fn oversized_diff_is_skipped() {
        let c = cache();
        let big = "x".repeat(MAX_REMOTE_DIFF_BYTES + 1);
        insert_remote_pr_open_entry(&c, "o", "r", 9, entry(&big));
        assert!(get_remote_pr_open_entry(&c, "o", "r", 9).is_none());
    }

    #[test]
    fn claim_is_exclusive_and_release_clears() {
        let inflight: RemotePrOpenInFlight = Arc::new(Mutex::new(std::collections::HashSet::new()));
        assert!(claim_remote_pr_open(&inflight, "o", "r", 3));
        assert!(!claim_remote_pr_open(&inflight, "o", "r", 3), "dedupes");
        release_remote_pr_open(&inflight, "o", "r", 3);
        assert!(claim_remote_pr_open(&inflight, "o", "r", 3), "released");
    }
}
