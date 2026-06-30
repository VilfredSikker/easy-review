use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::snapshot::GithubStatusSnapshot;

/// Live GitHub status cache: keyed by `(repo slug, branch, pr number)`.
type GithubStatusCache = HashMap<(String, String, u64), GithubStatusSnapshot>;

const GH_STATUS_CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedGhStatusFile {
    version: u32,
    entries: Vec<PersistedGhStatusEntry>,
}

/// A `HashMap` with a tuple key can't serialize to a JSON object (keys must be
/// strings), so each entry is persisted as a flat `{ key, entry }` record where
/// `key` serializes as a JSON array. The map is rebuilt from these records on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedGhStatusEntry {
    key: (String, String, u64),
    entry: GithubStatusSnapshot,
}

fn gh_status_cache_path() -> Option<PathBuf> {
    let dir = dirs::config_dir()?.join("er");
    Some(dir.join("gh-status-cache.json"))
}

/// Build the persisted records from the live cache. Pure (no I/O, no locks) so
/// the serialisation logic is testable. Status snapshots are small, so no size
/// guard is applied — every entry is persisted.
fn build_persisted_entries(cache: &GithubStatusCache) -> Vec<PersistedGhStatusEntry> {
    cache
        .iter()
        .map(|(key, entry)| PersistedGhStatusEntry {
            key: key.clone(),
            entry: entry.clone(),
        })
        .collect()
}

/// Parse persisted JSON into the live cache map. Pure (no I/O) so the version
/// gate is testable. A wrong/missing version yields `None` (treated as no cache).
fn parse_persisted(content: &str) -> Option<GithubStatusCache> {
    let parsed: PersistedGhStatusFile = serde_json::from_str(content).ok()?;
    if parsed.version != GH_STATUS_CACHE_SCHEMA_VERSION {
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

/// Load the persisted GitHub-status cache. Missing file, version mismatch, or a
/// corrupt file all yield `Ok(None)` so a bad file never crashes startup.
pub fn load_persisted_gh_status_cache() -> Result<Option<GithubStatusCache>> {
    let Some(path) = gh_status_cache_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(parse_persisted(&content))
}

/// Persist the GitHub-status cache. Writes atomically via a tmp file + rename.
/// Best-effort: failures are silent.
pub fn save_persisted_gh_status_cache(cache: &Arc<Mutex<GithubStatusCache>>) {
    let Some(path) = gh_status_cache_path() else {
        return;
    };
    let cache_map = cache.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let payload = PersistedGhStatusFile {
        version: GH_STATUS_CACHE_SCHEMA_VERSION,
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

/// Drop every entry whose key is not in `keep`. The (owner, repo) comparison is
/// case-insensitive: cache keys preserve the casing of the URL a PR was opened
/// with (`tab.remote_repo` is built from the raw `PrRef`), whereas `keep` is
/// derived from `project.remote`, which is always lowercased at registration
/// (`normalize_remote_slug`). GitHub treats owner/repo case-insensitively, so
/// the lowercased pair is the canonical PR identity — without this folding, a PR
/// opened via a mixed-case URL would be evicted on every restart. `keep` MUST
/// therefore contain lowercased (owner, repo) tuples (the PR number is exact).
///
/// Intended for startup pruning: callers build `keep` from each project's saved
/// PRs plus its top-10 recently opened PRs, so entries for deleted or long-idle
/// PRs age out automatically.
pub fn prune_gh_status_cache(map: &mut GithubStatusCache, keep: &HashSet<(String, String, u64)>) {
    map.retain(|(owner, repo, number), _| {
        keep.contains(&(
            owner.to_ascii_lowercase(),
            repo.to_ascii_lowercase(),
            *number,
        ))
    });
}

/// True only when `last_updated` parses as a u64 epoch-seconds string AND is
/// less than `ttl_secs` old relative to `now_secs`. Fails open — `None`,
/// non-numeric, or any other unparseable value returns `false` (= "stale, go
/// fetch"), never panics, never silently treats unknown freshness as fresh.
///
/// `now_secs.saturating_sub(parsed)` means a `last_updated` that is in the
/// future (clock skew) reads as age 0, i.e. "fresh" — both sides come from
/// `SystemTime::now()` on the same machine, so this is intentionally
/// permissive rather than a correctness gap worth guarding against here.
pub fn status_is_fresh(last_updated: Option<&str>, now_secs: u64, ttl_secs: u64) -> bool {
    let Some(raw) = last_updated else {
        return false;
    };
    let Ok(parsed) = raw.parse::<u64>() else {
        return false;
    };
    now_secs.saturating_sub(parsed) < ttl_secs
}

/// Current time as epoch seconds, in the same format `fetch_github_status`
/// writes to `GithubStatusSnapshot.last_updated` (`commands.rs:642-645`).
/// `now_secs` is a parameter on `status_is_fresh` (not called internally) so
/// callers can pass a fixed value in tests; this just centralizes the
/// `SystemTime` boilerplate for the three real call sites.
pub fn now_epoch_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(owner: &str, repo: &str, number: u64) -> GithubStatusSnapshot {
        GithubStatusSnapshot {
            owner: owner.to_string(),
            repo: repo.to_string(),
            number,
            url: format!("https://github.com/{owner}/{repo}/pull/{number}"),
            state: "OPEN".to_string(),
            is_draft: false,
            title: format!("PR #{number}"),
            body: "## Summary\ntest PR".to_string(),
            author: "alice".to_string(),
            head_ref: "feature-branch".to_string(),
            base_ref: "main".to_string(),
            review_decision: Some("REVIEW_REQUIRED".to_string()),
            mergeable: Some("MERGEABLE".to_string()),
            labels: vec!["bug".to_string(), "priority".to_string()],
            checks: vec![
                crate::snapshot::CheckSummary {
                    name: "ci/build".to_string(),
                    status: "COMPLETED".to_string(),
                    conclusion: "SUCCESS".to_string(),
                    url: Some("https://ci.example.com/build/1".to_string()),
                },
                crate::snapshot::CheckSummary {
                    name: "ci/test".to_string(),
                    status: "COMPLETED".to_string(),
                    conclusion: "FAILURE".to_string(),
                    url: None,
                },
            ],
            comments_count: 3,
            reviews_count: 1,
            recent_comments: vec![crate::snapshot::GhCommentSummary {
                author: "bob".to_string(),
                body: "LGTM".to_string(),
                created_at: "2024-01-02T10:00:00Z".to_string(),
                url: "https://github.com/comment/1".to_string(),
            }],
            recent_reviews: vec![crate::snapshot::GhReviewSummary {
                author: "carol".to_string(),
                state: "APPROVED".to_string(),
                body: "Looks good".to_string(),
                submitted_at: "2024-01-02T11:00:00Z".to_string(),
            }],
            last_updated: Some("2024-01-02T11:00:00Z".to_string()),
            is_authored_by_me: false,
        }
    }

    #[test]
    fn round_trip_preserves_entries() {
        let mut cache: GithubStatusCache = HashMap::new();
        cache.insert(
            ("myorg".to_string(), "myrepo".to_string(), 42),
            make_snapshot("myorg", "myrepo", 42),
        );
        cache.insert(
            ("otherorg".to_string(), "otherrepo".to_string(), 7),
            make_snapshot("otherorg", "otherrepo", 7),
        );

        let payload = PersistedGhStatusFile {
            version: GH_STATUS_CACHE_SCHEMA_VERSION,
            entries: build_persisted_entries(&cache),
        };
        let json = serde_json::to_string_pretty(&payload).expect("serialize");
        let loaded = parse_persisted(&json).expect("parse");

        assert_eq!(loaded.len(), 2);

        let k42 = ("myorg".to_string(), "myrepo".to_string(), 42);
        let s42 = loaded.get(&k42).expect("entry (myorg, myrepo, 42) present");
        assert_eq!(s42.checks[0].name, "ci/build");
        assert_eq!(s42.checks[0].conclusion, "SUCCESS");
        assert_eq!(s42.recent_reviews[0].author, "carol");
        assert_eq!(s42.recent_reviews[0].state, "APPROVED");

        let k7 = ("otherorg".to_string(), "otherrepo".to_string(), 7);
        let s7 = loaded
            .get(&k7)
            .expect("entry (otherorg, otherrepo, 7) present");
        assert_eq!(s7.owner, "otherorg");
        assert_eq!(s7.number, 7);
    }

    #[test]
    fn schema_mismatch_returns_none() {
        let payload = PersistedGhStatusFile {
            version: GH_STATUS_CACHE_SCHEMA_VERSION + 99,
            entries: Vec::new(),
        };
        let json = serde_json::to_string_pretty(&payload).expect("serialize");
        assert!(parse_persisted(&json).is_none());
    }

    #[test]
    fn prune_retains_only_kept() {
        let mut map: GithubStatusCache = HashMap::new();
        let k1 = ("org".to_string(), "repo".to_string(), 1);
        let k2 = ("org".to_string(), "repo".to_string(), 2);
        let k3 = ("org".to_string(), "repo".to_string(), 3);

        map.insert(k1.clone(), make_snapshot("org", "repo", 1));
        map.insert(k2.clone(), make_snapshot("org", "repo", 2));
        map.insert(k3.clone(), make_snapshot("org", "repo", 3));

        let mut keep = HashSet::new();
        keep.insert(k2.clone());

        prune_gh_status_cache(&mut map, &keep);

        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&k2), "kept key survives");
        assert!(!map.contains_key(&k1), "unpruned key 1 removed");
        assert!(!map.contains_key(&k3), "unpruned key 3 removed");
    }

    #[test]
    fn prune_matches_case_insensitively() {
        // Cache keys preserve the casing of the URL a PR was opened with
        // (`tab.remote_repo` is built from the raw `PrRef`), so a mixed-case slug
        // lands a mixed-case key in the cache.
        let mut map: GithubStatusCache = HashMap::new();
        let mixed = ("MyOrg".to_string(), "MyRepo".to_string(), 5);
        let other = ("OtherOrg".to_string(), "OtherRepo".to_string(), 9);
        map.insert(mixed.clone(), make_snapshot("MyOrg", "MyRepo", 5));
        map.insert(other.clone(), make_snapshot("OtherOrg", "OtherRepo", 9));

        // The keep-set comes from `project.remote`, always lowercased at
        // registration (`normalize_remote_slug`). Only PR #5 is kept.
        let mut keep = HashSet::new();
        keep.insert(("myorg".to_string(), "myrepo".to_string(), 5));

        prune_gh_status_cache(&mut map, &keep);

        assert_eq!(
            map.len(),
            1,
            "case-insensitive match retains the mixed-case key"
        );
        assert!(
            map.contains_key(&mixed),
            "mixed-case cache key kept despite lowercase keep-set"
        );
        assert!(
            !map.contains_key(&other),
            "unkept entry dropped even though it is also mixed-case"
        );
    }

    // ── status_is_fresh ──

    #[test]
    fn status_is_fresh_within_ttl() {
        assert!(status_is_fresh(Some("1000"), 1050, 90)); // 50s old, ttl 90s
    }

    #[test]
    fn status_is_fresh_stale_beyond_ttl() {
        assert!(!status_is_fresh(Some("1000"), 1200, 90)); // 200s old, ttl 90s
    }

    #[test]
    fn status_is_fresh_none_is_stale() {
        assert!(!status_is_fresh(None, 1000, 90));
    }

    #[test]
    fn status_is_fresh_non_numeric_is_stale() {
        // The ISO-string shape from the (unrepresentative) test fixture above —
        // confirms a format mismatch fails open rather than panicking.
        assert!(!status_is_fresh(Some("2024-01-02T11:00:00Z"), 1000, 90));
    }

    #[test]
    fn status_is_fresh_exactly_at_ttl_boundary_is_stale() {
        // age == ttl_secs exactly → `<` excludes it → stale, not fresh.
        assert!(!status_is_fresh(Some("1000"), 1090, 90));
    }

    #[test]
    fn status_is_fresh_one_second_inside_boundary_is_fresh() {
        assert!(status_is_fresh(Some("1000"), 1089, 90));
    }
}
