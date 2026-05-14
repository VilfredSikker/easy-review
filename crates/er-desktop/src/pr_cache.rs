use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::projects;
use crate::snapshot::PrInfo;

pub type PrCacheMap = Arc<Mutex<HashMap<String, Vec<PrInfo>>>>;

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

/// Refresh PRs for every project with a remote. Fetches all remotes in parallel.
/// Preserves stale cache entries for remotes that fail.
pub(crate) async fn refresh_pr_cache(cache: &PrCacheMap) {
    let file = projects::load();
    let remotes: Vec<String> = file
        .projects
        .iter()
        .filter_map(|p| p.remote.clone())
        .collect();

    if remotes.is_empty() {
        return;
    }

    let t = std::time::Instant::now();

    let handles: Vec<_> = remotes
        .iter()
        .cloned()
        .map(|remote| {
            tokio::spawn(async move {
                let rt = std::time::Instant::now();
                let result = fetch_prs_for_remote(&remote).await;
                (remote, result, rt.elapsed().as_millis())
            })
        })
        .collect();

    let mut results: Vec<(String, Option<Vec<PrInfo>>)> = Vec::new();
    for handle in handles {
        if let Ok((remote, result, ms)) = handle.await {
            if let Some(ref prs) = result {
                log::info!("pr_list fetch {} PRs from {} in {}ms", prs.len(), remote, ms);
            } else {
                log::warn!("pr_list fetch failed for {} after {}ms", remote, ms);
            }
            results.push((remote, result));
        }
    }

    if let Ok(mut guard) = cache.lock() {
        merge_pr_results(&mut guard, results);
    }
    log::info!(
        "pr_list refresh done ({} remotes) in {}ms",
        remotes.len(),
        t.elapsed().as_millis()
    );
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
            "number,title,headRefName,state,isDraft,author,assignees,reviewRequests,reviewDecision,mergedAt,latestReviews",
            "--limit",
            "25",
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
            latest_reviewer_states: vec![],
        }
    }

    #[test]
    fn successful_fetch_replaces_remote_entry() {
        let mut cache = HashMap::new();
        cache.insert("org/old".to_string(), vec![make_pr(1, "feature-old")]);

        let results = vec![
            ("org/old".to_string(), Some(vec![make_pr(2, "feature-new")])),
        ];
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

        let results = vec![
            ("org/new".to_string(), Some(vec![make_pr(5, "branch-5")])),
        ];
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
}
