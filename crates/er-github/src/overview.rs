//! PR overview and status: title, body, reviewers, CI checks, the combined
//! status bundle, and the PR-wide writes that ride with it.

use crate::{with_gh_sentence, with_prefix};
use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::{default_handle, CliGitHub, GhError};

/// Fetched PR overview data for the PrOverview panel
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrOverviewData {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub author: String,
    pub url: String,
    pub base_branch: String,
    pub head_branch: String,
    pub checks: Vec<CiCheck>,
    pub reviewers: Vec<ReviewerStatus>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CiCheck {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewerStatus {
    pub login: String,
    pub state: String,
}

/// Deduplicate reviewers by login from the reviews array.
/// APPROVED and CHANGES_REQUESTED are "decisive" — a later COMMENTED review
/// does not downgrade them (matches GitHub's displayed review state).
fn deduplicate_reviewers(reviews_arr: &[serde_json::Value]) -> Vec<ReviewerStatus> {
    let mut reviewer_map: HashMap<String, String> = HashMap::new();
    for r in reviews_arr {
        if let Some(login) = r["author"]["login"].as_str() {
            let state = r["state"].as_str().unwrap_or("PENDING").to_string();
            if let Some(existing) = reviewer_map.get(login) {
                // Don't let COMMENTED overwrite a decisive review state
                if (existing == "APPROVED" || existing == "CHANGES_REQUESTED")
                    && state == "COMMENTED"
                {
                    continue;
                }
            }
            reviewer_map.insert(login.to_string(), state);
        }
    }
    reviewer_map
        .into_iter()
        .map(|(login, state)| ReviewerStatus { login, state })
        .collect()
}

impl CliGitHub {
    /// Update the PR body with the given content via `gh pr edit --body`.
    pub(crate) fn edit_body(&self, repo_root: &str, body: &str) -> Result<(), GhError> {
        self.run_gh_args(&["pr", "edit", "--body", body], Some(repo_root))
            .map(|_| ())
    }

    /// Approve a PR via `gh pr review --approve`. Uses `--repo` and PR number
    /// when reviewing a remote PR.
    pub(crate) fn approve(
        &self,
        repo_root: &str,
        remote_repo: Option<&str>,
        pr_number: Option<u64>,
    ) -> Result<(), GhError> {
        let mut args = vec![
            "pr".to_string(),
            "review".to_string(),
            "--approve".to_string(),
        ];
        if let (Some(slug), Some(n)) = (remote_repo, pr_number) {
            args.push("--repo".to_string());
            args.push(slug.to_string());
            args.push(n.to_string());
        }
        self.run_gh(&args, Some(repo_root), None).map(|_| ())
    }
}

impl CliGitHub {
    /// Fetch PR overview data: title, body, state, author, branches, reviewers.
    /// If `pr_number` is Some, fetches that specific PR; otherwise auto-detects
    /// from the current branch.
    pub(crate) fn overview(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
    ) -> Result<Option<PrOverviewData>, GhError> {
        Ok(self.overview_impl(repo_root, pr_number, true))
    }

    /// Like [`Self::overview`], but skips the separate `gh pr checks`
    /// subprocess (`checks` is always empty in the result). Use this when the
    /// caller already has a fresher CI-checks source for the same PR — e.g. the
    /// desktop's 30s gh-status loop (`fetch_github_status` / `gh_status_cache`)
    /// — and only needs title/state/author/branch/reviewer metadata, to avoid a
    /// duplicate `gh pr checks` call on every tick.
    pub(crate) fn overview_no_checks(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
    ) -> Result<Option<PrOverviewData>, GhError> {
        Ok(self.overview_impl(repo_root, pr_number, false))
    }

    fn overview_impl(
        &self,
        repo_root: &str,
        pr_number: Option<u64>,
        include_checks: bool,
    ) -> Option<PrOverviewData> {
        let mut args = vec!["pr".to_string(), "view".to_string()];
        if let Some(n) = pr_number {
            args.push(n.to_string());
        }
        args.extend([
            "--json".to_string(),
            "number,title,body,state,author,url,baseRefName,headRefName,reviews".to_string(),
        ]);
        let view_output = self.run_gh(&args, Some(repo_root), None).ok()?;
        let json_str = String::from_utf8_lossy(&view_output.body);
        let v: serde_json::Value = serde_json::from_str(&json_str).ok()?;

        let number = v["number"].as_u64().unwrap_or(0);
        let title = v["title"].as_str().unwrap_or("").to_string();
        let body = v["body"].as_str().unwrap_or("").to_string();
        let state = v["state"].as_str().unwrap_or("").to_string();
        let author = v["author"]["login"].as_str().unwrap_or("").to_string();
        let url = v["url"].as_str().unwrap_or("").to_string();
        let base_branch = v["baseRefName"].as_str().unwrap_or("").to_string();
        let head_branch = v["headRefName"].as_str().unwrap_or("").to_string();

        let reviewers: Vec<ReviewerStatus> = if let Some(reviews_arr) = v["reviews"].as_array() {
            deduplicate_reviewers(reviews_arr)
        } else {
            Vec::new()
        };

        // Fetch CI checks (separate call — may fail if no checks configured).
        let checks = if include_checks {
            self.checks_data(repo_root).unwrap_or_default()
        } else {
            Vec::new()
        };

        Some(PrOverviewData {
            number,
            title,
            body,
            state,
            author,
            url,
            base_branch,
            head_branch,
            checks,
            reviewers,
        })
    }

    /// Fetch CI check runs for the current PR; a failure reads as no checks.
    fn checks_data(&self, repo_root: &str) -> Result<Vec<CiCheck>, GhError> {
        let Ok(output) = self.run_gh_args(
            &["pr", "checks", "--json", "name,state,bucket"],
            Some(repo_root),
        ) else {
            return Ok(Vec::new());
        };
        let json_str = String::from_utf8_lossy(&output.body);
        let arr: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap_or_default();

        let checks = arr
            .iter()
            .filter_map(|c| {
                let name = c["name"].as_str()?;
                // gh pr checks uses "state" (e.g. SUCCESS, FAILURE, PENDING)
                // and "bucket" (pass, fail, pending) for grouping
                let state = c["state"].as_str().unwrap_or("unknown");
                let bucket = c["bucket"].as_str().map(|s| s.to_string());
                Some(CiCheck {
                    name: name.to_string(),
                    status: state.to_string(),
                    conclusion: bucket,
                })
            })
            .collect();

        Ok(checks)
    }

    // Rich GitHub status for the SourcesCard: review decision, mergeable,
    // labels, checks, recent comments/reviews. Parsing lives in pure functions
    // so it can be unit-tested against fixed JSON blobs.

    /// Get PR metadata (base branch, head branch) via `gh pr view --repo`.
    pub(crate) fn pr_metadata_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(String, String), GhError> {
        let repo_slug = format!("{owner}/{repo}");
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &number.to_string(),
                "--repo",
                &repo_slug,
                "--json",
                "baseRefName,headRefName",
                "--jq",
                r#"[.baseRefName, .headRefName] | @tsv"#,
            ],
            None,
        )?;
        let text = String::from_utf8_lossy(&out.body);
        let text = text.trim();
        let (base, head) = text
            .split_once('\t')
            .ok_or_else(|| anyhow::anyhow!("Unexpected gh pr view output: {text}"))?;
        Ok((base.to_string(), head.to_string()))
    }

    /// Fetch PR overview data for a remote repo (no local clone needed).
    pub(crate) fn overview_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Option<PrOverviewData>, GhError> {
        let repo_slug = format!("{owner}/{repo}");
        let number_arg = number.to_string();
        let Ok(view_output) = self.run_gh_args(
            &[
                "pr",
                "view",
                &number_arg,
                "--repo",
                &repo_slug,
                "--json",
                "number,title,body,state,author,url,baseRefName,headRefName,reviews",
            ],
            None,
        ) else {
            return Ok(None);
        };
        let json_str = String::from_utf8_lossy(&view_output.body);
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) else {
            return Ok(None);
        };

        let number = v["number"].as_u64().unwrap_or(0);
        let title = v["title"].as_str().unwrap_or("").to_string();
        let body = v["body"].as_str().unwrap_or("").to_string();
        let state = v["state"].as_str().unwrap_or("").to_string();
        let author = v["author"]["login"].as_str().unwrap_or("").to_string();
        let url = v["url"].as_str().unwrap_or("").to_string();
        let base_branch = v["baseRefName"].as_str().unwrap_or("").to_string();
        let head_branch = v["headRefName"].as_str().unwrap_or("").to_string();

        let reviewers: Vec<ReviewerStatus> = if let Some(reviews_arr) = v["reviews"].as_array() {
            deduplicate_reviewers(reviews_arr)
        } else {
            Vec::new()
        };

        // Skip CI checks for remote (would need --repo flag on gh pr checks too)
        Ok(Some(PrOverviewData {
            number,
            title,
            body,
            state,
            author,
            url,
            base_branch,
            head_branch,
            checks: Vec::new(),
            reviewers,
        }))
    }
}

/// Compatibility wrappers for front ends and engine callers.
pub fn gh_pr_metadata_remote(owner: &str, repo: &str, number: u64) -> Result<(String, String)> {
    default_handle()
        .pr_metadata_remote(owner, repo, number)
        .map_err(|error| with_gh_sentence(format!("Failed to get PR #{number} metadata"), error))
}

pub fn gh_pr_overview(repo_root: &str, pr_number: Option<u64>) -> Option<PrOverviewData> {
    default_handle()
        .overview(repo_root, pr_number)
        .ok()
        .flatten()
}

pub fn gh_pr_overview_no_checks(repo_root: &str, pr_number: Option<u64>) -> Option<PrOverviewData> {
    default_handle()
        .overview_no_checks(repo_root, pr_number)
        .ok()
        .flatten()
}

pub fn gh_pr_overview_remote(owner: &str, repo: &str, number: u64) -> Option<PrOverviewData> {
    default_handle()
        .overview_remote(owner, repo, number)
        .ok()
        .flatten()
}

#[derive(Debug, Clone)]
pub struct PrOverviewFull {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub is_draft: bool,
    pub author: String,
    pub url: String,
    pub review_decision: Option<String>,
    pub mergeable: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub labels: Vec<String>,
    /// GitHub's merge-box verdict: `CLEAN`, `BEHIND`, `BLOCKED`, `DIRTY`,
    /// `UNSTABLE`, `HAS_HOOKS`, `DRAFT`, `UNKNOWN`.
    pub merge_state_status: Option<String>,
    /// Head commit the PR points at now. The desktop merge pins to the reviewed
    /// diff's head when one is on screen, and to this only otherwise (ADR 0040).
    pub head_ref_oid: String,
    /// Head branch lives in a fork. Branch delete/restore is offered only for
    /// same-repo PRs (ADR 0040).
    pub is_cross_repository: bool,
    /// Merge method of a pending auto-merge (`MERGE`, `SQUASH`, `REBASE`).
    pub auto_merge_method: Option<String>,
    /// ISO-8601 time the PR was opened. `gh pr view --json` has no timeline,
    /// so "opened N ago" and the opening activity event are built from this
    /// instead of a timeline query (ADR 0026).
    pub created_at: Option<String>,
    /// Reviews requested and not yet submitted. GitHub drops a request once
    /// the reviewer submits, and adds it back on a re-request.
    pub review_requests: Vec<PrReviewRequest>,
    /// Commits on the PR, counted from the `commits` list `gh` returns.
    pub commit_count: usize,
}

/// One pending review request: a user, or a team named by its `org/slug`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReviewRequest {
    pub reviewer: String,
    pub is_team: bool,
}

/// A person on the PR's reviewer list, folded from pending requests and
/// submitted reviews.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReviewer {
    pub reviewer: String,
    pub is_team: bool,
    /// Their review currently has a pending request (first ask or re-ask).
    pub requested: bool,
    /// State of the review that counts for them (`APPROVED`,
    /// `CHANGES_REQUESTED`, `DISMISSED`, `COMMENTED`); `None` until they review.
    pub state: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PrComment {
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct PrReview {
    pub author: String,
    pub state: String,
    pub body: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct CheckRun {
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub url: Option<String>,
    /// ISO-8601 start/end. `gh` reports a not-yet-reached time as Go's zero
    /// time; that parses to `None`, not to year 1.
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    /// Status-context text, e.g. a deploy preview's "Ready". It is what a
    /// check without timings shows in place of a duration.
    pub description: Option<String>,
}

impl CheckRun {
    /// Wall time of a finished check. `None` while it runs, when either time
    /// is missing, or when the clock order is inverted.
    pub fn duration_secs(&self) -> Option<u64> {
        let start = crate::types::parse_github_updated_at(self.started_at.as_deref()?)?;
        let end = crate::types::parse_github_updated_at(self.completed_at.as_deref()?)?;
        u64::try_from(end - start).ok()
    }
}

/// Pure parser for `gh pr view --json number,title,state,isDraft,author,reviewDecision,mergeable,headRefName,baseRefName,labels,url` output.
pub fn parse_pr_overview(json: &str) -> Result<PrOverviewFull> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid PR overview JSON")?;
    Ok(PrOverviewFull {
        number: v["number"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or("").to_string(),
        body: v["body"].as_str().unwrap_or("").to_string(),
        state: v["state"].as_str().unwrap_or("").to_string(),
        is_draft: v["isDraft"].as_bool().unwrap_or(false),
        author: v["author"]["login"].as_str().unwrap_or("").to_string(),
        url: v["url"].as_str().unwrap_or("").to_string(),
        review_decision: v["reviewDecision"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        mergeable: v["mergeable"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        head_ref_name: v["headRefName"].as_str().unwrap_or("").to_string(),
        base_ref_name: v["baseRefName"].as_str().unwrap_or("").to_string(),
        labels: v["labels"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        merge_state_status: non_empty_str(&v["mergeStateStatus"]),
        head_ref_oid: v["headRefOid"].as_str().unwrap_or("").to_string(),
        is_cross_repository: v["isCrossRepository"].as_bool().unwrap_or(false),
        // `autoMergeRequest` is null when auto-merge is off.
        auto_merge_method: non_empty_str(&v["autoMergeRequest"]["mergeMethod"]),
        created_at: non_empty_str(&v["createdAt"]),
        review_requests: v["reviewRequests"]
            .as_array()
            .map(|arr| arr.iter().filter_map(parse_review_request).collect())
            .unwrap_or_default(),
        commit_count: v["commits"].as_array().map_or(0, Vec::len),
    })
}

/// `gh` flattens a requested reviewer to `{__typename, login}` for a user or
/// bot and `{__typename, name, slug}` for a team, where `slug` is `org/slug`.
fn parse_review_request(r: &serde_json::Value) -> Option<PrReviewRequest> {
    let is_team = r["__typename"].as_str() == Some("Team");
    let reviewer = non_empty_str(&r["login"])
        .or_else(|| non_empty_str(&r["slug"]))
        .or_else(|| non_empty_str(&r["name"]))?;
    Some(PrReviewRequest { reviewer, is_team })
}

/// Fold pending requests and submitted reviews into one reviewer list:
/// requested reviewers first in request order, then everyone else who
/// reviewed, in order of their first review. The PR author's own reviews are
/// replies on their PR, not a review of it, so they are left out.
///
/// The state that counts is the reviewer's latest approve/request-changes/
/// dismiss; a later plain comment does not undo an approval on GitHub either.
/// Someone who only commented shows `COMMENTED`.
pub fn pr_reviewers(
    author: &str,
    requests: &[PrReviewRequest],
    reviews: &[PrReview],
) -> Vec<PrReviewer> {
    let mut out: Vec<PrReviewer> = requests
        .iter()
        .map(|r| PrReviewer {
            reviewer: r.reviewer.clone(),
            is_team: r.is_team,
            requested: true,
            state: None,
        })
        .collect();
    let mut ordered: Vec<&PrReview> = reviews
        .iter()
        .filter(|r| !r.author.is_empty() && !r.author.eq_ignore_ascii_case(author))
        .filter(|r| r.state != "PENDING")
        .collect();
    // `gh` returns reviews oldest first; sort anyway so the latest-wins rule
    // does not lean on that. ISO-8601 in UTC sorts as text.
    ordered.sort_by(|a, b| a.submitted_at.cmp(&b.submitted_at));
    for review in ordered {
        let idx = match out
            .iter()
            .position(|p| !p.is_team && p.reviewer.eq_ignore_ascii_case(&review.author))
        {
            Some(i) => i,
            None => {
                out.push(PrReviewer {
                    reviewer: review.author.clone(),
                    is_team: false,
                    requested: false,
                    state: None,
                });
                out.len() - 1
            }
        };
        let slot = &mut out[idx].state;
        let decisive = review.state != "COMMENTED";
        if decisive || slot.is_none() {
            *slot = Some(review.state.clone());
        }
    }
    out
}

fn non_empty_str(v: &serde_json::Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())
}

/// Pure parser for `gh pr view --json comments` output (the wrapping object).
pub fn parse_pr_comments(json: &str) -> Result<Vec<PrComment>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid comments JSON")?;
    let arr = v["comments"].as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .map(|c| PrComment {
            author: c["author"]["login"].as_str().unwrap_or("").to_string(),
            body: c["body"].as_str().unwrap_or("").to_string(),
            created_at: c["createdAt"].as_str().unwrap_or("").to_string(),
            url: c["url"].as_str().unwrap_or("").to_string(),
        })
        .collect())
}

/// Pure parser for `gh pr view --json reviews` output (the wrapping object).
pub fn parse_pr_reviews(json: &str) -> Result<Vec<PrReview>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid reviews JSON")?;
    let arr = v["reviews"].as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .map(|r| PrReview {
            author: r["author"]["login"].as_str().unwrap_or("").to_string(),
            state: r["state"].as_str().unwrap_or("").to_string(),
            body: r["body"].as_str().unwrap_or("").to_string(),
            submitted_at: r["submittedAt"].as_str().unwrap_or("").to_string(),
        })
        .collect())
}

/// Pure parser for `gh pr checks --json name,state,bucket,link,startedAt,completedAt,description` output.
pub fn parse_pr_checks(json: &str) -> Result<Vec<CheckRun>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid checks JSON")?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .map(|c| {
            let state = c["state"].as_str().unwrap_or("").to_string();
            let bucket = c["bucket"].as_str().unwrap_or("").to_string();
            // Map `gh pr checks` semantics into status/conclusion:
            //   state ~ PENDING|IN_PROGRESS|QUEUED|SUCCESS|FAILURE|...
            //   bucket ~ pass|fail|pending|cancel|skipping
            let state_l = state.to_ascii_lowercase();
            let (status, conclusion) = if state_l == "pending"
                || state_l == "in_progress"
                || state_l == "queued"
                || state_l == "waiting"
                || state_l == "requested"
                || state_l == "expected"
                || bucket.eq_ignore_ascii_case("pending")
            {
                ("PENDING".to_string(), "".to_string())
            } else {
                ("COMPLETED".to_string(), state)
            };
            CheckRun {
                name: c["name"].as_str().unwrap_or("").to_string(),
                status,
                conclusion: if conclusion.is_empty() {
                    bucket
                } else {
                    conclusion
                },
                url: c["link"].as_str().map(|s| s.to_string()),
                started_at: check_time(&c["startedAt"]),
                completed_at: check_time(&c["completedAt"]),
                description: non_empty_str(&c["description"]),
            }
        })
        .collect())
}

/// `gh` serializes an unset check time as Go's zero time rather than null.
fn check_time(v: &serde_json::Value) -> Option<String> {
    non_empty_str(v).filter(|s| !s.starts_with("0001-01-01"))
}

/// Combined overview + conversation-comments + reviews for a remote PR, in
/// ONE `gh pr view --json` subprocess. Collapses three separate `gh pr view`
/// calls (`gh_pr_overview_remote_full` + `gh_pr_comments_overview` +
/// `gh_pr_reviews`) into one GraphQL-backed call — verified empirically that
/// requesting `comments`/`reviews` alongside other fields returns byte-identical
/// data to requesting them alone.
///
/// `gh pr checks` (CI status) is intentionally NOT folded in here — it's a
/// different `gh` subcommand and must stay a separate call (`gh_pr_checks_remote`).
#[derive(Debug, Clone)]
pub struct PrStatusBundle {
    pub overview: PrOverviewFull,
    pub comments: Vec<PrComment>,
    pub reviews: Vec<PrReview>,
}

impl CliGitHub {
    /// Fetch overview + comments + reviews for a remote PR in one `gh pr view`
    /// call. No local clone required.
    pub(crate) fn pr_status_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<PrStatusBundle, GhError> {
        let repo_slug = format!("{owner}/{repo}");
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &number.to_string(),
                "--repo",
                &repo_slug,
                "--json",
                "number,title,body,state,isDraft,author,reviewDecision,mergeable,mergeStateStatus,headRefName,headRefOid,isCrossRepository,autoMergeRequest,baseRefName,labels,url,comments,reviews,createdAt,reviewRequests,commits",
            ],
            None,
        )?;
        let json = String::from_utf8_lossy(&out.body);
        // overview parse failure is fatal (the caller treats a failed overview
        // as "no status available").
        let overview = parse_pr_overview(&json)?;
        // comments/reviews failures are non-fatal (the old call sites wrapped
        // these in `.unwrap_or_default()`).
        let comments = parse_pr_comments(&json).unwrap_or_default();
        let reviews = parse_pr_reviews(&json).unwrap_or_default();
        Ok(PrStatusBundle {
            overview,
            comments,
            reviews,
        })
    }

    /// Fetch CI check runs for a remote PR; no checks configured is not an error.
    pub(crate) fn pr_checks_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Vec<CheckRun>, GhError> {
        let repo_slug = format!("{owner}/{repo}");
        // No checks configured is an empty list; a gh that could not run at
        // all is an error, not "no checks".
        let out = match self.run_gh_args(
            &[
                "pr",
                "checks",
                &number.to_string(),
                "--repo",
                &repo_slug,
                "--json",
                "name,state,bucket,link,startedAt,completedAt,description",
            ],
            None,
        ) {
            Ok(out) => out,
            Err(error)
                if matches!(
                    error,
                    GhError::NotInstalled | GhError::Timeout | GhError::Tool { status: -1, .. }
                ) =>
            {
                return Err(error);
            }
            Err(_) => return Ok(Vec::new()),
        };
        parse_pr_checks(&String::from_utf8_lossy(&out.body)).map_err(GhError::from)
    }
}

/// Compatibility wrappers for front ends and engine callers.
pub fn gh_pr_status_remote(owner: &str, repo: &str, number: u64) -> Result<PrStatusBundle> {
    default_handle()
        .pr_status_remote(owner, repo, number)
        .map_err(|error| with_gh_sentence("gh pr view (status bundle) failed", error))
}

pub fn gh_pr_checks_remote(owner: &str, repo: &str, number: u64) -> Result<Vec<CheckRun>> {
    default_handle()
        .pr_checks_remote(owner, repo, number)
        .map_err(anyhow::Error::from)
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_edit_body(repo_root: &str, body: &str) -> Result<()> {
    default_handle()
        .edit_body(repo_root, body)
        .map_err(|error| with_prefix("Failed to update PR body", error))
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_pr_approve(
    repo_root: &str,
    remote_repo: Option<&str>,
    pr_number: Option<u64>,
) -> Result<()> {
    default_handle()
        .approve(repo_root, remote_repo, pr_number)
        .map_err(|error| with_prefix("gh pr approve failed", error))
}

/// Whether a CI check run counts as failing.
pub fn check_run_is_failing(check: &CheckRun) -> bool {
    let conclusion = check.conclusion.to_ascii_lowercase();
    matches!(
        conclusion.as_str(),
        "failure"
            | "fail"
            | "cancelled"
            | "canceled"
            | "timed_out"
            | "timeout"
            | "error"
            | "action_required"
    )
}

/// Aggregate CI state for a PR from `gh pr checks` results.
pub fn summarize_checks_state(checks: &[CheckRun]) -> &'static str {
    if checks.is_empty() {
        return "unknown";
    }
    if checks.iter().any(check_run_is_failing) {
        return "failing";
    }
    if checks.iter().any(check_run_is_pending) {
        return "pending";
    }
    "passing"
}

fn check_run_is_pending(c: &CheckRun) -> bool {
    let status = c.status.to_ascii_lowercase();
    let conclusion = c.conclusion.to_ascii_lowercase();
    matches!(
        status.as_str(),
        "pending" | "in_progress" | "queued" | "waiting" | "requested" | "expected"
    ) || matches!(conclusion.as_str(), "pending" | "in_progress" | "queued")
}

impl CliGitHub {
    /// Fetch checks and return rollup state for a remote PR.
    pub(crate) fn pr_checks_state_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<&'static str, GhError> {
        let checks = self.pr_checks_remote(owner, repo, number)?;
        Ok(summarize_checks_state(&checks))
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_checks_state_remote(owner: &str, repo: &str, number: u64) -> Result<&'static str> {
    default_handle()
        .pr_checks_state_remote(owner, repo, number)
        .map_err(anyhow::Error::from)
}
