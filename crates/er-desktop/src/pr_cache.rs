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

/// PRs per repo, and when each repo's list was last fetched.
pub type PersistedPrCache = (HashMap<String, Vec<PrInfo>>, HashMap<String, u64>);

pub fn load_persisted_pr_cache() -> Result<Option<PersistedPrCache>> {
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
    if let Err(e) = crate::persist::save_json_atomic(&path, &payload) {
        log::error!(
            "[pr-cache] failed to persist PR cache at {}: {e}",
            path.display()
        );
    }
}

/// Merge fresh fetch results into the existing cache.
///
/// Successful remotes replace their old entries; failed remotes keep stale data.
/// This is a pure function — no I/O, no locks — making it straightforward to test.
pub fn merge_pr_results(
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

/// Patch a single PR's `head_oid` in place. Used by the 30s per-PR head probe
///
/// (`main.rs`) so the stale pill can light without waiting for the 10-min
/// `pr_cache` sweep. Returns `true` iff the oid actually changed (caller bumps
/// the desktop revision only then, to avoid churning polls on every tick).
///
/// Does NOT persist to disk — the next full `refresh_pr_cache` sweep persists.
/// Keeps the probe off the disk-write path so a 30s cadence is cheap.
pub fn patch_pr_head_oid(cache: &PrCacheMap, remote: &str, pr_number: u64, head_oid: &str) -> bool {
    let Some(mut guard) = cache.lock().ok() else {
        return false;
    };
    let Some(prs) = guard.get_mut(remote) else {
        return false;
    };
    let Some(pr) = prs.iter_mut().find(|p| p.number == pr_number) else {
        return false;
    };
    if pr.head_oid == head_oid {
        return false;
    }
    pr.head_oid = head_oid.to_string();
    true
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
pub async fn refresh_pr_cache(cache: &PrCacheMap, fetched_at: &PrCacheFetchedAtMap) -> Vec<String> {
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
    crate::profile_log::profile_log(
        "pr_list_refresh_done",
        &[
            ("remotes", remotes.len().to_string()),
            ("ms", t.elapsed().as_millis().to_string()),
        ],
    );
    failed_remotes
}

/// Every open PR is fetched, however old. A stack's lower layers are often
/// the oldest open PRs, and a single `--state all` window of the newest PRs
/// dropped them from My PRs.
const OPEN_PR_LIMIT: usize = 200;
/// Closed/merged PRs only feed Recently merged and the inbox transitions, so
/// the newest few are enough.
const CLOSED_PR_LIMIT: usize = 50;

pub async fn fetch_prs_for_remote(remote: &str) -> Option<Vec<PrInfo>> {
    let (open, closed) = tokio::join!(
        run_pr_list(remote, "open", OPEN_PR_LIMIT),
        run_pr_list(remote, "closed", CLOSED_PR_LIMIT),
    );
    Some(merge_open_and_closed(open?, closed?))
}

/// Union of the open and closed lists, one entry per PR number. A PR that
/// changed state between the two calls shows up in both; keep the entry
/// GitHub touched last.
fn merge_open_and_closed(open: Vec<PrInfo>, closed: Vec<PrInfo>) -> Vec<PrInfo> {
    let mut out: Vec<PrInfo> = Vec::with_capacity(open.len() + closed.len());
    for pr in open.into_iter().chain(closed) {
        match out.iter_mut().find(|p| p.number == pr.number) {
            Some(existing) if pr.updated_at > existing.updated_at => *existing = pr,
            Some(_) => {}
            None => out.push(pr),
        }
    }
    out
}

async fn run_pr_list(remote: &str, state: &str, limit: usize) -> Option<Vec<PrInfo>> {
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
            state,
            "--json",
            "number,title,headRefName,baseRefName,headRefOid,updatedAt,state,isDraft,author,assignees,reviewRequests,reviewDecision,mergedAt,latestReviews",
            "--limit",
            &limit.to_string(),
        ])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_pr_list(&out.stdout)
}

fn parse_pr_list(stdout: &[u8]) -> Option<Vec<PrInfo>> {
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

    let raw: Vec<Raw> = serde_json::from_slice(stdout).ok()?;
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

    fn with_state(mut pr: PrInfo, state: &str, updated_at: &str) -> PrInfo {
        pr.state = state.to_string();
        pr.updated_at = updated_at.to_string();
        pr
    }

    #[test]
    fn merge_keeps_old_open_prs_next_to_recent_closed_ones() {
        // #1506 is far older than every closed PR, which is exactly the
        // stack layer a single newest-100 window used to drop.
        let open = vec![make_pr(1506, "stack-bottom"), make_pr(1512, "stack-top")];
        let closed = vec![
            with_state(make_pr(1609, "merged"), "MERGED", "2026-09-30T08:00:00Z"),
            with_state(make_pr(1600, "closed"), "CLOSED", "2026-09-29T08:00:00Z"),
        ];
        let merged = merge_open_and_closed(open, closed);
        let numbers: Vec<u64> = merged.iter().map(|p| p.number).collect();
        assert_eq!(numbers, vec![1506, 1512, 1609, 1600]);
    }

    #[test]
    fn merge_dedupes_a_pr_that_changed_state_between_calls() {
        let open = vec![with_state(make_pr(7, "f"), "OPEN", "2026-09-30T08:00:00Z")];
        let closed = vec![with_state(
            make_pr(7, "f"),
            "MERGED",
            "2026-09-30T08:00:05Z",
        )];
        let merged = merge_open_and_closed(open, closed);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].state, "MERGED", "the later update wins");

        let open = vec![with_state(make_pr(8, "g"), "OPEN", "2026-09-30T09:00:00Z")];
        let closed = vec![with_state(
            make_pr(8, "g"),
            "CLOSED",
            "2026-09-30T08:00:00Z",
        )];
        let merged = merge_open_and_closed(open, closed);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].state, "OPEN",
            "a reopened PR keeps its newer open entry"
        );
    }

    #[test]
    fn parse_pr_list_reads_gh_json() {
        let json = br#"[{"number":1506,"title":"Stack bottom","headRefName":"a","baseRefName":"main","headRefOid":"abc","updatedAt":"2026-09-10T12:30:43Z","state":"OPEN","isDraft":false,"author":{"login":"will"},"assignees":[],"reviewRequests":[],"reviewDecision":null,"mergedAt":null,"latestReviews":[{"author":{"login":"bo"},"state":"APPROVED"}]}]"#;
        let prs = parse_pr_list(json).expect("parses");
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].head_ref, "a");
        assert_eq!(prs[0].base_ref, "main");
        assert_eq!(prs[0].author, "will");
        assert_eq!(
            prs[0].latest_reviewer_states,
            vec![("bo".to_string(), "APPROVED".to_string())]
        );
        assert!(parse_pr_list(b"not json").is_none());
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

    // ── patch_pr_head_oid (30s probe writer) ──

    fn pr_cache_with(pr: PrInfo) -> PrCacheMap {
        Arc::new(Mutex::new(HashMap::from([(
            "org/repo".to_string(),
            vec![pr],
        )])))
    }

    #[test]
    fn patch_pr_head_oid_updates_matching_pr() {
        let mut pr = make_pr(42, "feature");
        pr.head_oid = "oldsha".to_string();
        let cache = pr_cache_with(pr);

        let changed = patch_pr_head_oid(&cache, "org/repo", 42, "newsha");

        assert!(changed, "oid differed → should report changed");
        assert_eq!(cache.lock().unwrap()["org/repo"][0].head_oid, "newsha");
    }

    #[test]
    fn patch_pr_head_oid_noop_when_oid_equal() {
        let mut pr = make_pr(42, "feature");
        pr.head_oid = "same".to_string();
        let cache = pr_cache_with(pr);

        let changed = patch_pr_head_oid(&cache, "org/repo", 42, "same");

        assert!(
            !changed,
            "oid unchanged → should report no change (no revision bump)"
        );
        assert_eq!(cache.lock().unwrap()["org/repo"][0].head_oid, "same");
    }

    #[test]
    fn patch_pr_head_oid_noop_when_remote_missing() {
        let cache: PrCacheMap = Arc::new(Mutex::new(HashMap::new()));

        let changed = patch_pr_head_oid(&cache, "org/repo", 42, "newsha");

        assert!(!changed, "remote not in cache → nothing to patch");
    }

    #[test]
    fn patch_pr_head_oid_noop_when_pr_missing() {
        // Cache knows the remote but a different PR number.
        let cache = pr_cache_with(make_pr(99, "other"));

        let changed = patch_pr_head_oid(&cache, "org/repo", 42, "newsha");

        assert!(!changed, "PR not in cache → nothing to patch (don't guess)");
        assert_eq!(cache.lock().unwrap()["org/repo"][0].number, 99);
        assert_eq!(cache.lock().unwrap()["org/repo"][0].head_oid, "");
    }

    #[test]
    fn new_remote_added_when_successful() {
        let mut cache = HashMap::new();

        let results = vec![("org/new".to_string(), Some(vec![make_pr(5, "branch-5")]))];
        merge_pr_results(&mut cache, results);

        assert_eq!(cache["org/new"].len(), 1);
        assert_eq!(cache["org/new"][0].number, 5);
        assert_eq!(cache["org/new"][0].head_ref, "branch-5");
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
