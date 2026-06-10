//! Nearest-PR cache (issue #70).
//!
//! Persists the top-N most recently updated PRs from the "My PRs" and
//! "To Review" sidebar lists so the UI can render them — and check out their
//! branches — immediately on startup, before any `gh` call completes
//! (stale-while-revalidate).
//!
//! Stored per remote under the managed storage root:
//! `<storage_root>/repos/<owner-repo-slug>/pr-cache.json`
//! (atomic writes via tmp+rename, like other sidecar files).
//!
//! Invalidation rules (applied by [`update_cache`] on every refresh):
//! - entries are rebuilt from the fresh fetch, so merged/closed PRs and PRs
//!   that fell out of the top-N recent simply drop out;
//! - entries whose `updated_at` is older than the TTL are dropped;
//! - the expensive derived value (`diff_hash`, SHA-256 of the PR diff) is
//!   carried over only when the head SHA is unchanged — when new commits
//!   arrive (`head_oid` changes) it is cleared so callers recompute it, and
//!   only entries still in the cache (i.e. still top-N recent) are eligible
//!   for recomputation via [`record_diff_hash`].

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::storage;

/// How many PRs per list ("My PRs" / "To Review") are cached.
pub const NEAREST_PR_LIMIT: usize = 10;

/// Default time-to-live for a cache entry: PRs not updated within this window
/// are considered stale and evicted. Configurable via `[pr_cache] ttl_days`.
pub const DEFAULT_TTL_DAYS: u64 = 7;

const PR_CACHE_SCHEMA_VERSION: u32 = 1;

/// Milliseconds in one day.
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// One cached PR — everything needed to render a sidebar row and check out
/// the branch without waiting for `gh`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedPr {
    pub number: u64,
    pub title: String,
    /// Head branch name (what `gh pr checkout` / local checkout needs).
    pub head_ref: String,
    /// Base branch name (e.g. "main").
    pub base_ref: String,
    /// Head commit SHA — freshness key for derived data.
    pub head_oid: String,
    /// ISO 8601 UTC timestamp from GitHub's `updatedAt`.
    pub updated_at: String,
    pub author: String,
    pub url: String,
    #[serde(default)]
    pub is_draft: bool,
    /// SHA-256 of the PR diff (expensive to compute — requires fetching the
    /// full diff). `None` means "not computed for the current `head_oid`".
    #[serde(default)]
    pub diff_hash: Option<String>,
}

/// Persisted cache for one remote.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NearestPrCache {
    pub version: u32,
    /// Epoch ms of the fetch that produced this cache.
    pub fetched_at_epoch_ms: u64,
    /// Open PRs authored by the current user (top-N most recent).
    pub my_prs: Vec<CachedPr>,
    /// Open PRs awaiting the current user's review (top-N most recent).
    pub to_review: Vec<CachedPr>,
}

/// Current time in epoch milliseconds.
pub fn now_epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Path of the nearest-PR cache for a remote ("owner/repo").
pub fn cache_path(remote: &str) -> PathBuf {
    let slug = storage::slugify(&remote.replace('/', "-"));
    storage::storage_root()
        .join("repos")
        .join(slug)
        .join("pr-cache.json")
}

/// Load the persisted cache for a remote. `Ok(None)` when missing or the
/// schema version doesn't match (treated as a cold cache, not an error).
pub fn load(remote: &str) -> Result<Option<NearestPrCache>> {
    let path = cache_path(remote);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed: NearestPrCache = match serde_json::from_str(&content) {
        Ok(c) => c,
        // Corrupt or hand-edited file — treat as cold cache rather than
        // failing every refresh.
        Err(_) => return Ok(None),
    };
    if parsed.version != PR_CACHE_SCHEMA_VERSION {
        return Ok(None);
    }
    Ok(Some(parsed))
}

/// Persist the cache for a remote atomically (tmp + rename).
pub fn save(remote: &str, cache: &NearestPrCache) -> Result<()> {
    let path = cache_path(remote);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(cache).context("failed to serialize pr cache")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("failed to rename into {}", path.display()))?;
    Ok(())
}

/// Parse a UTC ISO 8601 timestamp ("YYYY-MM-DDTHH:MM:SSZ") into epoch ms.
/// Returns `None` for malformed input. (No chrono dependency — matches the
/// hand-rolled `chrono_now()` precedent in `app/state`.)
pub fn parse_iso8601_epoch_ms(ts: &str) -> Option<u64> {
    let b = ts.as_bytes();
    if b.len() < 19 {
        return None;
    }
    if b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let year: i64 = ts.get(0..4)?.parse().ok()?;
    let month: i64 = ts.get(5..7)?.parse().ok()?;
    let day: i64 = ts.get(8..10)?.parse().ok()?;
    let hour: u64 = ts.get(11..13)?.parse().ok()?;
    let min: u64 = ts.get(14..16)?.parse().ok()?;
    let sec: u64 = ts.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    // Howard Hinnant's days_from_civil algorithm.
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146097 + doe - 719_468;
    if days < 0 {
        return None;
    }
    Some(((days as u64) * 86_400 + hour * 3_600 + min * 60 + sec) * 1_000)
}

/// Whether an entry is stale: `updated_at` older than `ttl_ms` before `now_ms`.
/// Unparseable timestamps are treated as *not* stale — the entry still ages
/// out via the top-N cut and merged/closed eviction.
fn is_stale(entry: &CachedPr, now_ms: u64, ttl_ms: u64) -> bool {
    match parse_iso8601_epoch_ms(&entry.updated_at) {
        Some(updated) => now_ms.saturating_sub(updated) > ttl_ms,
        None => false,
    }
}

/// Build the new top-N list from a fresh fetch, carrying expensive derived
/// data forward from the previous cache where still valid.
///
/// - `fresh` must contain only PRs that belong in the list (the caller filters
///   to open PRs authored-by-me / awaiting-my-review). Merged/closed PRs are
///   evicted simply by not appearing in `fresh`.
/// - Entries older than the TTL are dropped.
/// - The list is sorted by `updated_at` (descending) and cut to
///   [`NEAREST_PR_LIMIT`].
/// - `diff_hash` is reused from `old` only when the head SHA is unchanged;
///   a changed `head_oid` clears it so callers know to recompute.
pub fn refresh_list(
    old: &[CachedPr],
    fresh: &[CachedPr],
    now_ms: u64,
    ttl_ms: u64,
) -> Vec<CachedPr> {
    let mut list: Vec<CachedPr> = fresh
        .iter()
        .filter(|pr| !is_stale(pr, now_ms, ttl_ms))
        .cloned()
        .collect();
    // ISO 8601 UTC timestamps sort lexicographically.
    list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    list.truncate(NEAREST_PR_LIMIT);
    for entry in &mut list {
        if entry.diff_hash.is_some() {
            continue;
        }
        if let Some(prev) = old.iter().find(|o| o.number == entry.number) {
            if prev.head_oid == entry.head_oid && !entry.head_oid.is_empty() {
                entry.diff_hash = prev.diff_hash.clone();
            }
            // head_oid changed ⇒ leave diff_hash as None: new changes came in,
            // the hash must be recomputed (and only because the PR is still in
            // the top-N, by virtue of being in this list at all).
        }
    }
    list
}

/// Produce the next cache state from a fresh fetch of both lists.
pub fn update_cache(
    old: Option<&NearestPrCache>,
    fresh_my: &[CachedPr],
    fresh_to_review: &[CachedPr],
    now_ms: u64,
    ttl_ms: u64,
) -> NearestPrCache {
    let empty: &[CachedPr] = &[];
    let (old_my, old_review) = match old {
        Some(c) => (c.my_prs.as_slice(), c.to_review.as_slice()),
        None => (empty, empty),
    };
    NearestPrCache {
        version: PR_CACHE_SCHEMA_VERSION,
        fetched_at_epoch_ms: now_ms,
        my_prs: refresh_list(old_my, fresh_my, now_ms, ttl_ms),
        to_review: refresh_list(old_review, fresh_to_review, now_ms, ttl_ms),
    }
}

/// Entries whose diff hash needs (re)computation: the head SHA changed since
/// the hash was last computed, or it was never computed. By construction these
/// are exactly the PRs still in the top-N recent lists.
pub fn prs_needing_diff_hash(cache: &NearestPrCache) -> Vec<CachedPr> {
    let mut out: Vec<CachedPr> = Vec::new();
    for pr in cache.my_prs.iter().chain(cache.to_review.iter()) {
        if pr.diff_hash.is_none() && !out.iter().any(|p| p.number == pr.number) {
            out.push(pr.clone());
        }
    }
    out
}

/// Record a freshly computed diff hash for a PR, if (and only if) the PR is
/// still cached with the same head SHA — i.e. it is still in the top-N recent
/// and no newer commits landed while the hash was being computed.
/// Returns `true` when the cache was updated.
pub fn record_diff_hash(
    remote: &str,
    pr_number: u64,
    head_oid: &str,
    diff_hash: &str,
) -> Result<bool> {
    let Some(mut cache) = load(remote)? else {
        return Ok(false);
    };
    let mut changed = false;
    for pr in cache.my_prs.iter_mut().chain(cache.to_review.iter_mut()) {
        if pr.number == pr_number
            && pr.head_oid == head_oid
            && pr.diff_hash.as_deref() != Some(diff_hash)
        {
            pr.diff_hash = Some(diff_hash.to_string());
            changed = true;
        }
    }
    if changed {
        save(remote, &cache)?;
    }
    Ok(changed)
}

/// Convert a TTL in days (from config) to milliseconds.
pub fn ttl_ms_from_days(days: u64) -> u64 {
    days.saturating_mul(DAY_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::STORAGE_TEST_ENV_LOCK;
    use tempfile::TempDir;

    const NOW: u64 = 1_780_000_000_000; // arbitrary fixed "now" (epoch ms)
    const TTL: u64 = 7 * DAY_MS;

    fn pr(number: u64, updated_at: &str, head_oid: &str) -> CachedPr {
        CachedPr {
            number,
            title: format!("PR #{number}"),
            head_ref: format!("feature/{number}"),
            base_ref: "main".to_string(),
            head_oid: head_oid.to_string(),
            updated_at: updated_at.to_string(),
            author: "alice".to_string(),
            url: format!("https://github.com/org/repo/pull/{number}"),
            is_draft: false,
            diff_hash: None,
        }
    }

    #[test]
    fn parse_iso8601_epoch() {
        assert_eq!(parse_iso8601_epoch_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_iso8601_epoch_ms("2026-06-10T00:00:00Z"),
            Some(1_781_049_600_000)
        );
        assert_eq!(
            parse_iso8601_epoch_ms("2024-02-29T12:30:45Z"),
            Some(1_709_209_845_000)
        );
        assert_eq!(parse_iso8601_epoch_ms(""), None);
        assert_eq!(parse_iso8601_epoch_ms("not-a-date"), None);
        assert_eq!(parse_iso8601_epoch_ms("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn refresh_drops_entries_older_than_ttl() {
        let now = parse_iso8601_epoch_ms("2026-06-10T00:00:00Z").unwrap();
        let fresh = vec![
            pr(1, "2026-06-09T00:00:00Z", "aaa"), // 1 day old — kept
            pr(2, "2026-06-01T00:00:00Z", "bbb"), // 9 days old — stale
        ];
        let out = refresh_list(&[], &fresh, now, TTL);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].number, 1);
    }

    #[test]
    fn refresh_truncates_to_limit_sorted_by_recency() {
        let now = parse_iso8601_epoch_ms("2026-06-10T00:00:00Z").unwrap();
        let fresh: Vec<CachedPr> = (1..=15)
            .map(|n| pr(n, &format!("2026-06-09T{:02}:00:00Z", n), "sha"))
            .collect();
        let out = refresh_list(&[], &fresh, now, TTL);
        assert_eq!(out.len(), NEAREST_PR_LIMIT);
        // Most recently updated first.
        assert_eq!(out[0].number, 15);
        assert_eq!(out[9].number, 6);
    }

    #[test]
    fn merged_or_closed_prs_are_evicted() {
        // Merged/closed PRs are filtered out of `fresh` by the caller; an
        // entry present in `old` but absent from `fresh` must disappear.
        let now = parse_iso8601_epoch_ms("2026-06-10T00:00:00Z").unwrap();
        let mut merged = pr(1, "2026-06-09T00:00:00Z", "aaa");
        merged.diff_hash = Some("hash-1".to_string());
        let old = vec![merged, pr(2, "2026-06-09T01:00:00Z", "bbb")];
        let fresh = vec![pr(2, "2026-06-09T02:00:00Z", "bbb")];
        let out = refresh_list(&old, &fresh, now, TTL);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].number, 2);
    }

    #[test]
    fn diff_hash_carried_forward_when_head_oid_unchanged() {
        let now = parse_iso8601_epoch_ms("2026-06-10T00:00:00Z").unwrap();
        let mut old_entry = pr(1, "2026-06-08T00:00:00Z", "aaa");
        old_entry.diff_hash = Some("cached-hash".to_string());
        // Same head SHA, newer updated_at (e.g. a comment landed).
        let fresh = vec![pr(1, "2026-06-09T00:00:00Z", "aaa")];
        let out = refresh_list(&[old_entry], &fresh, now, TTL);
        assert_eq!(out[0].diff_hash.as_deref(), Some("cached-hash"));
    }

    #[test]
    fn diff_hash_cleared_when_head_oid_changed() {
        let now = parse_iso8601_epoch_ms("2026-06-10T00:00:00Z").unwrap();
        let mut old_entry = pr(1, "2026-06-08T00:00:00Z", "aaa");
        old_entry.diff_hash = Some("cached-hash".to_string());
        // New commits pushed: head SHA changed.
        let fresh = vec![pr(1, "2026-06-09T00:00:00Z", "new-sha")];
        let out = refresh_list(&[old_entry], &fresh, now, TTL);
        assert_eq!(out[0].diff_hash, None, "hash must be recomputed");
    }

    #[test]
    fn diff_hash_not_carried_for_empty_head_oid() {
        let now = parse_iso8601_epoch_ms("2026-06-10T00:00:00Z").unwrap();
        let mut old_entry = pr(1, "2026-06-08T00:00:00Z", "");
        old_entry.diff_hash = Some("cached-hash".to_string());
        let fresh = vec![pr(1, "2026-06-09T00:00:00Z", "")];
        let out = refresh_list(&[old_entry], &fresh, now, TTL);
        assert_eq!(out[0].diff_hash, None);
    }

    #[test]
    fn update_cache_builds_both_lists() {
        let now = parse_iso8601_epoch_ms("2026-06-10T00:00:00Z").unwrap();
        let my = vec![pr(1, "2026-06-09T00:00:00Z", "aaa")];
        let review = vec![pr(2, "2026-06-09T00:00:00Z", "bbb")];
        let cache = update_cache(None, &my, &review, now, TTL);
        assert_eq!(cache.version, PR_CACHE_SCHEMA_VERSION);
        assert_eq!(cache.fetched_at_epoch_ms, now);
        assert_eq!(cache.my_prs.len(), 1);
        assert_eq!(cache.to_review.len(), 1);
        assert_eq!(cache.my_prs[0].number, 1);
        assert_eq!(cache.to_review[0].number, 2);
    }

    #[test]
    fn prs_needing_diff_hash_deduplicates_and_skips_hashed() {
        let mut hashed = pr(1, "2026-06-09T00:00:00Z", "aaa");
        hashed.diff_hash = Some("h".to_string());
        let needs = pr(2, "2026-06-09T00:00:00Z", "bbb");
        let cache = NearestPrCache {
            version: PR_CACHE_SCHEMA_VERSION,
            fetched_at_epoch_ms: 0,
            my_prs: vec![hashed, needs.clone()],
            to_review: vec![needs], // same PR appears in both lists
        };
        let pending = prs_needing_diff_hash(&cache);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].number, 2);
    }

    #[test]
    fn save_load_roundtrip_and_record_diff_hash() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let result = std::panic::catch_unwind(|| {
            let remote = "org/repo";
            let cache = update_cache(
                None,
                &[pr(7, "2026-06-09T00:00:00Z", "sha7")],
                &[],
                NOW,
                TTL,
            );
            save(remote, &cache).unwrap();

            let loaded = load(remote).unwrap().expect("cache should exist");
            assert_eq!(loaded.my_prs.len(), 1);
            assert_eq!(loaded.my_prs[0].number, 7);
            assert_eq!(loaded.my_prs[0].diff_hash, None);

            // Record a computed hash — matching head SHA updates the entry.
            assert!(record_diff_hash(remote, 7, "sha7", "deadbeef").unwrap());
            let loaded = load(remote).unwrap().unwrap();
            assert_eq!(loaded.my_prs[0].diff_hash.as_deref(), Some("deadbeef"));

            // Mismatching head SHA (new commits landed meanwhile) is ignored.
            assert!(!record_diff_hash(remote, 7, "other-sha", "ffff").unwrap());
            let loaded = load(remote).unwrap().unwrap();
            assert_eq!(loaded.my_prs[0].diff_hash.as_deref(), Some("deadbeef"));

            // Unknown PR number (not in top-N) is ignored.
            assert!(!record_diff_hash(remote, 999, "sha", "ffff").unwrap());
        });
        std::env::remove_var("ER_STORAGE_ROOT");
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn load_missing_returns_none() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        let loaded = load("org/never-seen");
        std::env::remove_var("ER_STORAGE_ROOT");
        assert!(loaded.unwrap().is_none());
    }

    #[test]
    fn cache_path_slugs_remote() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        let path = cache_path("My-Org/some.repo");
        std::env::remove_var("ER_STORAGE_ROOT");
        let s = path.to_string_lossy();
        assert!(s.contains("repos/My-Org-some.repo/pr-cache.json"), "{s}");
    }
}
