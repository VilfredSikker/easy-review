use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::commands::{PrOpenCacheEntry, PrOpenCacheKey};
use anyhow::Result;

const PR_OPEN_CACHE_SCHEMA_VERSION: u32 = 1;

/// Per-entry cap on the `raw_diff` we persist to disk. Entries above this stay
/// in the in-memory cache but are skipped when writing the on-disk file — a big
/// PR re-fetches on open rather than bloating the cache and slowing startup load.
const PR_OPEN_CACHE_MAX_PERSISTED_DIFF_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedPrOpenFile {
    version: u32,
    entries: Vec<PersistedPrOpenEntry>,
}

/// A HashMap with a struct key can't serialize to JSON (object keys must be
/// strings), so each entry is persisted as a flat `{ key, entry }` record and
/// the map is rebuilt on load.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedPrOpenEntry {
    key: PrOpenCacheKey,
    entry: PrOpenCacheEntry,
}

fn pr_open_cache_path() -> Option<PathBuf> {
    let dir = dirs::config_dir()?.join("er");
    Some(dir.join("pr-open-cache.json"))
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

/// Parse persisted JSON into the live cache map. Pure (no I/O) so the version
/// gate is testable. A wrong/missing version yields None (treated as no cache).
fn parse_persisted(content: &str) -> Option<HashMap<PrOpenCacheKey, PrOpenCacheEntry>> {
    let parsed: PersistedPrOpenFile = serde_json::from_str(content).ok()?;
    if parsed.version != PR_OPEN_CACHE_SCHEMA_VERSION {
        return None;
    }
    Some(
        parsed
            .entries
            .into_iter()
            .map(|e| (e.key, e.entry))
            .collect(),
    )
}

/// Load the persisted open-diff cache. Missing file, version mismatch, or a
/// corrupt/old file all yield Ok(None) so a bad file never crashes startup.
pub fn load_persisted_pr_open_cache() -> Result<Option<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>> {
    let Some(path) = pr_open_cache_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(parse_persisted(&content))
}

/// Persist the open-diff cache, skipping oversized entries (size guard). Writes
/// atomically via a tmp file + rename. Best-effort: failures are silent.
pub fn save_persisted_pr_open_cache(cache: &Arc<Mutex<HashMap<PrOpenCacheKey, PrOpenCacheEntry>>>) {
    let Some(path) = pr_open_cache_path() else {
        return;
    };
    let cache_map = cache.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let payload = PersistedPrOpenFile {
        version: PR_OPEN_CACHE_SCHEMA_VERSION,
        entries: build_persisted_entries(&cache_map),
    };
    let Ok(json) = serde_json::to_string_pretty(&payload) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
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
        let mut cache: HashMap<PrOpenCacheKey, PrOpenCacheEntry> = HashMap::new();
        cache.insert(
            make_key(11),
            make_entry("oid-11", "diff for 11".to_string()),
        );
        cache.insert(
            make_key(22),
            make_entry("oid-22", "diff for 22".to_string()),
        );

        let payload = PersistedPrOpenFile {
            version: PR_OPEN_CACHE_SCHEMA_VERSION,
            entries: build_persisted_entries(&cache),
        };
        let json = serde_json::to_string_pretty(&payload).expect("serialize");
        let loaded = parse_persisted(&json).expect("parse");

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
    }

    #[test]
    fn schema_mismatch_returns_none() {
        let payload = PersistedPrOpenFile {
            version: PR_OPEN_CACHE_SCHEMA_VERSION + 99,
            entries: Vec::new(),
        };
        let json = serde_json::to_string_pretty(&payload).expect("serialize");
        assert!(parse_persisted(&json).is_none());
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

        let payload = PersistedPrOpenFile {
            version: PR_OPEN_CACHE_SCHEMA_VERSION,
            entries: build_persisted_entries(&cache),
        };
        let json = serde_json::to_string_pretty(&payload).expect("serialize");
        let loaded = parse_persisted(&json).expect("parse");

        assert_eq!(loaded.len(), 1, "only the small entry should persist");
        assert!(loaded.contains_key(&make_key(1)), "small entry survives");
        assert!(
            !loaded.contains_key(&make_key(2)),
            "oversized entry is skipped"
        );
    }
}
