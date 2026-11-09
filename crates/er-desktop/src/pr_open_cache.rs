use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::commands::{PrOpenCacheEntry, PrOpenCacheKey};
use anyhow::Result;
use er_store::{Artifact, Loaded, Mode};

/// Per-entry cap on the `raw_diff` we persist to disk. Entries above this stay
/// in the in-memory cache but are skipped when writing the on-disk file — a big
/// PR re-fetches on open rather than bloating the cache and slowing startup load.
const PR_OPEN_CACHE_MAX_PERSISTED_DIFF_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedPrOpenFile {
    entries: Vec<PersistedPrOpenEntry>,
}

/// The payload holds `PrOverviewData` and `CommitInfo` from other crates; the
/// store only needs a schema to describe the kind, so a permissive object is
/// enough and keeps the shared types free of a schema dependency.
impl schemars::JsonSchema for PersistedPrOpenFile {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PersistedPrOpenFile".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "object"})
    }
}

impl Artifact for PersistedPrOpenFile {
    const KIND: &'static str = "pr-open-cache";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Enveloped;
    const FILE: &'static str = "pr-open-cache.json";
    type Slot = ();
}

/// A HashMap with a struct key can't serialize to JSON (object keys must be
/// strings), so each entry is persisted as a flat `{ key, entry }` record and
/// the map is rebuilt on load.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedPrOpenEntry {
    key: PrOpenCacheKey,
    entry: PrOpenCacheEntry,
}

/// Build the persisted records from the live cache, applying the per-entry size
/// guard. Pure (no I/O, no locks) so the size-guard behavior is testable.
fn build_persisted_entries(
    cache: &HashMap<PrOpenCacheKey, PrOpenCacheEntry>,
) -> Vec<PersistedPrOpenEntry> {
    cache
        .iter()
        .filter(|(_, entry)| entry.raw_diff.len() <= PR_OPEN_CACHE_MAX_PERSISTED_DIFF_BYTES)
        .map(|(key, entry)| PersistedPrOpenEntry {
            key: key.clone(),
            entry: entry.clone(),
        })
        .collect()
}

/// Retain only entries whose `(project_id, pr_number)` is in `keep`. Pure
///
/// (operates on the borrowed map) so the policy is unit-testable without a lock,
/// mirroring `evict_lru` in commands.rs. Startup prune keeps the on-disk cache
/// small: callers build `keep` from each project's saved ∪ top-10-recent PRs.
/// `repo_root` is intentionally not consulted — `project_id` already scopes the
/// project, so the key's repo_root component never affects retention.
pub fn prune_pr_open_cache(
    map: &mut HashMap<PrOpenCacheKey, PrOpenCacheEntry>,
    keep: &HashSet<(String, u64)>,
) {
    map.retain(|k, _| keep.contains(&(k.project_id.clone(), k.pr_number)));
}

/// Load the persisted open-diff cache. A missing record, a version mismatch, or
/// a corrupt record all yield Ok(None) so a bad record never crashes startup.
pub fn load_persisted_pr_open_cache() -> Result<Option<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>> {
    let key = er_engine::store_registry::app_key::<PersistedPrOpenFile>()?;
    let parsed = match er_engine::store_registry::store().read(&key)? {
        Loaded::Found(stored) => stored.value,
        Loaded::Absent | Loaded::Unreadable(_) => return Ok(None),
    };
    Ok(Some(
        parsed
            .entries
            .into_iter()
            .map(|e| (e.key, e.entry))
            .collect(),
    ))
}

/// Persist the open-diff cache, skipping oversized entries (size guard).
/// Best-effort: failures are logged.
pub fn save_persisted_pr_open_cache(cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>) {
    let Ok(key) = er_engine::store_registry::app_key::<PersistedPrOpenFile>() else {
        return;
    };
    let cache_map = cache.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let payload = PersistedPrOpenFile {
        entries: build_persisted_entries(&cache_map),
    };
    if let Err(e) = er_engine::store_registry::store().write(&key, &payload) {
        log::error!("[pr-open-cache] failed to persist open-diff cache: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::PrOpenFreshness;
    use er_engine::git::CommitInfo;

    fn make_commit(short_hash: &str, subject: &str) -> CommitInfo {
        CommitInfo {
            hash: format!("{short_hash}0000000000000000000000000000000000"),
            short_hash: short_hash.to_string(),
            subject: subject.to_string(),
            author: "alice".to_string(),
            date: "2024-01-01".to_string(),
            relative_date: "1 day ago".to_string(),
            file_count: 2,
            adds: 10,
            dels: 3,
            is_merge: false,
            diffstat_known: true,
        }
    }

    fn make_key(pr_number: u64) -> PrOpenCacheKey {
        PrOpenCacheKey {
            project_id: "proj".to_string(),
            repo_root: "/tmp/repo".to_string(),
            pr_number,
        }
    }

    fn make_entry(head_oid: &str, raw_diff: String) -> PrOpenCacheEntry {
        PrOpenCacheEntry {
            freshness: PrOpenFreshness {
                base_branch: "main".to_string(),
                head_branch: "feature".to_string(),
                head_oid: head_oid.to_string(),
                updated_at: "2024-01-01T00:00:00Z".to_string(),
            },
            raw_diff,
            pr_data: None,
            pr_commits: Some(vec![
                make_commit("aaa1111", "first commit"),
                make_commit("bbb2222", "second commit"),
            ]),
            last_touched: 0,
        }
    }

    #[test]
    fn round_trip_preserves_entries() {
        let tmp = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            let mut cache: HashMap<PrOpenCacheKey, PrOpenCacheEntry> = HashMap::new();
            cache.insert(
                make_key(11),
                make_entry("oid-11", "diff for 11".to_string()),
            );
            cache.insert(
                make_key(22),
                make_entry("oid-22", "diff for 22".to_string()),
            );
            save_persisted_pr_open_cache(&Arc::new(Mutex::new(cache)));

            let loaded = load_persisted_pr_open_cache()
                .expect("load")
                .expect("a written cache");

            assert_eq!(loaded.len(), 2);

            let e11 = loaded.get(&make_key(11)).expect("entry 11 present");
            assert_eq!(e11.raw_diff, "diff for 11");
            assert_eq!(e11.freshness.head_oid, "oid-11");
            let commits = e11.pr_commits.as_ref().expect("commits present");
            assert_eq!(commits.len(), 2);
            assert_eq!(commits[0].subject, "first commit");
            assert_eq!(commits[1].subject, "second commit");

            let e22 = loaded.get(&make_key(22)).expect("entry 22 present");
            assert_eq!(e22.raw_diff, "diff for 22");
            assert_eq!(e22.freshness.head_oid, "oid-22");
        });
    }

    #[test]
    fn schema_mismatch_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            let path = er_engine::store_registry::app_path::<PersistedPrOpenFile>().expect("record path");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                path,
                r#"{"kind":"pr-open-cache","version":100,"diff_hash":null,"producer":"host","written_at":"2026-01-01T00:00:00Z","payload":{"entries":[]}}"#,
            )
            .unwrap();
            assert!(load_persisted_pr_open_cache().expect("load").is_none());
        });
    }

    #[test]
    fn size_guard_skips_oversized_entry() {
        let mut cache: HashMap<PrOpenCacheKey, PrOpenCacheEntry> = HashMap::new();
        cache.insert(
            make_key(1),
            make_entry("small-oid", "tiny diff".to_string()),
        );
        let huge = "x".repeat(PR_OPEN_CACHE_MAX_PERSISTED_DIFF_BYTES + 1);
        cache.insert(make_key(2), make_entry("huge-oid", huge));

        let entries = build_persisted_entries(&cache);
        let loaded: HashMap<PrOpenCacheKey, PrOpenCacheEntry> =
            entries.into_iter().map(|e| (e.key, e.entry)).collect();

        assert_eq!(loaded.len(), 1, "only the small entry should persist");
        assert!(loaded.contains_key(&make_key(1)), "small entry survives");
        assert!(
            !loaded.contains_key(&make_key(2)),
            "oversized entry is skipped"
        );
    }

    #[test]
    fn prune_retains_only_kept() {
        let mut map: HashMap<PrOpenCacheKey, PrOpenCacheEntry> = HashMap::new();
        map.insert(make_key(11), make_entry("oid-11", "diff 11".to_string()));
        map.insert(make_key(22), make_entry("oid-22", "diff 22".to_string()));
        map.insert(make_key(33), make_entry("oid-33", "diff 33".to_string()));

        let keep: HashSet<(String, u64)> = [("proj".to_string(), 11), ("proj".to_string(), 33)]
            .into_iter()
            .collect();

        prune_pr_open_cache(&mut map, &keep);

        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&make_key(11)));
        assert!(map.contains_key(&make_key(33)));
        assert!(!map.contains_key(&make_key(22)));
    }

    #[test]
    fn prune_empty_keep_clears_all() {
        let mut map: HashMap<PrOpenCacheKey, PrOpenCacheEntry> = HashMap::new();
        map.insert(make_key(1), make_entry("oid-1", "diff 1".to_string()));
        map.insert(make_key(2), make_entry("oid-2", "diff 2".to_string()));

        let keep: HashSet<(String, u64)> = HashSet::new();

        prune_pr_open_cache(&mut map, &keep);

        assert!(map.is_empty());
    }
}
