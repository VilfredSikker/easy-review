//! PR commit listings, local and remote.

use anyhow::{Context, Result};

use crate::{default_handle, CliGitHub, GhError};

/// Parse `gh pr view --json commits` output into commit metadata.
///
/// GitHub returns PR commits in chronological order. The desktop scroller
/// matches GitHub's PR Commits tab by showing newest first.
pub fn parse_pr_commits_view_json(bytes: &[u8], limit: usize) -> Result<Vec<er_git::CommitInfo>> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("Failed to parse PR commits JSON")?;
    Ok(parse_pr_commits_view_value(&value, limit))
}

pub fn parse_pr_commits_view_value(
    value: &serde_json::Value,
    limit: usize,
) -> Vec<er_git::CommitInfo> {
    let Some(commits) = value.get("commits").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    commits
        .iter()
        .rev()
        .take(limit)
        .filter_map(|commit| {
            let oid = commit.get("oid").and_then(|v| v.as_str())?;
            let author = commit
                .get("authors")
                .and_then(|v| v.as_array())
                .map(|authors| {
                    authors
                        .iter()
                        .filter_map(|author| {
                            author
                                .get("login")
                                .and_then(|v| v.as_str())
                                .or_else(|| author.get("name").and_then(|v| v.as_str()))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let date = commit
                .get("committedDate")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            // `gh pr view --json commits` has no diffstat; only the GraphQL
            // listing (`gh_pr_commits_remote`) carries these fields.
            let count = |key: &str| {
                commit
                    .get(key)
                    .and_then(|v| v.as_u64())
                    .and_then(|n| usize::try_from(n).ok())
            };
            let (adds, dels) = (count("additions"), count("deletions"));
            Some(er_git::CommitInfo {
                hash: oid.to_string(),
                short_hash: oid.chars().take(7).collect(),
                subject: commit
                    .get("messageHeadline")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                author,
                date: date.clone(),
                relative_date: date,
                file_count: count("changedFiles").unwrap_or(0),
                adds: adds.unwrap_or(0),
                dels: dels.unwrap_or(0),
                is_merge: false,
                diffstat_known: adds.is_some() && dels.is_some(),
            })
        })
        .collect()
}

/// GraphQL query behind [`gh_pr_commits_remote`]. `gh pr view --json commits`
/// cannot return a commit's diffstat, and a remote tab has no object DB to
/// read one from, so the remote listing asks GraphQL directly — still one `gh`
/// process per fetch, paginated by `gh api --paginate`.
const PR_COMMITS_GRAPHQL: &str =
    "query($owner: String!, $repo: String!, $number: Int!, $endCursor: String) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      commits(first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes {
          commit {
            oid
            messageHeadline
            committedDate
            additions
            deletions
            changedFilesIfAvailable
            authors(first: 10) { nodes { name user { login } } }
          }
        }
      }
    }
  }
}";

/// Parse `gh api graphql --paginate` output for [`PR_COMMITS_GRAPHQL`] — one
/// JSON document per page, concatenated — into the newest-first list
/// [`parse_pr_commits_view_value`] produces, each commit with its diffstat.
pub fn parse_pr_commits_graphql(bytes: &[u8], limit: usize) -> Result<Vec<er_git::CommitInfo>> {
    let mut commits = Vec::new();
    for page in serde_json::Deserializer::from_slice(bytes).into_iter::<serde_json::Value>() {
        let page = page.context("Failed to parse PR commits GraphQL JSON")?;
        let nodes = page
            .pointer("/data/repository/pullRequest/commits/nodes")
            .and_then(|v| v.as_array())
            .map(Vec::as_slice)
            .unwrap_or_default();
        for c in nodes.iter().filter_map(|node| node.get("commit")) {
            let authors: Vec<serde_json::Value> = c
                .pointer("/authors/nodes")
                .and_then(|v| v.as_array())
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|a| serde_json::json!({ "login": a.pointer("/user/login"), "name": a.get("name") }))
                .collect();
            commits.push(serde_json::json!({
                "oid": c.get("oid"),
                "messageHeadline": c.get("messageHeadline"),
                "committedDate": c.get("committedDate"),
                "authors": authors,
                "additions": c.get("additions"),
                "deletions": c.get("deletions"),
                "changedFiles": c.get("changedFilesIfAvailable"),
            }));
        }
    }
    Ok(parse_pr_commits_view_value(
        &serde_json::json!({ "commits": commits }),
        limit,
    ))
}

impl CliGitHub {
    /// Fetch the commits on a local-clone PR using the GitHub API via `gh`.
    pub(crate) fn pr_commits(
        &self,
        repo_root: &str,
        number: u64,
        limit: usize,
    ) -> Result<Vec<er_git::CommitInfo>, GhError> {
        let out = self.run_gh_args(
            &["pr", "view", &number.to_string(), "--json", "commits"],
            Some(repo_root),
        )?;
        parse_pr_commits_view_json(&out.body, limit).map_err(GhError::from)
    }

    /// A PR's base branch and commit list, from one `gh pr view`.
    pub(crate) fn pr_base_branch_and_commits(
        &self,
        repo_root: &str,
        number: u64,
        limit: usize,
    ) -> Result<(String, Vec<er_git::CommitInfo>), GhError> {
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &number.to_string(),
                "--json",
                "baseRefName,commits",
            ],
            Some(repo_root),
        )?;
        let value: serde_json::Value =
            serde_json::from_slice(&out.body).map_err(|error| GhError::Local {
                message: format!("Failed to parse gh pr view JSON: {error}"),
            })?;
        let base = value
            .get("baseRefName")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        if base.is_empty() {
            return Err(GhError::Local {
                message: format!("PR #{number} has no base branch"),
            });
        }
        let commits = parse_pr_commits_view_value(&value, limit);
        Ok((base, commits))
    }
}

/// Compatibility wrappers for front ends and engine callers.
pub fn gh_pr_commits(
    repo_root: &str,
    number: u64,
    limit: usize,
) -> Result<Vec<er_git::CommitInfo>> {
    default_handle()
        .pr_commits(repo_root, number, limit)
        .map_err(anyhow::Error::from)
}

pub fn gh_pr_base_branch_and_commits(
    repo_root: &str,
    number: u64,
    limit: usize,
) -> Result<(String, Vec<er_git::CommitInfo>)> {
    default_handle()
        .pr_base_branch_and_commits(repo_root, number, limit)
        .map_err(anyhow::Error::from)
}

/// Fetch the list of commits on a remote PR (no local clone required).
///
/// Returns commits in reverse-chronological order (newest first), capped at
/// `limit`, each with its diffstat — see [`PR_COMMITS_GRAPHQL`] for why this one
/// listing is GraphQL rather than `gh pr view --json commits`.
///
/// On any failure (gh unavailable, no auth, etc.) returns an empty list so the
/// caller degrades gracefully.
impl CliGitHub {
    /// The commits on a remote PR, through the paginated GraphQL query.
    pub(crate) fn commits_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        limit: usize,
    ) -> Result<Vec<er_git::CommitInfo>, GhError> {
        let query = format!("query={PR_COMMITS_GRAPHQL}");
        let owner_arg = format!("owner={owner}");
        let repo_arg = format!("repo={repo}");
        let number_arg = format!("number={number}");
        let args = vec![
            "api".to_string(),
            "graphql".to_string(),
            "--paginate".to_string(),
            "-f".to_string(),
            query,
            "-f".to_string(),
            owner_arg,
            "-f".to_string(),
            repo_arg,
            "-F".to_string(),
            number_arg,
        ];
        let out = self.run_gh(&args, None, None)?;
        parse_pr_commits_graphql(&out.body, limit).map_err(GhError::from)
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_commits_remote(
    owner: &str,
    repo: &str,
    number: u64,
    limit: usize,
) -> Vec<er_git::CommitInfo> {
    default_handle()
        .commits_remote(owner, repo, number, limit)
        .unwrap_or_default()
}
