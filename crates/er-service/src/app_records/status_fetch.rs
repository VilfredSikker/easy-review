//! The GitHub status fetch: one PR's overview, checks, reviews and merge
//! settings, gathered for its merge box and stored in the status cache by
//! the owner.

use crate::app_records::app_owner::ChangeHook;
use crate::app_records::sidebar::{
    CheckSummary, GhCommentSummary, GhReviewSummary, GhReviewerSummary, GithubStatusSnapshot,
};

/// Fetch all GitHub status data for a PR. Runs 2 gh calls in parallel: one
///
/// combined `gh pr view --json` for overview+comments+reviews, one `gh pr
/// checks` for CI status (separate subcommand, can't be merged into `pr view`).
/// Returns None when the overview/status-bundle fetch fails (e.g. no network,
/// gh not authed). Comments/reviews parse failures inside the bundle are
/// non-fatal; checks failures are non-fatal — the snapshot still populates.
pub(crate) fn fetch_github_status(
    owner: &str,
    repo: &str,
    number: u64,
    on_change: &ChangeHook,
) -> Option<GithubStatusSnapshot> {
    let t = std::time::Instant::now();
    // The independent lookups run concurrently. The branch and merge-queue
    // lookups after this need the PR state, so they wait for the bundle.
    let (bundle_res, checks, repo_merge) = std::thread::scope(|s| {
        let b = s.spawn(|| er_engine::github::gh_pr_status_remote(owner, repo, number));
        let c = s.spawn(|| {
            er_engine::github::gh_pr_checks_remote(owner, repo, number).unwrap_or_default()
        });
        // Cached per repo, so this is usually no subprocess at all.
        let r = s.spawn(|| er_engine::gh_pr_actions::cached_repo_merge_settings(owner, repo));
        (
            b.join().ok(),
            c.join().unwrap_or_default(),
            r.join().ok().flatten(),
        )
    });
    let bundle = bundle_res?.ok()?;
    let overview = bundle.overview;
    let head_branch_exists =
        head_branch_exists_after_close(owner, repo, &overview, repo_merge.as_ref());
    let (base_has_merge_queue, in_merge_queue) = merge_queue_status(owner, repo, number, &overview);
    let can_bypass_rules = can_bypass_rules(owner, repo, number, &overview, base_has_merge_queue);
    let comments = bundle.comments;
    let reviews = bundle.reviews;
    on_change(
        "gh_status_fetch",
        &[
            ("owner", owner.to_string()),
            ("repo", repo.to_string()),
            ("number", number.to_string()),
            ("ms", t.elapsed().as_millis().to_string()),
        ],
    );

    // Most recent 5, newest last in the source — keep the trailing 5.
    let recent_comments: Vec<GhCommentSummary> = comments
        .iter()
        .rev()
        .take(5)
        .map(|c| GhCommentSummary {
            author: c.author.clone(),
            body: c.body.clone(),
            created_at: c.created_at.clone(),
            url: c.url.clone(),
        })
        .collect();
    let recent_reviews: Vec<GhReviewSummary> = reviews
        .iter()
        .rev()
        .take(5)
        .map(|r| GhReviewSummary {
            author: r.author.clone(),
            state: r.state.clone(),
            body: r.body.clone(),
            submitted_at: r.submitted_at.clone(),
        })
        .collect();

    let check_summaries: Vec<CheckSummary> = checks.into_iter().map(CheckSummary::from).collect();
    let reviewers = github_reviewers(&overview, &reviews);

    // ISO-8601-ish timestamp using only std. Format: seconds since epoch as
    // string — frontend renders as "x sec ago". Avoids pulling in chrono.
    let last_updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs().to_string());

    Some(GithubStatusSnapshot {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
        url: overview.url,
        state: overview.state,
        is_draft: overview.is_draft,
        title: overview.title,
        body: overview.body,
        author: overview.author,
        head_ref: overview.head_ref_name,
        base_ref: overview.base_ref_name,
        review_decision: overview.review_decision,
        mergeable: overview.mergeable,
        labels: overview.labels,
        checks: check_summaries,
        comments_count: comments.len(),
        reviews_count: reviews.len(),
        recent_comments,
        recent_reviews,
        last_updated,
        is_authored_by_me: false,
        merge_state_status: overview.merge_state_status,
        head_oid: overview.head_ref_oid,
        is_cross_repository: overview.is_cross_repository,
        auto_merge_method: overview.auto_merge_method,
        head_branch_exists,
        repo_merge: repo_merge.map(Into::into),
        in_merge_queue,
        base_has_merge_queue,
        can_bypass_rules,
        created_at: overview.created_at,
        commit_count: overview.commit_count,
        reviewers,
        fetch_ticket: 0,
    })
}

/// Delete/Restore branch only make sense once a same-repo PR is merged or
/// closed, so the extra `gh api` lookup runs only then. A fork's branch is
/// never looked up in the base repo: a same-named branch there is a different
/// branch (ADR 0040).
fn head_branch_exists_after_close(
    owner: &str,
    repo: &str,
    overview: &er_engine::github::PrOverviewFull,
    repo_merge: Option<&er_engine::gh_pr_actions::RepoMergeSettings>,
) -> Option<bool> {
    if overview.state == "OPEN" || overview.head_ref_name.is_empty() || overview.is_cross_repository
    {
        return None;
    }
    // GitHub deletes the branch a moment after the merge on such repos.
    let deleting_itself =
        overview.state == "MERGED" && repo_merge.is_some_and(|s| s.delete_branch_on_merge);
    er_engine::gh_pr_actions::cached_remote_branch_exists(
        owner,
        repo,
        &overview.head_ref_name,
        deleting_itself,
    )
}

/// `(base has a merge queue, PR is in it)` for an open PR. Queue presence is
/// cached per repo and base; the membership lookup costs a `gh` call, so it
/// runs only on a base that has a queue.
fn merge_queue_status(
    owner: &str,
    repo: &str,
    number: u64,
    overview: &er_engine::github::PrOverviewFull,
) -> (bool, bool) {
    use er_engine::gh_pr_actions::{cached_base_has_merge_queue, gh_pr_in_merge_queue};
    let has_queue = overview.state == "OPEN"
        && cached_base_has_merge_queue(owner, repo, &overview.base_ref_name) == Some(true);
    let queued = has_queue && gh_pr_in_merge_queue(owner, repo, number).unwrap_or(false);
    (has_queue, queued)
}

/// Only a blocked PR can use a bypass, so other states skip the lookup. A
/// merge-queue base is skipped too: `--admin` there merges past the queue.
fn can_bypass_rules(
    owner: &str,
    repo: &str,
    number: u64,
    overview: &er_engine::github::PrOverviewFull,
    base_has_merge_queue: bool,
) -> bool {
    overview.state == "OPEN"
        && !base_has_merge_queue
        && overview.merge_state_status.as_deref() == Some("BLOCKED")
        && er_engine::gh_pr_actions::cached_viewer_can_bypass(
            owner,
            repo,
            &overview.base_ref_name,
            number,
        ) == Some(true)
}

/// The reviewer list for a PR. The author is left out: their reviews are
/// replies on their own PR, not reviews of it.
pub fn github_reviewers(
    overview: &er_engine::github::PrOverviewFull,
    reviews: &[er_engine::github::PrReview],
) -> Vec<GhReviewerSummary> {
    er_engine::github::pr_reviewers(&overview.author, &overview.review_requests, reviews)
        .into_iter()
        .map(|r| GhReviewerSummary {
            reviewer: r.reviewer,
            is_team: r.is_team,
            requested: r.requested,
            state: r.state,
        })
        .collect()
}
