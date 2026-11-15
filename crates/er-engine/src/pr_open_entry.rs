//! The local PR open cache's entry types and LRU policy, shared by the
//! desktop's flag-off open path and the service's app registry.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PrOpenCacheKey {
    pub project_id: String,
    pub repo_root: String,
    pub pr_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrOpenFreshness {
    pub base_branch: String,
    pub head_branch: String,
    pub head_oid: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrOpenCacheEntry {
    pub freshness: PrOpenFreshness,
    pub raw_diff: String,
    /// Cached PR overview so a click after a hover-prefetch can render the
    /// right panel without re-running `gh pr view`.
    pub pr_data: Option<crate::github::PrOverviewData>,
    /// Cached GitHub PR commits, newest first, keyed by the same freshness.
    pub pr_commits: Option<Vec<crate::git::CommitInfo>>,
    /// Monotonic tick from `pr_open_clock()` recording the last read or write.
    /// Drives LRU eviction (`evict_lru`) when the cache exceeds its entry cap.
    /// Older persisted entries deserialize to 0 (treated as least-recent).
    #[serde(default)]
    pub last_touched: u64,
}

/// How many PR commits an open caches.
pub const PR_COMMIT_CACHE_LIMIT: usize = 250;

/// Monotonic counter for `PrOpenCacheEntry::last_touched`. Strictly increasing
/// per process; only relative ordering matters for LRU eviction, never the
/// absolute value.
pub fn pr_open_clock() -> u64 {
    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Per-process cap on the open-diff cache. Over this, the least-recently-touched
/// entry is evicted (LRU) so a hot PR survives a churn of one-off opens.
pub const MAX_PR_OPEN_CACHE_ENTRIES: usize = 32;

/// Evict the least-recently-touched entry when the map exceeds `MAX_PR_OPEN_CACHE_ENTRIES`.
/// Pure (operates on the borrowed map) so the LRU policy is unit-testable without a lock.
pub fn evict_lru(map: &mut HashMap<PrOpenCacheKey, PrOpenCacheEntry>) {
    if map.len() <= MAX_PR_OPEN_CACHE_ENTRIES {
        return;
    }
    // Remove the entry with the smallest `last_touched`. Touch times from
    // `pr_open_clock()` are process-unique and increasing, so there is a strict
    // minimum; the `HashMap` iteration order does not affect which key is chosen.
    let evict_key = map
        .iter()
        .min_by_key(|(_, e)| e.last_touched)
        .map(|(k, _)| k.clone());
    if let Some(key) = evict_key {
        map.remove(&key);
    }
}

/// Insert a fetched entry and evict over the cap. Pure: the caller holds the
/// lock and saves afterwards.
pub fn remember_pr_open_entry(
    map: &mut HashMap<PrOpenCacheKey, PrOpenCacheEntry>,
    key: PrOpenCacheKey,
    freshness: PrOpenFreshness,
    raw_diff: String,
    pr_data: Option<crate::github::PrOverviewData>,
    pr_commits: Option<Vec<crate::git::CommitInfo>>,
) {
    // Preserve existing metadata if the new entry doesn't bring it (lets a
    // hint-based prefetch keep data fetched by an earlier full open).
    let pr_data = pr_data.or_else(|| {
        map.get(&key)
            .filter(|e| e.freshness == freshness)
            .and_then(|e| e.pr_data.clone())
    });
    let pr_commits = pr_commits.or_else(|| {
        map.get(&key)
            .filter(|e| e.freshness == freshness)
            .and_then(|e| e.pr_commits.clone())
    });
    map.insert(
        key,
        PrOpenCacheEntry {
            freshness,
            raw_diff,
            pr_data,
            pr_commits,
            last_touched: pr_open_clock(),
        },
    );
    evict_lru(map);
}
