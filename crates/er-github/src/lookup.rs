//! Lookups: PR URLs and refs, the checked-out branch's PR, notifications,
//! the viewer, and the review-queue listing.

use crate::{with_gh_sentence, with_prefix};
use anyhow::{Context, Result};
use er_git::Git;
use serde::Deserialize;

use crate::repo::parse_origin_remote;
use crate::{default_handle, CliGitHub, GhError};

/// Parsed reference to a GitHub PR
#[derive(Debug, Clone)]
pub struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

/// Parse a GitHub PR URL into its components.
///
/// Supports: https://github.com/owner/repo/pull/42
/// Also handles: trailing /files, /commits, /checks, etc.
/// Also handles: github.com/owner/repo/pull/42 (no scheme)
pub fn parse_github_pr_url(url: &str) -> Option<PrRef> {
    // Strip scheme
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    // Must start with github.com
    let rest = stripped.strip_prefix("github.com/")?;

    // Split: owner/repo/pull/number[/...]
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 4 {
        return None;
    }
    if parts[2] != "pull" {
        return None;
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    let number = parts[3].parse::<u64>().ok()?;

    if owner.is_empty() || repo.is_empty() || number == 0 {
        return None;
    }

    Some(PrRef {
        owner,
        repo,
        number,
    })
}

/// Check if a string looks like a GitHub PR URL
pub fn is_github_pr_url(s: &str) -> bool {
    parse_github_pr_url(s).is_some()
}

impl CliGitHub {
    /// Check that `gh` is installed and authenticated.
    pub(crate) fn ensure_installed(&self) -> Result<(), GhError> {
        match self.run_gh_args(&["--version"], None) {
            Ok(_) => {}
            Err(GhError::NotInstalled) => {
                return Err(GhError::Local {
                    message: "GitHub CLI (gh) is not installed. Install it: https://cli.github.com"
                        .to_string(),
                });
            }
            Err(_) => {
                return Err(GhError::Local {
                    message: "GitHub CLI (gh) is not working properly".to_string(),
                });
            }
        }
        match self.run_gh_args(&["auth", "status"], None) {
            Ok(_) => Ok(()),
            Err(_) => Err(GhError::Unauthenticated {
                message: "GitHub CLI is not authenticated. Run: gh auth login".to_string(),
            }),
        }
    }

    /// Participating unread notifications (not the full watched-repo firehose).
    pub(crate) fn list_participating_notifications(&self) -> Result<Vec<GhNotification>, GhError> {
        let out = self.run_gh_args(
            &["api", "notifications?participating=true&per_page=50"],
            None,
        )?;
        serde_json::from_slice(&out.body).map_err(|error| GhError::Local {
            message: format!("Failed to parse GitHub notifications: {error}"),
        })
    }

    /// Get the base branch for a PR using `gh pr view`.
    pub(crate) fn pr_base_branch(
        &self,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<String, GhError> {
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &pr_number.to_string(),
                "--json",
                "baseRefName",
                "--jq",
                ".baseRefName",
            ],
            Some(repo_root),
        )?;
        let base = String::from_utf8_lossy(&out.body).trim().to_string();
        if base.is_empty() {
            return Err(GhError::Local {
                message: format!("PR #{pr_number} has no base branch"),
            });
        }
        Ok(base)
    }
}

/// Compatibility wrappers for front ends and engine callers.
pub fn ensure_gh_installed() -> Result<()> {
    default_handle()
        .ensure_installed()
        .map_err(anyhow::Error::from)
}

pub fn gh_list_participating_notifications() -> Result<Vec<GhNotification>> {
    default_handle()
        .list_participating_notifications()
        .map_err(|error| with_gh_sentence("Failed to fetch GitHub notifications", error))
}

pub fn gh_pr_base_branch(pr_number: u64, repo_root: &str) -> Result<String> {
    default_handle()
        .pr_base_branch(pr_number, repo_root)
        .map_err(|error| with_gh_sentence(format!("Failed to get PR #{pr_number}"), error))
}

/// A GitHub notification thread from `GET /notifications`.
#[derive(Debug, Clone, Deserialize)]
pub struct GhNotification {
    pub id: String,
    pub reason: String,
    #[serde(default)]
    pub unread: bool,
    #[serde(default)]
    pub updated_at: String,
    pub subject: GhNotificationSubject,
    pub repository: GhNotificationRepo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhNotificationSubject {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "type")]
    pub subject_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhNotificationRepo {
    #[serde(default)]
    pub full_name: String,
}

/// Pull request number from a notification subject URL.
///
/// Handles both `/pulls/N` and `/issues/N` (PR comments use the issues path).
pub fn pr_number_from_notification_url(url: &str) -> Option<u64> {
    let path = url.split('?').next().unwrap_or(url);
    let mut parts = path.rsplit('/');
    let last = parts.next()?;
    let kind = parts.next()?;
    if kind != "pulls" && kind != "issues" && kind != "pull" {
        return None;
    }
    last.parse().ok()
}

impl CliGitHub {
    /// Get the base and head branch names for a PR with a single `gh pr view`.
    pub(crate) fn pr_branch_names(
        &self,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<(String, String), GhError> {
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &pr_number.to_string(),
                "--json",
                "baseRefName,headRefName",
                "--jq",
                r#"[.baseRefName, .headRefName] | @tsv"#,
            ],
            Some(repo_root),
        )?;
        let text = String::from_utf8_lossy(&out.body);
        let mut parts = text.trim().split('\t');
        let base = parts.next().unwrap_or("").to_string();
        let head = parts.next().unwrap_or("").to_string();
        if base.is_empty() {
            return Err(GhError::Local {
                message: format!("PR #{pr_number} has no base branch"),
            });
        }
        Ok((base, head))
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_branch_names(pr_number: u64, repo_root: &str) -> Result<(String, String)> {
    default_handle()
        .pr_branch_names(pr_number, repo_root)
        .map_err(|error| with_gh_sentence(format!("Failed to get PR #{pr_number}"), error))
}

/// Result of probing the checked-out branch's PR. gh's explicit "no pull
/// request" answer is [`BranchPrProbe::Absent`]; a probe that could not ask is
/// an `Err`, never a third variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchPrProbe {
    Present {
        number: u64,
        base: String,
    },
    /// gh answered that this branch has no pull request.
    Absent,
}

impl CliGitHub {
    pub(crate) fn probe_pr_for_current_branch(
        &self,
        repo_root: &str,
    ) -> Result<BranchPrProbe, GhError> {
        // Use --jq to extract "number<tab>baseRefName" — robust against JSON formatting
        let out = match self.run_gh_args(
            &[
                "pr",
                "view",
                "--json",
                "number,baseRefName",
                "--jq",
                r#"[.number, .baseRefName] | @tsv"#,
            ],
            Some(repo_root),
        ) {
            Ok(out) => out,
            Err(error) if crate::classify::is_no_pull_request_answer(&error) => {
                return Ok(BranchPrProbe::Absent);
            }
            Err(error) => return Err(error),
        };

        let text = String::from_utf8_lossy(&out.body);
        let text = text.trim();
        let Some((num_str, base)) = text.split_once('\t') else {
            return Err(GhError::Local {
                message: format!("Unexpected gh pr view output: {text}"),
            });
        };
        let Ok(number) = num_str.parse::<u64>() else {
            return Err(GhError::Local {
                message: format!("Unexpected gh pr view output: {text}"),
            });
        };
        if base.is_empty() {
            return Err(GhError::Local {
                message: format!("Unexpected gh pr view output: {text}"),
            });
        }
        Ok(BranchPrProbe::Present {
            number,
            base: base.to_string(),
        })
    }

    /// The PR for the checked-out branch, when gh says the branch has one.
    ///
    /// Prefer [`Self::probe_pr_for_current_branch`] when "no PR" and "could
    /// not ask" must be told apart.
    pub(crate) fn pr_for_current_branch(
        &self,
        repo_root: &str,
    ) -> Result<Option<(u64, String)>, GhError> {
        match self.probe_pr_for_current_branch(repo_root)? {
            BranchPrProbe::Present { number, base } => Ok(Some((number, base))),
            BranchPrProbe::Absent => Ok(None),
        }
    }
}

/// Compatibility wrappers for front ends and engine callers.
pub fn probe_pr_for_current_branch(repo_root: &str) -> Result<BranchPrProbe> {
    default_handle()
        .probe_pr_for_current_branch(repo_root)
        .map_err(anyhow::Error::from)
}

pub fn gh_pr_for_current_branch(repo_root: &str) -> Result<Option<(u64, String)>> {
    default_handle()
        .pr_for_current_branch(repo_root)
        .map_err(anyhow::Error::from)
}

/// Result of a PR lookup for the checked-out branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrLookup {
    Present {
        owner: String,
        repo: String,
        number: u64,
    },
    /// gh answered that this branch has no pull request.
    Absent,
}

impl CliGitHub {
    /// Resolve the checked-out branch's PR without collapsing "no PR" and
    /// "could not ask" into one answer.
    pub(crate) fn lookup_pr_for_current_branch(
        &self,
        git: &dyn Git,
        repo_root: &str,
    ) -> Result<PrLookup, GhError> {
        let out = match self.run_gh_args(
            &["pr", "view", "--json", "number,headRepository"],
            Some(repo_root),
        ) {
            Ok(out) => out,
            Err(error) if crate::classify::is_no_pull_request_answer(&error) => {
                return Ok(PrLookup::Absent);
            }
            Err(error) => return Err(error),
        };

        let v: serde_json::Value =
            serde_json::from_slice(&out.body).map_err(|error| GhError::Local {
                message: format!("Failed to parse PR info JSON: {error}"),
            })?;
        let Some(number) = v["number"].as_u64() else {
            return Err(GhError::Local {
                message: "Missing PR number".to_string(),
            });
        };
        let owner = v["headRepository"]["owner"]["login"].as_str().unwrap_or("");
        let repo_name = v["headRepository"]["name"].as_str().unwrap_or("");
        if !owner.is_empty() && !repo_name.is_empty() {
            return Ok(PrLookup::Present {
                owner: owner.to_string(),
                repo: repo_name.to_string(),
                number,
            });
        }
        match parse_origin_remote(git, repo_root) {
            Some((owner, repo)) => Ok(PrLookup::Present {
                owner,
                repo,
                number,
            }),
            None => Err(GhError::Local {
                message: "Could not read the repository's origin remote".to_string(),
            }),
        }
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn lookup_pr_for_current_branch(git: &dyn Git, repo_root: &str) -> Result<PrLookup> {
    default_handle()
        .lookup_pr_for_current_branch(git, repo_root)
        .map_err(anyhow::Error::from)
}

impl CliGitHub {
    /// Open PR number whose head branch matches `head` on `owner/repo` (first
    /// non-draft).
    pub(crate) fn open_pr_number_for_head(
        &self,
        owner: &str,
        repo: &str,
        head: &str,
    ) -> Result<Option<u64>, GhError> {
        let slug = format!("{owner}/{repo}");
        let out = self.run_gh_args(
            &[
                "pr",
                "list",
                "--repo",
                &slug,
                "--head",
                head,
                "--state",
                "open",
                "--limit",
                "10",
                "--json",
                "number,isDraft",
            ],
            None,
        )?;
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&out.body).map_err(|error| GhError::Local {
                message: format!("invalid gh pr list JSON: {error}"),
            })?;
        Ok(rows
            .iter()
            .find(|row| row.get("isDraft").and_then(|v| v.as_bool()) != Some(true))
            .and_then(|row| row.get("number").and_then(|v| v.as_u64())))
    }

    /// PR title via `gh pr view --repo`; `None` when the JSON has no title.
    pub(crate) fn pr_title(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Option<String>, GhError> {
        let slug = format!("{owner}/{repo}");
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &number.to_string(),
                "--repo",
                &slug,
                "--json",
                "title",
            ],
            None,
        )?;
        let value: serde_json::Value =
            serde_json::from_slice(&out.body).map_err(|error| GhError::Local {
                message: format!("invalid gh pr view JSON: {error}"),
            })?;
        Ok(value
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    /// Get PR info (owner, repo, number) for the current branch.
    pub(crate) fn get_pr_info(
        &self,
        git: &dyn Git,
        repo_root: &str,
    ) -> Result<(String, String, u64), GhError> {
        let lookup = self
            .lookup_pr_for_current_branch(git, repo_root)
            .map_err(|error| GhError::Local {
                message: format!("Failed to get PR info: {error}"),
            })?;
        match lookup {
            PrLookup::Present {
                owner,
                repo,
                number,
            } => Ok((owner, repo, number)),
            PrLookup::Absent => Err(GhError::Local {
                message: "No PR associated with current branch".to_string(),
            }),
        }
    }

    /// Login of the authenticated `gh` user.
    pub(crate) fn viewer_login(&self, repo_root: Option<&str>) -> Result<String, GhError> {
        let out = self.run_gh_args(&["api", "user", "--jq", ".login"], repo_root)?;
        let login = String::from_utf8_lossy(&out.body).trim().to_string();
        if login.is_empty() {
            return Err(GhError::Local {
                message: "empty login".to_string(),
            });
        }
        Ok(login)
    }

    /// Run `gh stack view --json` (github/gh-stack extension) for the repo at
    /// `repo_root` and return its stdout.
    ///
    /// A non-zero exit is an expected steady state — the branch may not be in a
    /// stack, or the extension may not be installed — so `gh`'s stderr
    /// diagnostic is the error message.
    pub(crate) fn stack_view_json(&self, repo_root: &str) -> Result<String, GhError> {
        // `gh stack view` opens an interactive TUI under a TTY; `--json` avoids
        // it, and these keep any prompt from blocking the worker thread that
        // runs this.
        const ENV: &[(&str, &str)] = &[("GH_PROMPT_DISABLED", "1"), ("GH_NO_UPDATE_NOTIFIER", "1")];
        let args = [
            "stack".to_string(),
            "view".to_string(),
            "--json".to_string(),
        ];
        let out = self.run_gh_with_env(&args, Some(repo_root), None, ENV)?;
        Ok(String::from_utf8_lossy(&out.body).to_string())
    }

    /// Open PRs of `owner/repo` with just the refs needed to walk a stack chain.
    /// Blocking; callers run it on a worker thread.
    ///
    /// A single `gh pr list` read, so it takes the same 10 s bound as every
    /// other gh call: a worker stuck on a stalled gh is worse than a timeout
    /// the caller can retry.
    pub(crate) fn open_pr_refs_json(&self, repo_slug: &str) -> Result<String, GhError> {
        let out = self.run_gh_args(
            &[
                "pr",
                "list",
                "--repo",
                repo_slug,
                "--state",
                "open",
                "--limit",
                "200",
                "--json",
                "number,url,headRefName,baseRefName",
            ],
            None,
        )?;
        Ok(String::from_utf8_lossy(&out.body).to_string())
    }
}

/// Compatibility wrappers for front ends and engine callers.
pub fn gh_open_pr_number_for_head(owner: &str, repo: &str, head: &str) -> Result<Option<u64>> {
    default_handle()
        .open_pr_number_for_head(owner, repo, head)
        .map_err(|error| with_gh_sentence("gh pr list --head failed", error))
}

pub fn gh_pr_title(owner: &str, repo: &str, number: u64) -> Result<Option<String>> {
    default_handle()
        .pr_title(owner, repo, number)
        .map_err(anyhow::Error::from)
}

pub fn get_pr_info(git: &dyn Git, repo_root: &str) -> Result<(String, String, u64)> {
    default_handle()
        .get_pr_info(git, repo_root)
        .map_err(anyhow::Error::from)
}

/// Compatibility wrapper for front ends; removed when they take the handle.
pub fn gh_viewer_login(repo_root: Option<&str>) -> Result<String> {
    default_handle()
        .viewer_login(repo_root)
        .map_err(|error| with_prefix("Failed to read the gh user", error))
}

pub fn gh_stack_view_json(repo_root: &str) -> Result<String> {
    default_handle()
        .stack_view_json(repo_root)
        .map_err(anyhow::Error::from)
}

pub fn gh_open_pr_refs_json(repo_slug: &str) -> Result<String> {
    default_handle()
        .open_pr_refs_json(repo_slug)
        .map_err(|error| with_gh_sentence("gh pr list failed", error))
}

impl CliGitHub {
    /// Login of the authenticated `gh` user, when there is one.
    pub(crate) fn current_login(&self) -> Result<Option<String>, GhError> {
        let out = self.run_gh_args(&["api", "user", "--jq", ".login"], None)?;
        let login = String::from_utf8_lossy(&out.body).trim().to_string();
        Ok((!login.is_empty()).then_some(login))
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_current_login() -> Result<Option<String>> {
    default_handle()
        .current_login()
        .map_err(anyhow::Error::from)
}

// ─────────────────────────────────────────────────────────────────────────────
// Review-queue listing (MCP / headless). Includes size + merge-state fields that
// the desktop PR list intentionally omits for latency.
// ─────────────────────────────────────────────────────────────────────────────

impl CliGitHub {
    /// List PRs for a repo with the fields the engine's review queue needs.
    ///
    /// `state` is passed to `gh pr list --state` (`open`, `closed`, `merged`,
    /// `all`).
    pub(crate) fn pr_list_queue(
        &self,
        owner: &str,
        repo: &str,
        state: &str,
        limit: usize,
    ) -> Result<Vec<crate::types::QueuePr>, GhError> {
        let repo_slug = format!("{owner}/{repo}");
        let limit = limit.clamp(1, 100).to_string();
        let out = self.run_gh_args(
            &[
                "pr",
                "list",
                "--repo",
                &repo_slug,
                "--state",
                state,
                "--limit",
                &limit,
                "--json",
                "number,title,headRefName,baseRefName,state,isDraft,author,reviewRequests,reviewDecision,mergeable,mergeStateStatus,additions,deletions,changedFiles,updatedAt,labels,url,latestReviews",
            ],
            None,
        )?;
        // A failed login lookup only loses the "review requested of me"
        // decoration; the queue itself is still worth listing.
        let me = self.current_login().ok().flatten();
        parse_pr_list_queue(&String::from_utf8_lossy(&out.body), me.as_deref())
            .map_err(GhError::from)
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_list_queue(
    owner: &str,
    repo: &str,
    state: &str,
    limit: usize,
) -> Result<Vec<crate::types::QueuePr>> {
    default_handle()
        .pr_list_queue(owner, repo, state, limit)
        .map_err(|error| with_gh_sentence("gh pr list failed", error))
}

/// Pure parser for `gh pr list --json …` review-queue payloads.
pub fn parse_pr_list_queue(
    json: &str,
    current_login: Option<&str>,
) -> Result<Vec<crate::types::QueuePr>> {
    #[derive(Deserialize)]
    struct Raw {
        number: u64,
        title: String,
        #[serde(default, rename = "headRefName")]
        head_ref_name: String,
        #[serde(default, rename = "baseRefName")]
        base_ref_name: String,
        state: String,
        #[serde(rename = "isDraft")]
        is_draft: bool,
        author: RawAuthor,
        #[serde(default, rename = "reviewRequests")]
        review_requests: Vec<RawReviewRequest>,
        #[serde(default, rename = "reviewDecision")]
        review_decision: Option<String>,
        #[serde(default)]
        mergeable: Option<String>,
        #[serde(default, rename = "mergeStateStatus")]
        merge_state_status: Option<String>,
        #[serde(default)]
        additions: u64,
        #[serde(default)]
        deletions: u64,
        #[serde(default, rename = "changedFiles")]
        changed_files: u64,
        #[serde(default, rename = "updatedAt")]
        updated_at: String,
        #[serde(default)]
        labels: Vec<RawLabel>,
        #[serde(default)]
        url: String,
        #[serde(default, rename = "latestReviews")]
        latest_reviews: Vec<RawReview>,
    }
    #[derive(Deserialize)]
    struct RawAuthor {
        login: Option<String>,
    }
    #[derive(Deserialize)]
    struct RawReviewRequest {
        #[serde(default)]
        login: Option<String>,
    }
    #[derive(Deserialize)]
    struct RawLabel {
        name: String,
    }
    #[derive(Deserialize)]
    struct RawReview {
        author: RawAuthor,
        state: String,
    }

    let raw: Vec<Raw> = serde_json::from_str(json).context("invalid gh pr list JSON")?;
    let me = current_login.map(|s| s.to_ascii_lowercase());
    Ok(raw
        .into_iter()
        .map(|r| {
            let reviewers: Vec<String> = r
                .review_requests
                .into_iter()
                .filter_map(|rr| rr.login)
                .collect();
            let review_requested_of_me = me
                .as_ref()
                .map(|m| reviewers.iter().any(|r| r.eq_ignore_ascii_case(m)))
                .unwrap_or(false);
            let my_latest_review_state = me.as_ref().and_then(|m| {
                r.latest_reviews
                    .iter()
                    .find(|rv| {
                        rv.author
                            .login
                            .as_deref()
                            .is_some_and(|l| l.eq_ignore_ascii_case(m))
                    })
                    .map(|rv| rv.state.clone())
            });
            crate::types::QueuePr {
                number: r.number,
                title: r.title,
                author: r.author.login.unwrap_or_default(),
                is_draft: r.is_draft,
                state: r.state,
                review_decision: r.review_decision.filter(|s| !s.is_empty()),
                mergeable: r.mergeable.filter(|s| !s.is_empty()),
                merge_state_status: r.merge_state_status.filter(|s| !s.is_empty()),
                additions: r.additions,
                deletions: r.deletions,
                changed_files: r.changed_files,
                updated_at: r.updated_at,
                labels: r.labels.into_iter().map(|l| l.name).collect(),
                reviewers,
                url: r.url,
                head_ref: r.head_ref_name,
                base_ref: r.base_ref_name,
                production_lines: None,
                review_requested_of_me,
                my_latest_review_state,
                checks_state: None,
            }
        })
        .collect())
}
