use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::projects;
use crate::snapshot::PrInfo;
use anyhow::Result;

pub type PrCacheMap = Arc<Mutex<HashMap<String, Vec<PrInfo>>>>;
pub type PrCacheFetchedAtMap = Arc<Mutex<HashMap<String, u64>>>;

const PR_CACHE_SCHEMA_VERSION: u32 = 1;
/// If every configured remote was fetched within this window, skip the startup
/// full multi-remote sweep (the active project's remote is still refreshed first).
const PR_CACHE_STARTUP_MAX_AGE_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedPrCacheFile {
    version: u32,
    entries: Vec<PersistedPrCacheEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedPrCacheEntry {
    remote: String,
    fetched_at_epoch_ms: u64,
    prs: Vec<PrInfo>,
}

fn pr_cache_path() -> Option<PathBuf> {
    let dir = dirs::config_dir()?.join("er");
    Some(dir.join("pr-cache.json"))
}

fn now_epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[allow(clippy::type_complexity)]
pub fn load_persisted_pr_cache(
) -> Result<Option<(HashMap<String, Vec<PrInfo>>, HashMap<String, u64>)>> {
    let Some(path) = pr_cache_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let parsed: PersistedPrCacheFile = serde_json::from_str(&content)?;
    if parsed.version != PR_CACHE_SCHEMA_VERSION {
        return Ok(None);
    }
    let mut pr_map: HashMap<String, Vec<PrInfo>> = HashMap::new();
    let mut fetched_map: HashMap<String, u64> = HashMap::new();
    for entry in parsed.entries {
        pr_map.insert(entry.remote.clone(), entry.prs);
        fetched_map.insert(entry.remote, entry.fetched_at_epoch_ms);
    }
    Ok(Some((pr_map, fetched_map)))
}

pub fn save_persisted_pr_cache(cache: &PrCacheMap, fetched_at: &PrCacheFetchedAtMap) {
    let Some(path) = pr_cache_path() else {
        return;
    };
    let cache_map = cache.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let fetched_map = fetched_at
        .lock()
        .ok()
        .map(|g| g.clone())
        .unwrap_or_default();
    let entries: Vec<PersistedPrCacheEntry> = cache_map
        .into_iter()
        .map(|(remote, prs)| PersistedPrCacheEntry {
            fetched_at_epoch_ms: fetched_map.get(&remote).copied().unwrap_or(0),
            remote,
            prs,
        })
        .collect();
    let payload = PersistedPrCacheFile {
        version: PR_CACHE_SCHEMA_VERSION,
        entries,
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

/// Merge fresh fetch results into the existing cache.
/// Successful remotes replace their old entries; failed remotes keep stale data.
/// This is a pure function — no I/O, no locks — making it straightforward to test.
pub(crate) fn merge_pr_results(
    existing: &mut HashMap<String, Vec<PrInfo>>,
    results: Vec<(String, Option<Vec<PrInfo>>)>,
) {
    for (remote, prs) in results {
        if let Some(prs) = prs {
            existing.insert(remote, prs);
        }
        // On failure: leave the existing entry untouched (stale is better than gone)
    }
}

/// Whether the startup full PR sweep should run (any configured remote missing
/// or older than [`PR_CACHE_STARTUP_MAX_AGE_MS`]).
pub fn startup_full_refresh_due(fetched_at: &PrCacheFetchedAtMap) -> bool {
    let file = projects::load();
    let remotes = refreshable_remotes(&file);
    if remotes.is_empty() {
        return false;
    }
    let now = now_epoch_ms();
    let guard = fetched_at.lock().ok();
    remotes.iter().any(|remote| {
        match guard.as_ref().and_then(|g| g.get(remote)) {
            None => true,
            Some(ts) => now.saturating_sub(*ts) > PR_CACHE_STARTUP_MAX_AGE_MS,
        }
    })
}

/// Return the remote slug for the currently-active project, if any.
pub fn active_project_remote() -> Option<String> {
    let file = projects::load();
    let active_id = file.active_id.as_ref()?;
    file.projects
        .iter()
        .find(|p| &p.id == active_id)
        .and_then(|p| p.remote.clone())
        .filter(|s| !s.is_empty())
}

/// Refresh a single remote and merge into the cache. Used at startup so the
/// active project's PR list is hot before the full multi-remote sweep runs.
pub async fn refresh_pr_cache_for_remote(
    remote: &str,
    cache: &PrCacheMap,
    fetched_at: &PrCacheFetchedAtMap,
) -> bool {
    let t = std::time::Instant::now();
    let result = fetch_prs_for_remote(remote).await;
    let ms = t.elapsed().as_millis();
    let success = result.is_some();
    if let Some(ref prs) = result {
        log::info!("pr_list fetch {} PRs from {} in {}ms", prs.len(), remote, ms);
    } else {
        log::warn!("pr_list fetch failed for {} after {}ms", remote, ms);
    }
    if let Ok(mut guard) = cache.lock() {
        merge_pr_results(&mut guard, vec![(remote.to_string(), result)]);
    }
    if success {
        if let Ok(mut fetched_guard) = fetched_at.lock() {
            fetched_guard.insert(remote.to_string(), now_epoch_ms());
        }
    }
    save_persisted_pr_cache(cache, fetched_at);
    success
}

fn refreshable_remotes(file: &projects::ProjectsFile) -> Vec<String> {
    file.projects
        .iter()
        .filter(|p| !p.root_path.is_empty())
        .filter_map(|p| p.remote.clone())
        .collect()
}

/// Refresh PRs for every project with a remote. Fetches all remotes in parallel.
/// Preserves stale cache entries for remotes that fail.
pub(crate) async fn refresh_pr_cache(
    cache: &PrCacheMap,
    fetched_at: &PrCacheFetchedAtMap,
) -> Vec<String> {
    let file = projects::load();
    let remotes = refreshable_remotes(&file);

    if remotes.is_empty() {
        return Vec::new();
    }

    let t = std::time::Instant::now();

    let handles: Vec<_> = remotes
        .iter()
        .map(|remote| {
            let remote = remote.clone();
            tokio::spawn(async move {
                let rt = std::time::Instant::now();
                let result = fetch_prs_for_remote(&remote).await;
                (remote, result, rt.elapsed().as_millis())
            })
        })
        .collect();

    let mut results: Vec<(String, Option<Vec<PrInfo>>)> = Vec::new();
    let mut refreshed_remotes: Vec<String> = Vec::new();
    let mut failed_remotes: Vec<String> = Vec::new();
    for handle in handles {
        if let Ok((remote, result, ms)) = handle.await {
            if let Some(ref prs) = result {
                log::info!(
                    "pr_list fetch {} PRs from {} in {}ms",
                    prs.len(),
                    remote,
                    ms
                );
                refreshed_remotes.push(remote.clone());
            } else {
                log::warn!("pr_list fetch failed for {} after {}ms", remote, ms);
                failed_remotes.push(remote.clone());
            }
            results.push((remote, result));
        }
    }

    if let Ok(mut guard) = cache.lock() {
        merge_pr_results(&mut guard, results);
    }
    if let Ok(mut fetched_guard) = fetched_at.lock() {
        let ts = now_epoch_ms();
        for remote in refreshed_remotes {
            fetched_guard.insert(remote, ts);
        }
    }
    save_persisted_pr_cache(cache, fetched_at);
    log::info!(
        "pr_list refresh done ({} remotes) in {}ms",
        remotes.len(),
        t.elapsed().as_millis()
    );
    failed_remotes
}

pub(crate) async fn fetch_prs_for_remote(remote: &str) -> Option<Vec<PrInfo>> {
    // statusCheckRollup is intentionally excluded — it forces GitHub to aggregate
    // CI checks for every PR and is the dominant cause of latency (adds ~5s per fetch).
    // Icon colors use reviewDecision instead, which is cheap.
    let out = tokio::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            remote,
            "--state",
            "all",
            "--json",
            "number,title,headRefName,baseRefName,headRefOid,updatedAt,state,isDraft,author,assignees,reviewRequests,reviewDecision,mergedAt,latestReviews",
            "--limit",
            "100",
        ])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct Raw {
        number: u64,
        title: String,
        #[serde(rename = "headRefName")]
        head_ref_name: String,
        #[serde(default, rename = "baseRefName")]
        base_ref_name: String,
        #[serde(default, rename = "headRefOid")]
        head_ref_oid: String,
        #[serde(default, rename = "updatedAt")]
        updated_at: String,
        state: String,
        #[serde(rename = "isDraft")]
        is_draft: bool,
        author: RawAuthor,
        #[serde(default)]
        assignees: Vec<RawLogin>,
        #[serde(default, rename = "reviewRequests")]
        review_requests: Vec<RawReviewRequest>,
        #[serde(default, rename = "reviewDecision")]
        review_decision: Option<String>,
        #[serde(default, rename = "mergedAt")]
        merged_at: Option<String>,
        #[serde(default, rename = "latestReviews")]
        latest_reviews: Vec<RawReview>,
    }
    #[derive(serde::Deserialize)]
    struct RawAuthor {
        login: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawLogin {
        login: String,
    }
    #[derive(serde::Deserialize)]
    struct RawReviewRequest {
        #[serde(default)]
        login: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawReview {
        author: RawAuthor,
        state: String,
    }

    let raw: Vec<Raw> = serde_json::from_slice(&out.stdout).ok()?;
    Some(
        raw.into_iter()
            .map(|r| {
                let latest_reviewer_states = r
                    .latest_reviews
                    .into_iter()
                    .filter_map(|rv| rv.author.login.map(|l| (l, rv.state)))
                    .collect();
                PrInfo {
                    number: r.number,
                    title: r.title,
                    head_ref: r.head_ref_name,
                    state: r.state,
                    is_draft: r.is_draft,
                    author: r.author.login.unwrap_or_default(),
                    assignees: r.assignees.into_iter().map(|a| a.login).collect(),
                    reviewers: r
                        .review_requests
                        .into_iter()
                        .filter_map(|rr| rr.login)
                        .collect(),
                    checks_state: None,
                    review_decision: r.review_decision,
                    merged_at: r.merged_at,
                    approved_by_me: false, // computed in build_projects()
                    base_ref: r.base_ref_name,
                    head_oid: r.head_ref_oid,
                    updated_at: r.updated_at,
                    latest_reviewer_states,
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pr(number: u64, head_ref: &str) -> PrInfo {
        PrInfo {
            number,
            title: format!("PR #{}", number),
            head_ref: head_ref.to_string(),
            state: "OPEN".to_string(),
            is_draft: false,
            author: "alice".to_string(),
            assignees: vec![],
            reviewers: vec![],
            checks_state: None,
            review_decision: None,
            merged_at: None,
            approved_by_me: false,
            base_ref: "main".to_string(),
            head_oid: String::new(),
            updated_at: String::new(),
            latest_reviewer_states: vec![],
        }
    }

    #[test]
    fn successful_fetch_replaces_remote_entry() {
        let mut cache = HashMap::new();
        cache.insert("org/old".to_string(), vec![make_pr(1, "feature-old")]);

        let results = vec![("org/old".to_string(), Some(vec![make_pr(2, "feature-new")]))];
        merge_pr_results(&mut cache, results);

        assert_eq!(cache["org/old"].len(), 1);
        assert_eq!(cache["org/old"][0].number, 2);
    }

    #[test]
    fn failed_fetch_preserves_stale_entry() {
        let mut cache = HashMap::new();
        cache.insert("org/repo".to_string(), vec![make_pr(10, "main")]);

        let results = vec![("org/repo".to_string(), None)];
        merge_pr_results(&mut cache, results);

        // stale data survives
        assert_eq!(cache["org/repo"].len(), 1);
        assert_eq!(cache["org/repo"][0].number, 10);
    }

    #[test]
    fn new_remote_added_when_successful() {
        let mut cache = HashMap::new();

        let results = vec![("org/new".to_string(), Some(vec![make_pr(5, "branch-5")]))];
        merge_pr_results(&mut cache, results);

        assert_eq!(cache["org/new"].len(), 1);
    }

    #[test]
    fn partial_failure_leaves_other_remotes_intact() {
        let mut cache = HashMap::new();
        cache.insert("org/a".to_string(), vec![make_pr(1, "a")]);
        cache.insert("org/b".to_string(), vec![make_pr(2, "b")]);

        let results = vec![
            ("org/a".to_string(), Some(vec![make_pr(3, "a-new")])), // success
            ("org/b".to_string(), None),                            // failure
        ];
        merge_pr_results(&mut cache, results);

        assert_eq!(cache["org/a"][0].number, 3, "a should be updated");
        assert_eq!(cache["org/b"][0].number, 2, "b should be preserved");
    }

    #[test]
    fn refreshable_remotes_excludes_remote_only_projects() {
        let file = projects::ProjectsFile {
            projects: vec![
                projects::ProjectRecord {
                    id: "local".to_string(),
                    name: "local".to_string(),
                    root_path: "/tmp/local".to_string(),
                    remote: Some("owner/local".to_string()),
                    dismissed_prs: Vec::new(),
                    tracked_prs: Vec::new(),
                    tracked_branches: Vec::new(),
                    dismissed_branches: Vec::new(),
                    recent_prs: Vec::new(),
                    saved_prs: Vec::new(),
                },
                projects::ProjectRecord {
                    id: "remote-owner-bun".to_string(),
                    name: "owner/bun".to_string(),
                    root_path: String::new(),
                    remote: Some("owner/bun".to_string()),
                    dismissed_prs: Vec::new(),
                    tracked_prs: Vec::new(),
                    tracked_branches: Vec::new(),
                    dismissed_branches: Vec::new(),
                    recent_prs: Vec::new(),
                    saved_prs: Vec::new(),
                },
            ],
            active_id: None,
        };

        assert_eq!(refreshable_remotes(&file), vec!["owner/local".to_string()]);
    }
}
