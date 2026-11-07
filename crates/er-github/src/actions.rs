//! Branch actions on a pull request: merge, update branch, close/reopen,
//! ready/draft, and deleting or restoring the head branch.
//!
//! Every call names the repository (`--repo` or a REST path) and none runs
//! against a working tree. `gh pr merge --delete-branch` from inside a checkout
//! deletes the local branch and switches to the base, which ADR 0020 forbids,
//! so the head branch is removed with a separate `git/refs` call instead
//! (ADR 0040).

use crate::classify::is_unknown_command;
use crate::cli::CliGitHub;
use crate::default_handle;
use crate::error::GhError;
use anyhow::Context;
use std::time::Duration;

/// Deserializes from `"merge"` / `"squash"` / `"rebase"`, so an unknown method
/// is refused where a request enters, before anything runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    fn flag(self) -> &'static str {
        match self {
            Self::Merge => "--merge",
            Self::Squash => "--squash",
            Self::Rebase => "--rebase",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrAction {
    /// `head_oid` pins the merge to the commit the reviewer saw; GitHub refuses
    /// if the branch moved since.
    Merge {
        method: MergeMethod,
        auto: bool,
        /// Merge now despite unmet requirements (`--admin`), for a viewer
        /// GitHub lets bypass the base branch's rules.
        admin: bool,
        head_oid: String,
    },
    DisableAutoMerge,
    UpdateBranch {
        rebase: bool,
    },
    Close,
    Reopen,
    MarkReady,
    ConvertToDraft,
}

/// `gh` arguments for `action` on PR `number` of `repo_slug` (`owner/repo`).
pub fn pr_action_args(
    repo_slug: &str,
    number: u64,
    action: &PrAction,
) -> anyhow::Result<Vec<String>> {
    let n = number.to_string();
    let mut args: Vec<&str> = match action {
        PrAction::Merge {
            method,
            auto,
            admin,
            head_oid,
        } => {
            if head_oid.trim().is_empty() {
                anyhow::bail!(
                    "Refusing to merge: the PR head commit is unknown. Refresh and try again."
                );
            }
            if *auto && *admin {
                anyhow::bail!("A bypass merge lands now, so it cannot also wait for auto-merge.");
            }
            let mut a = vec![
                "pr",
                "merge",
                &n,
                method.flag(),
                "--match-head-commit",
                head_oid,
            ];
            if *auto {
                a.push("--auto");
            }
            if *admin {
                a.push("--admin");
            }
            a
        }
        PrAction::DisableAutoMerge => vec!["pr", "merge", &n, "--disable-auto"],
        PrAction::UpdateBranch { rebase } => {
            let mut a = vec!["pr", "update-branch", &n];
            if *rebase {
                a.push("--rebase");
            }
            a
        }
        PrAction::Close => vec!["pr", "close", &n],
        PrAction::Reopen => vec!["pr", "reopen", &n],
        PrAction::MarkReady => vec!["pr", "ready", &n],
        PrAction::ConvertToDraft => vec!["pr", "ready", &n, "--undo"],
    };
    args.extend(["--repo", repo_slug]);
    Ok(args.into_iter().map(str::to_string).collect())
}

fn action_error(action: &PrAction, error: GhError) -> GhError {
    if matches!(action, PrAction::UpdateBranch { .. }) && is_unknown_command(&error) {
        return GhError::Local {
            message: "Your `gh` is too old for `gh pr update-branch`. Upgrade the GitHub CLI."
                .to_string(),
        };
    }
    error
}

// ── Head branch ─────────────────────────────────────────────────────────────

/// Only GitHub's 422 for a missing ref counts. A 404 is also what a viewer
/// without access to the repo gets, and reading that as "deleted" would
/// report a failed delete as done and offer Restore for a branch still there.
fn already_gone(error: &GhError) -> bool {
    matches!(
        error,
        GhError::Unprocessable { message } if message.contains("Reference does not exist")
    )
}

fn check_tip_matches(branch: &str, tip: &str, expected_tip: &str) -> Result<(), GhError> {
    if expected_tip.trim().is_empty() {
        return Err(GhError::Local {
            message: format!(
                "Cannot delete {branch}: the PR head commit is unknown. Refresh and try again."
            ),
        });
    }
    if tip != expected_tip {
        let short: String = tip.chars().take(7).collect();
        return Err(GhError::Local {
            message: format!(
                "{branch} has moved past this PR (now at {short}), so deleting it would drop \
                 commits no PR holds. It was kept."
            ),
        });
    }
    Ok(())
}

// ── Merge queue ─────────────────────────────────────────────────────────────

const MERGE_QUEUE_QUERY: &str = "query($owner: String!, $name: String!, $branch: String!) { \
    repository(owner: $owner, name: $name) { mergeQueue(branch: $branch) { id } } }";

const IN_MERGE_QUEUE_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { \
    repository(owner: $owner, name: $name) { pullRequest(number: $number) { isInMergeQueue } } }";

// ── Rule bypass ─────────────────────────────────────────────────────────────

const ADMIN_MERGE_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { \
    repository(owner: $owner, name: $name) { pullRequest(number: $number) { viewerCanMergeAsAdmin } } }";

fn parse_can_merge_as_admin(json: &str) -> Result<bool, GhError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| GhError::Local {
        message: "invalid admin merge JSON".to_string(),
    })?;
    v["data"]["repository"]["pullRequest"]["viewerCanMergeAsAdmin"]
        .as_bool()
        .ok_or_else(|| GhError::Local {
            message: "viewerCanMergeAsAdmin missing".to_string(),
        })
}

fn parse_in_merge_queue(json: &str) -> Result<bool, GhError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| GhError::Local {
        message: "invalid merge queue JSON".to_string(),
    })?;
    v["data"]["repository"]["pullRequest"]["isInMergeQueue"]
        .as_bool()
        .ok_or_else(|| GhError::Local {
            message: "merge queue status missing".to_string(),
        })
}

/// Distinct rulesets behind the rules `rules/branches/{base}` lists.
fn parse_branch_ruleset_ids(json: &str) -> Result<Vec<u64>, GhError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| GhError::Local {
        message: "invalid branch rules JSON".to_string(),
    })?;
    let rules = v.as_array().ok_or_else(|| GhError::Local {
        message: "branch rules response is not a list".to_string(),
    })?;
    let mut ids: Vec<u64> = rules
        .iter()
        .filter_map(|r| r["ruleset_id"].as_u64())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// `pull_requests_only` is enough: the bypass happens through a merge.
fn parse_ruleset_bypassable(json: &str) -> Result<bool, GhError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| GhError::Local {
        message: "invalid ruleset JSON".to_string(),
    })?;
    match v["current_user_can_bypass"].as_str() {
        Some("always" | "pull_requests_only") => Ok(true),
        Some(_) => Ok(false),
        None => Err(GhError::Local {
            message: "ruleset response has no current_user_can_bypass".to_string(),
        }),
    }
}

// ── Repository merge settings ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoMergeSettings {
    pub merge_commit_allowed: bool,
    pub squash_merge_allowed: bool,
    pub rebase_merge_allowed: bool,
    pub delete_branch_on_merge: bool,
    /// Auto-merge is off by default on GitHub; offering it on a repo that has
    /// not enabled it gives the user a button GitHub refuses.
    pub auto_merge_allowed: bool,
    /// `ADMIN`, `MAINTAIN`, `WRITE`, `TRIAGE`, `READ`.
    pub viewer_permission: Option<String>,
}

/// `gh repo view --json` has no `autoMergeAllowed`, so the settings come from
/// one GraphQL query instead.
const REPO_SETTINGS_QUERY: &str = "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { \
    mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed deleteBranchOnMerge autoMergeAllowed viewerPermission } }";

/// Parse the GraphQL response for [`REPO_SETTINGS_QUERY`].
pub fn parse_repo_merge_settings(json: &str) -> anyhow::Result<RepoMergeSettings> {
    let root: serde_json::Value =
        serde_json::from_str(json).context("invalid repo settings JSON")?;
    let v = &root["data"]["repository"];
    if !v.is_object() {
        anyhow::bail!("repo settings response has no repository");
    }
    // A missing merge-method flag reads as allowed, so the merge box never
    // hides a method GitHub would accept; GitHub still rejects a disallowed one.
    let allowed = |key: &str| v[key].as_bool().unwrap_or(true);
    Ok(RepoMergeSettings {
        merge_commit_allowed: allowed("mergeCommitAllowed"),
        squash_merge_allowed: allowed("squashMergeAllowed"),
        rebase_merge_allowed: allowed("rebaseMergeAllowed"),
        delete_branch_on_merge: v["deleteBranchOnMerge"].as_bool().unwrap_or(false),
        auto_merge_allowed: v["autoMergeAllowed"].as_bool().unwrap_or(false),
        viewer_permission: v["viewerPermission"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

/// Owner and repo are case-insensitive on GitHub.
fn repo_key(owner: &str, repo: &str) -> String {
    format!("{owner}/{repo}").to_ascii_lowercase()
}

/// Branch names, unlike owner and repo, are case-sensitive.
fn branch_key(owner: &str, repo: &str, branch: &str) -> String {
    format!("{}/{branch}", repo_key(owner, repo))
}

const BRANCH_EXISTS_TTL: Duration = Duration::from_mins(5);
const PENDING_DELETE_TTL: Duration = Duration::from_secs(60);

fn branch_exists_ttl(exists: bool, deleting_itself: bool) -> Duration {
    if exists && deleting_itself {
        PENDING_DELETE_TTL
    } else {
        BRANCH_EXISTS_TTL
    }
}

/// Percent-encode a branch name for a REST path, keeping `/` so
/// `feat/x` stays `heads/feat/x`.
fn encode_ref(branch: &str) -> String {
    let mut out = String::with_capacity(branch.len());
    for b in branch.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl CliGitHub {
    /// Commit `branch` points at, or `None` when GitHub answers 404.
    fn remote_branch_tip(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<Option<String>, GhError> {
        // Singular `git/ref` matches exactly; plural `git/refs` would also match
        // every branch that has this one as a prefix.
        let path = format!("repos/{owner}/{repo}/git/ref/heads/{}", encode_ref(branch));
        let args = vec![
            "api".to_string(),
            path,
            "--jq".to_string(),
            ".object.sha".to_string(),
        ];
        match self.run_gh(&args, None, None) {
            Ok(output) => Ok(Some(
                String::from_utf8_lossy(&output.body).trim().to_string(),
            )),
            Err(GhError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl CliGitHub {
    pub(crate) fn run_pr_action(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        action: &PrAction,
    ) -> Result<(), GhError> {
        let args = pr_action_args(&format!("{owner}/{repo}"), number, action).map_err(|error| {
            GhError::Local {
                message: error.to_string(),
            }
        })?;
        self.run_gh(&args, None, None)
            .map(|_| ())
            .map_err(|error| action_error(action, error))
    }

    pub(crate) fn delete_remote_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        expected_tip: &str,
    ) -> Result<(), GhError> {
        // A missing tip is left to the DELETE below: a 404 here can also mean no
        // access, which must surface as a failure.
        if let Some(tip) = self.remote_branch_tip(owner, repo, branch)? {
            check_tip_matches(branch, &tip, expected_tip)?;
        }
        let path = format!("repos/{owner}/{repo}/git/refs/heads/{}", encode_ref(branch));
        let args = vec![
            "api".to_string(),
            "-X".to_string(),
            "DELETE".to_string(),
            path,
        ];
        match self.run_gh(&args, None, None) {
            Ok(_) => {}
            Err(error) if already_gone(&error) => {}
            Err(error) => return Err(error),
        }
        self.cache()
            .branch_exists
            .put(branch_key(owner, repo, branch), false);
        Ok(())
    }

    pub(crate) fn pr_state(&self, owner: &str, repo: &str, number: u64) -> Result<String, GhError> {
        let slug = format!("{owner}/{repo}");
        let args = vec![
            "pr".to_string(),
            "view".to_string(),
            number.to_string(),
            "--repo".to_string(),
            slug,
            "--json".to_string(),
            "state".to_string(),
            "--jq".to_string(),
            ".state".to_string(),
        ];
        let out = self.gh_ok(&args, None)?;
        Ok(out.trim().to_string())
    }

    pub(crate) fn base_has_merge_queue(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
    ) -> Result<bool, GhError> {
        let out = self.graphql(
            MERGE_QUEUE_QUERY,
            &[("owner", owner), ("name", repo), ("branch", base)],
            &[],
        )?;
        let v: serde_json::Value = serde_json::from_str(&out).map_err(|_| GhError::Local {
            message: "invalid merge queue JSON".to_string(),
        })?;
        Ok(v["data"]["repository"]["mergeQueue"].is_object())
    }

    pub(crate) fn cached_base_has_merge_queue(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
    ) -> Option<bool> {
        self.cache()
            .merge_queue
            .get_or_fetch(format!("{}:{base}", repo_key(owner, repo)), || {
                self.base_has_merge_queue(owner, repo, base)
            })
            .ok()
    }

    pub(crate) fn pr_in_merge_queue(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<bool, GhError> {
        let n = number.to_string();
        let out = self.graphql(
            IN_MERGE_QUEUE_QUERY,
            &[("owner", owner), ("name", repo)],
            &[("number", &n)],
        )?;
        parse_in_merge_queue(&out)
    }

    pub(crate) fn viewer_can_bypass(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        number: u64,
    ) -> Result<bool, GhError> {
        let n = number.to_string();
        let out = self.graphql(
            ADMIN_MERGE_QUERY,
            &[("owner", owner), ("name", repo)],
            &[("number", &n)],
        )?;
        if parse_can_merge_as_admin(&out)? {
            return Ok(true);
        }
        let path = format!(
            "repos/{owner}/{repo}/rules/branches/{}?per_page=100",
            encode_ref(base)
        );
        let ids = parse_branch_ruleset_ids(&self.gh_ok(&["api".to_string(), path], None)?)?;
        if ids.is_empty() {
            return Ok(false);
        }
        for id in ids {
            let path = format!("repos/{owner}/{repo}/rulesets/{id}");
            if !parse_ruleset_bypassable(&self.gh_ok(&["api".to_string(), path], None)?)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) fn restore_remote_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        sha: &str,
    ) -> Result<(), GhError> {
        if sha.trim().is_empty() {
            return Err(GhError::Local {
                message: format!("Cannot restore {branch}: the PR head commit is unknown."),
            });
        }
        let path = format!("repos/{owner}/{repo}/git/refs");
        let ref_field = format!("ref=refs/heads/{branch}");
        let sha_field = format!("sha={sha}");
        let args = vec![
            "api".to_string(),
            "-X".to_string(),
            "POST".to_string(),
            path,
            "-f".to_string(),
            ref_field,
            "-f".to_string(),
            sha_field,
        ];
        self.run_gh(&args, None, None)?;
        self.cache()
            .branch_exists
            .put(branch_key(owner, repo, branch), true);
        Ok(())
    }

    pub(crate) fn remote_branch_exists(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<bool, GhError> {
        Ok(self.remote_branch_tip(owner, repo, branch)?.is_some())
    }

    pub(crate) fn repo_merge_settings(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<RepoMergeSettings, GhError> {
        let out = self.graphql(
            REPO_SETTINGS_QUERY,
            &[("owner", owner), ("name", repo)],
            &[],
        )?;
        parse_repo_merge_settings(&out).map_err(|error| GhError::Local {
            message: error.to_string(),
        })
    }

    pub(crate) fn cached_repo_merge_settings(
        &self,
        owner: &str,
        repo: &str,
    ) -> Option<RepoMergeSettings> {
        self.cache()
            .repo_settings
            .get_or_fetch(repo_key(owner, repo), || {
                self.repo_merge_settings(owner, repo)
            })
            .ok()
    }

    pub(crate) fn cached_remote_branch_exists(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        deleting_itself: bool,
    ) -> Option<bool> {
        let key = branch_key(owner, repo, branch);
        self.cache()
            .branch_exists
            .get_or_fetch_for(
                key,
                |exists| branch_exists_ttl(*exists, deleting_itself),
                || self.remote_branch_exists(owner, repo, branch),
            )
            .ok()
    }

    pub(crate) fn cached_viewer_can_bypass(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        number: u64,
    ) -> Option<bool> {
        self.cache()
            .bypass
            .get_or_fetch(branch_key(owner, repo, base), || {
                self.viewer_can_bypass(owner, repo, base, number)
            })
            .ok()
    }
}

// ── Compatibility wrappers (kept for the front ends and lane F) ─────────────

pub fn run_pr_action(
    owner: &str,
    repo: &str,
    number: u64,
    action: &PrAction,
) -> anyhow::Result<()> {
    default_handle()
        .run_pr_action(owner, repo, number, action)
        .map_err(anyhow::Error::from)
}

pub fn gh_pr_state(owner: &str, repo: &str, number: u64) -> anyhow::Result<String> {
    default_handle()
        .pr_state(owner, repo, number)
        .map_err(|error| anyhow::anyhow!("gh pr view failed: {error}"))
}

pub fn gh_delete_remote_branch(
    owner: &str,
    repo: &str,
    branch: &str,
    expected_tip: &str,
) -> anyhow::Result<()> {
    default_handle()
        .delete_remote_branch(owner, repo, branch, expected_tip)
        .map_err(|error| match error {
            GhError::Local { .. } => anyhow::Error::from(error),
            error => anyhow::anyhow!("Failed to delete branch {branch}: {error}"),
        })
}

pub fn gh_restore_remote_branch(
    owner: &str,
    repo: &str,
    branch: &str,
    sha: &str,
) -> anyhow::Result<()> {
    default_handle()
        .restore_remote_branch(owner, repo, branch, sha)
        .map_err(|error| match error {
            GhError::Local { .. } => anyhow::Error::from(error),
            error => anyhow::anyhow!("Failed to restore branch {branch}: {error}"),
        })
}

pub fn gh_pr_in_merge_queue(owner: &str, repo: &str, number: u64) -> anyhow::Result<bool> {
    default_handle()
        .pr_in_merge_queue(owner, repo, number)
        .map_err(|error| anyhow::anyhow!("merge queue status query failed: {error}"))
}

pub fn cached_base_has_merge_queue(owner: &str, repo: &str, base: &str) -> Option<bool> {
    default_handle().cached_base_has_merge_queue(owner, repo, base)
}

pub fn cached_viewer_can_bypass(owner: &str, repo: &str, base: &str, number: u64) -> Option<bool> {
    default_handle().cached_viewer_can_bypass(owner, repo, base, number)
}

pub fn cached_remote_branch_exists(
    owner: &str,
    repo: &str,
    branch: &str,
    deleting_itself: bool,
) -> Option<bool> {
    default_handle().cached_remote_branch_exists(owner, repo, branch, deleting_itself)
}

pub fn cached_repo_merge_settings(owner: &str, repo: &str) -> Option<RepoMergeSettings> {
    default_handle().cached_repo_merge_settings(owner, repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(action: PrAction) -> Vec<String> {
        pr_action_args("o/r", 7, &action).unwrap()
    }

    #[test]
    fn merge_pins_head_and_names_repo() {
        let a = args(PrAction::Merge {
            method: MergeMethod::Squash,
            auto: false,
            admin: false,
            head_oid: "abc".into(),
        });
        assert_eq!(
            a,
            [
                "pr",
                "merge",
                "7",
                "--squash",
                "--match-head-commit",
                "abc",
                "--repo",
                "o/r"
            ]
        );
    }

    #[test]
    fn merge_never_deletes_the_branch_through_gh() {
        for method in [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase] {
            for auto in [false, true] {
                let a = args(PrAction::Merge {
                    method,
                    auto,
                    admin: false,
                    head_oid: "abc".into(),
                });
                assert!(
                    !a.iter().any(|s| s == "--delete-branch" || s == "-d"),
                    "{a:?}"
                );
                assert!(a.contains(&"--repo".to_string()));
            }
        }
    }

    #[test]
    fn auto_merge_adds_auto_flag() {
        let a = args(PrAction::Merge {
            method: MergeMethod::Rebase,
            auto: true,
            admin: false,
            head_oid: "abc".into(),
        });
        assert!(a.contains(&"--rebase".to_string()));
        assert!(a.contains(&"--auto".to_string()));
    }

    #[test]
    fn bypass_merge_adds_admin_flag_and_keeps_the_head_pin() {
        let a = args(PrAction::Merge {
            method: MergeMethod::Squash,
            auto: false,
            admin: true,
            head_oid: "abc".into(),
        });
        assert_eq!(
            a,
            [
                "pr",
                "merge",
                "7",
                "--squash",
                "--match-head-commit",
                "abc",
                "--admin",
                "--repo",
                "o/r"
            ]
        );
    }

    #[test]
    fn bypass_merge_cannot_also_be_auto() {
        let err = pr_action_args(
            "o/r",
            7,
            &PrAction::Merge {
                method: MergeMethod::Squash,
                auto: true,
                admin: true,
                head_oid: "abc".into(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("auto-merge"));
    }

    #[test]
    fn branch_rules_name_each_ruleset_once() {
        let json = r#"[
            {"type":"deletion","ruleset_id":13},
            {"type":"pull_request","ruleset_id":13},
            {"type":"required_status_checks","ruleset_id":4},
            {"type":"update"}
        ]"#;
        assert_eq!(parse_branch_ruleset_ids(json).unwrap(), [4, 13]);
        assert!(parse_branch_ruleset_ids("[]").unwrap().is_empty());
        assert!(parse_branch_ruleset_ids(r#"{"message":"x"}"#).is_err());
    }

    #[test]
    fn ruleset_bypass_counts_pull_request_bypass_actors() {
        let mode = |m: &str| format!(r#"{{"id":1,"current_user_can_bypass":"{m}"}}"#);
        assert!(parse_ruleset_bypassable(&mode("always")).unwrap());
        assert!(parse_ruleset_bypassable(&mode("pull_requests_only")).unwrap());
        assert!(!parse_ruleset_bypassable(&mode("never")).unwrap());
        assert!(parse_ruleset_bypassable(r#"{"id":1}"#).is_err());
    }

    #[test]
    fn merge_without_head_commit_is_refused() {
        let err = pr_action_args(
            "o/r",
            7,
            &PrAction::Merge {
                method: MergeMethod::Merge,
                auto: false,
                admin: false,
                head_oid: " ".into(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("head commit is unknown"));
    }

    #[test]
    fn other_actions_build_expected_args() {
        assert_eq!(
            args(PrAction::DisableAutoMerge),
            ["pr", "merge", "7", "--disable-auto", "--repo", "o/r"]
        );
        assert_eq!(
            args(PrAction::UpdateBranch { rebase: false }),
            ["pr", "update-branch", "7", "--repo", "o/r"]
        );
        assert_eq!(
            args(PrAction::UpdateBranch { rebase: true }),
            ["pr", "update-branch", "7", "--rebase", "--repo", "o/r"]
        );
        assert_eq!(args(PrAction::Close), ["pr", "close", "7", "--repo", "o/r"]);
        assert_eq!(
            args(PrAction::Reopen),
            ["pr", "reopen", "7", "--repo", "o/r"]
        );
        assert_eq!(
            args(PrAction::MarkReady),
            ["pr", "ready", "7", "--repo", "o/r"]
        );
        assert_eq!(
            args(PrAction::ConvertToDraft),
            ["pr", "ready", "7", "--undo", "--repo", "o/r"]
        );
    }

    #[test]
    fn merge_method_deserializes_and_refuses_unknown_methods() {
        let parse = |s: &str| serde_json::from_str::<MergeMethod>(s).ok();
        assert_eq!(parse(r#""squash""#), Some(MergeMethod::Squash));
        assert_eq!(parse(r#""merge""#), Some(MergeMethod::Merge));
        assert_eq!(parse(r#""rebase""#), Some(MergeMethod::Rebase));
        assert_eq!(parse(r#""fast-forward""#), None);
    }

    #[test]
    fn encode_ref_keeps_slashes_and_escapes_the_rest() {
        assert_eq!(encode_ref("feat/x-1.2_y"), "feat/x-1.2_y");
        assert_eq!(encode_ref("fix#12 a"), "fix%2312%20a");
        assert_eq!(encode_ref("ü"), "%C3%BC");
    }

    #[test]
    fn update_branch_on_old_gh_says_to_upgrade() {
        let error = action_error(
            &PrAction::UpdateBranch { rebase: false },
            GhError::Tool {
                status: 1,
                stderr: "unknown command \"update-branch\"".into(),
            },
        );
        assert!(error.to_string().contains("Upgrade the GitHub CLI"));
        assert_eq!(
            action_error(
                &PrAction::Close,
                GhError::Tool {
                    status: 1,
                    stderr: String::new(),
                }
            )
            .to_string(),
            "gh exited with an error"
        );
    }

    #[test]
    fn repo_settings_parse() {
        let s = parse_repo_merge_settings(
            r#"{"data": {"repository": {"mergeCommitAllowed": false, "squashMergeAllowed": true,
                "rebaseMergeAllowed": false, "deleteBranchOnMerge": true, "autoMergeAllowed": true,
                "viewerPermission": "WRITE"}}}"#,
        )
        .unwrap();
        assert_eq!(
            s,
            RepoMergeSettings {
                merge_commit_allowed: false,
                squash_merge_allowed: true,
                rebase_merge_allowed: false,
                delete_branch_on_merge: true,
                auto_merge_allowed: true,
                viewer_permission: Some("WRITE".into()),
            }
        );
    }

    #[test]
    fn repo_settings_missing_fields_allow_every_method_but_not_auto_merge() {
        let s = parse_repo_merge_settings(r#"{"data": {"repository": {}}}"#).unwrap();
        assert!(s.merge_commit_allowed && s.squash_merge_allowed && s.rebase_merge_allowed);
        assert!(!s.delete_branch_on_merge);
        assert!(!s.auto_merge_allowed);
        assert!(s.viewer_permission.is_none());
    }

    #[test]
    fn repo_settings_without_a_repository_is_an_error() {
        assert!(parse_repo_merge_settings(r#"{"data": {"repository": null}}"#).is_err());
        assert!(parse_repo_merge_settings("not json").is_err());
    }

    #[test]
    fn graphql_keeps_names_as_strings_and_numbers_typed() {
        let a = crate::cli::graphql_args("Q", &[("name", "123")], &[("number", "7")]);
        assert_eq!(
            a,
            ["api", "graphql", "-f", "query=Q", "-f", "name=123", "-F", "number=7"]
        );
    }

    #[test]
    fn admin_merge_flag_is_read_from_the_pull_request() {
        let yes = r#"{"data":{"repository":{"pullRequest":{"viewerCanMergeAsAdmin":true}}}}"#;
        let no = r#"{"data":{"repository":{"pullRequest":{"viewerCanMergeAsAdmin":false}}}}"#;
        assert!(parse_can_merge_as_admin(yes).unwrap());
        assert!(!parse_can_merge_as_admin(no).unwrap());
        assert!(parse_can_merge_as_admin(r#"{"data":{"repository":null}}"#).is_err());
    }

    #[test]
    fn in_merge_queue_parses_and_rejects_a_missing_answer() {
        let json = r#"{"data":{"repository":{"pullRequest":{"isInMergeQueue":true}}}}"#;
        assert!(parse_in_merge_queue(json).unwrap());
        // A null PR (no access) must not read as "not queued".
        assert!(parse_in_merge_queue(r#"{"data":{"repository":{"pullRequest":null}}}"#).is_err());
    }

    #[test]
    fn present_branch_on_an_auto_deleting_repo_is_kept_only_briefly() {
        assert_eq!(branch_exists_ttl(true, true), PENDING_DELETE_TTL);
        assert_eq!(branch_exists_ttl(true, false), BRANCH_EXISTS_TTL);
        assert_eq!(branch_exists_ttl(false, true), BRANCH_EXISTS_TTL);
    }

    #[test]
    fn a_branch_that_moved_past_the_pr_is_kept() {
        let err = check_tip_matches("dev", "newcommit123", "prhead").unwrap_err();
        assert!(err.to_string().contains("It was kept"), "{err}");
        assert!(err.to_string().contains("newcomm"), "{err}");
        assert!(check_tip_matches("feat/x", "prhead", "prhead").is_ok());
        assert!(check_tip_matches("feat/x", "prhead", "").is_err());
    }

    #[test]
    fn already_deleted_branch_counts_as_deleted() {
        assert!(already_gone(&GhError::Unprocessable {
            message: "Reference does not exist (HTTP 422)".into()
        }));
        // A 404 can mean "no access"; a failed delete must not read as done.
        assert!(!already_gone(&GhError::NotFound {
            message: "Not Found (HTTP 404)".into()
        }));
        assert!(!already_gone(&GhError::Forbidden {
            message: "Resource not accessible by integration (HTTP 403)".into()
        }));
    }

    #[test]
    fn branch_key_ignores_repo_case_but_not_branch_case() {
        assert_eq!(
            branch_key("Org", "Repo", "Feat/X"),
            branch_key("org", "repo", "Feat/X")
        );
        assert_ne!(branch_key("o", "r", "Feat"), branch_key("o", "r", "feat"));
    }
}
