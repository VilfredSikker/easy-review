use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::projects;
use crate::snapshot::{GhUser, PrInfo};
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

// ── Nearest-PR cache (issue #70) ────────────────────────────────────────────
//
// On top of the per-remote full PR list above, we persist the top-10 most
// recent PRs of "My PRs" and "To Review" (already split, so no gh user lookup
// is needed to serve them) into the managed storage area via
// `er_engine::pr_cache`. The sidebar reads them as an instant fallback while
// the live cache / gh user are still resolving, and PR branch checkout works
// from the cached head_ref without a `gh pr view` round-trip.

/// How many missing diff hashes to backfill per refresh cycle. Each backfill
/// costs one `gh pr diff` fetch, so keep the per-cycle budget small — unchanged
/// head SHAs never recompute, so the lists converge after a few cycles.
const DIFF_HASH_BACKFILL_PER_CYCLE: usize = 3;

/// "My PRs": open and authored by the current user.
pub(crate) fn pr_is_mine(pr: &PrInfo, me: &str) -> bool {
    pr.state == "OPEN" && pr.author == me
}

/// "To Review": open, not mine, and I haven't approved / requested changes.
/// Mirrors the sidebar derivation in `snapshot::build_projects_from_file`.
pub(crate) fn pr_needs_my_review(pr: &PrInfo, me: &str) -> bool {
    pr.state == "OPEN"
        && pr.author != me
        && !pr
            .latest_reviewer_states
            .iter()
            .any(|(l, s)| l == me && (s == "APPROVED" || s == "CHANGES_REQUESTED"))
}

fn cached_pr_from_info(pr: &PrInfo, remote: &str) -> er_engine::pr_cache::CachedPr {
    er_engine::pr_cache::CachedPr {
        number: pr.number,
        title: pr.title.clone(),
        head_ref: pr.head_ref.clone(),
        base_ref: pr.base_ref.clone(),
        head_oid: pr.head_oid.clone(),
        updated_at: pr.updated_at.clone(),
        author: pr.author.clone(),
        url: format!("https://github.com/{}/pull/{}", remote, pr.number),
        is_draft: pr.is_draft,
        diff_hash: None,
    }
}

fn pr_info_from_cached(pr: &er_engine::pr_cache::CachedPr) -> PrInfo {
    PrInfo {
        number: pr.number,
        title: pr.title.clone(),
        head_ref: pr.head_ref.clone(),
        state: "OPEN".to_string(),
        is_draft: pr.is_draft,
        author: pr.author.clone(),
        assignees: Vec::new(),
        reviewers: Vec::new(),
        checks_state: None,
        review_decision: None,
        merged_at: None,
        approved_by_me: false,
        base_ref: pr.base_ref.clone(),
        head_oid: pr.head_oid.clone(),
        updated_at: pr.updated_at.clone(),
        // Served straight from the persistent cache — checkout-ready by definition.
        cached: true,
        latest_reviewer_states: Vec::new(),
    }
}

/// Head SHAs of the PRs currently in the persistent nearest-PR cache, keyed by
/// PR number. The snapshot builder uses this to mark live "My PRs" /
/// "To Review" rows as checkout-ready (`PrInfo.cached`) when the cached entry
/// still matches the live head SHA.
pub(crate) fn cached_head_oids(remote: &str) -> HashMap<u64, String> {
    let Ok(Some(cache)) = er_engine::pr_cache::load(remote) else {
        return HashMap::new();
    };
    cache
        .my_prs
        .iter()
        .chain(cache.to_review.iter())
        .map(|pr| (pr.number, pr.head_oid.clone()))
        .collect()
}

fn nearest_pr_ttl_ms() -> u64 {
    er_engine::pr_cache::ttl_ms_from_days(er_engine::config::load_global_config().pr_cache.ttl_days)
}

/// Persist the top-10 "My PRs" / "To Review" entries for a remote. Carries
/// cached diff hashes forward for unchanged head SHAs; merged/closed/stale PRs
/// drop out (see `er_engine::pr_cache::update_cache`). No-op until the gh user
/// is known — the lists cannot be split without it.
pub(crate) fn persist_nearest_prs(remote: &str, prs: &[PrInfo], gh_user: &GhUser) {
    let Some(me) = gh_user.lock().ok().and_then(|g| g.clone()) else {
        return;
    };
    let my: Vec<er_engine::pr_cache::CachedPr> = prs
        .iter()
        .filter(|pr| pr_is_mine(pr, &me))
        .map(|pr| cached_pr_from_info(pr, remote))
        .collect();
    let to_review: Vec<er_engine::pr_cache::CachedPr> = prs
        .iter()
        .filter(|pr| pr_needs_my_review(pr, &me))
        .map(|pr| cached_pr_from_info(pr, remote))
        .collect();

    let old = match er_engine::pr_cache::load(remote) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("nearest-PR cache load failed for {remote}: {e}");
            None
        }
    };
    let updated = er_engine::pr_cache::update_cache(
        old.as_ref(),
        &my,
        &to_review,
        er_engine::pr_cache::now_epoch_ms(),
        nearest_pr_ttl_ms(),
    );
    if let Err(e) = er_engine::pr_cache::save(remote, &updated) {
        log::warn!("nearest-PR cache save failed for {remote}: {e}");
    }
}

/// Serve the persisted nearest-PR lists for a remote (stale-while-revalidate
/// read path). Applies the TTL at read time too, so a cache left on disk while
/// the app was closed doesn't resurrect long-stale PRs. Returns
/// `(my_prs, prs_to_review)` or `None` when no usable cache exists.
pub(crate) fn nearest_prs_fallback(
    remote: &str,
    dismissed: &[u64],
) -> Option<(Vec<PrInfo>, Vec<PrInfo>)> {
    let cache = er_engine::pr_cache::load(remote).ok().flatten()?;
    let now_ms = er_engine::pr_cache::now_epoch_ms();
    let ttl_ms = nearest_pr_ttl_ms();
    let convert = |list: &[er_engine::pr_cache::CachedPr]| -> Vec<PrInfo> {
        er_engine::pr_cache::refresh_list(&[], list, now_ms, ttl_ms)
            .iter()
            .filter(|pr| !dismissed.contains(&pr.number))
            .map(pr_info_from_cached)
            .collect()
    };
    let my = convert(&cache.my_prs);
    let to_review = convert(&cache.to_review);
    if my.is_empty() && to_review.is_empty() {
        return None;
    }
    Some((my, to_review))
}

/// Compute missing diff hashes for a remote's nearest PRs, bounded per cycle.
/// A hash is only recomputed when `update_cache` cleared it — i.e. the head
/// SHA changed (or it was never computed) and the PR is still top-10 recent.
async fn backfill_missing_diff_hashes(remote: &str) {
    let Ok(Some(cache)) = er_engine::pr_cache::load(remote) else {
        return;
    };
    let pending = er_engine::pr_cache::prs_needing_diff_hash(&cache);
    let Some((owner, repo)) = remote.split_once('/') else {
        return;
    };
    for pr in pending.into_iter().take(DIFF_HASH_BACKFILL_PER_CYCLE) {
        if pr.head_oid.is_empty() {
            continue;
        }
        let (owner, repo) = (owner.to_string(), repo.to_string());
        let number = pr.number;
        let t = std::time::Instant::now();
        let hash = tokio::task::spawn_blocking(move || {
            er_engine::github::gh_pr_diff_hash_remote(&owner, &repo, number)
        })
        .await;
        match hash {
            Ok(Ok(hash)) => {
                crate::profile_log::profile_log(
                    "pr_diff_hash_backfill",
                    &[
                        ("remote", remote.to_string()),
                        ("pr", number.to_string()),
                        ("ms", t.elapsed().as_millis().to_string()),
                    ],
                );
                // Ignored when newer commits landed mid-fetch (head_oid moved).
                if let Err(e) =
                    er_engine::pr_cache::record_diff_hash(remote, number, &pr.head_oid, &hash)
                {
                    log::warn!("recording diff hash for {remote}#{number} failed: {e}");
                }
            }
            Ok(Err(e)) => {
                log::warn!("diff hash backfill failed for {remote}#{number}: {e}");
            }
            Err(e) => {
                log::warn!("diff hash backfill task panicked for {remote}#{number}: {e}");
            }
        }
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
    remotes
        .iter()
        .any(|remote| match guard.as_ref().and_then(|g| g.get(remote)) {
            None => true,
            Some(ts) => now.saturating_sub(*ts) > PR_CACHE_STARTUP_MAX_AGE_MS,
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
    gh_user: &GhUser,
) -> bool {
    let t = std::time::Instant::now();
    let result = fetch_prs_for_remote(remote).await;
    let ms = t.elapsed().as_millis();
    let success = result.is_some();
    if let Some(ref prs) = result {
        crate::profile_log::profile_log(
            "pr_list_fetch",
            &[
                ("count", prs.len().to_string()),
                ("remote", remote.to_string()),
                ("ms", ms.to_string()),
            ],
        );
        persist_nearest_prs(remote, prs, gh_user);
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
    if success {
        backfill_missing_diff_hashes(remote).await;
    }
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
    gh_user: &GhUser,
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
                crate::profile_log::profile_log(
                    "pr_list_fetch",
                    &[
                        ("count", prs.len().to_string()),
                        ("remote", remote.clone()),
                        ("ms", ms.to_string()),
                    ],
                );
                persist_nearest_prs(&remote, prs, gh_user);
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
        for remote in &refreshed_remotes {
            fetched_guard.insert(remote.clone(), ts);
        }
    }
    save_persisted_pr_cache(cache, fetched_at);
    for remote in &refreshed_remotes {
        backfill_missing_diff_hashes(remote).await;
    }
    crate::profile_log::profile_log(
        "pr_list_refresh_done",
        &[
            ("remotes", remotes.len().to_string()),
            ("ms", t.elapsed().as_millis().to_string()),
        ],
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
                    cached: false, // marked in build_projects() from the persisted cache
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
            cached: false,
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
    fn pr_is_mine_requires_open_and_authored_by_me() {
        let mut pr = make_pr(1, "feature");
        pr.author = "alice".to_string();
        assert!(pr_is_mine(&pr, "alice"));
        assert!(!pr_is_mine(&pr, "bob"));
        pr.state = "MERGED".to_string();
        assert!(!pr_is_mine(&pr, "alice"));
    }

    #[test]
    fn pr_needs_my_review_excludes_own_reviewed_and_closed() {
        let mut pr = make_pr(2, "feature");
        pr.author = "alice".to_string();
        assert!(pr_needs_my_review(&pr, "bob"));
        // Own PR never needs my review.
        assert!(!pr_needs_my_review(&pr, "alice"));
        // Already approved by me — drops out.
        pr.latest_reviewer_states = vec![("bob".to_string(), "APPROVED".to_string())];
        assert!(!pr_needs_my_review(&pr, "bob"));
        // Someone else's approval doesn't count.
        assert!(pr_needs_my_review(&pr, "carol"));
        // Changes requested by me — also drops out.
        pr.latest_reviewer_states = vec![("carol".to_string(), "CHANGES_REQUESTED".to_string())];
        assert!(!pr_needs_my_review(&pr, "carol"));
        // Closed PRs drop out regardless.
        pr.state = "CLOSED".to_string();
        assert!(!pr_needs_my_review(&pr, "bob"));
    }

    #[test]
    fn cached_pr_conversion_roundtrip_keeps_checkout_fields() {
        let mut pr = make_pr(42, "feature/foo");
        pr.base_ref = "develop".to_string();
        pr.head_oid = "abc123".to_string();
        pr.updated_at = "2026-06-09T10:00:00Z".to_string();
        pr.is_draft = true;
        let cached = cached_pr_from_info(&pr, "org/repo");
        assert_eq!(cached.url, "https://github.com/org/repo/pull/42");
        let back = pr_info_from_cached(&cached);
        assert_eq!(back.number, 42);
        assert_eq!(back.head_ref, "feature/foo");
        assert_eq!(back.base_ref, "develop");
        assert_eq!(back.head_oid, "abc123");
        assert_eq!(back.updated_at, "2026-06-09T10:00:00Z");
        assert_eq!(back.state, "OPEN");
        assert!(back.is_draft);
        assert_eq!(back.author, "alice");
        assert!(back.cached, "fallback rows come from the cache itself");
    }

    /// Serializes tests that mutate the process-wide `ER_STORAGE_ROOT` env var.
    static STORAGE_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn cached_head_oids_maps_number_to_head_sha() {
        let _guard = STORAGE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());

        let result = std::panic::catch_unwind(|| {
            let remote = "org/repo";
            assert!(cached_head_oids(remote).is_empty(), "cold cache → empty");

            let mut pr = make_pr(7, "feature/seven");
            pr.head_oid = "sha7".to_string();
            pr.updated_at = "2026-06-09T00:00:00Z".to_string();
            let cached = cached_pr_from_info(&pr, remote);
            let cache = er_engine::pr_cache::update_cache(
                None,
                &[cached],
                &[],
                er_engine::pr_cache::parse_iso8601_epoch_ms("2026-06-10T00:00:00Z").unwrap(),
                er_engine::pr_cache::ttl_ms_from_days(7),
            );
            er_engine::pr_cache::save(remote, &cache).unwrap();

            let oids = cached_head_oids(remote);
            assert_eq!(oids.get(&7).map(String::as_str), Some("sha7"));
            assert_eq!(oids.len(), 1);
        });
        std::env::remove_var("ER_STORAGE_ROOT");
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
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
                    auto_triage: false,
                    auto_triage_own_prs: false,
                    auto_triage_when: "new-and-push".to_string(),
                    auto_triage_max_diff_kb: 0,
                    review_ignore_globs: Vec::new(),
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
                    auto_triage: false,
                    auto_triage_own_prs: false,
                    auto_triage_when: "new-and-push".to_string(),
                    auto_triage_max_diff_kb: 0,
                    review_ignore_globs: Vec::new(),
                },
            ],
            active_id: None,
        };

        assert_eq!(refreshable_remotes(&file), vec!["owner/local".to_string()]);
    }
}
