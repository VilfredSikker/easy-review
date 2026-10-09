//! Branch actions on a pull request: merge, update branch, close/reopen,
//! ready/draft, and deleting or restoring the head branch.
//!
//! Every call names the repository (`--repo` or a REST path) and none runs
//! against a working tree. `gh pr merge --delete-branch` from inside a checkout
//! deletes the local branch and switches to the base, which ADR 0020 forbids,
//! so the head branch is removed with a separate `git/refs` call instead
//! (ADR 0040).

use crate::proc::CommandTimeoutExt;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
pub fn pr_action_args(repo_slug: &str, number: u64, action: &PrAction) -> Result<Vec<String>> {
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

pub fn run_pr_action(owner: &str, repo: &str, number: u64, action: &PrAction) -> Result<()> {
    let args = pr_action_args(&format!("{owner}/{repo}"), number, action)?;
    let output = gh(&args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{}", action_error(action, stderr.trim()));
    }
    Ok(())
}

fn action_error(action: &PrAction, stderr: &str) -> String {
    if matches!(action, PrAction::UpdateBranch { .. }) && stderr.contains("unknown command") {
        return "Your `gh` is too old for `gh pr update-branch`. Upgrade the GitHub CLI."
            .to_string();
    }
    if stderr.is_empty() {
        "gh exited with an error".to_string()
    } else {
        stderr.to_string()
    }
}

// ── Head branch ─────────────────────────────────────────────────────────────

/// Delete `branch` on `owner/repo`. A branch that is already gone counts as
/// deleted: a repo that deletes merged branches itself gets there first, and
/// reporting that as a failure would read as a failed merge.
///
/// `expected_tip` is the PR's head commit. A long-lived branch (`dev`,
/// `staging`) can gain commits after its PR closed; deleting it then would
/// drop work no PR holds, and Restore would bring back only the old head. So
/// the branch is deleted only while it still points at the PR head, as
/// GitHub's own "can be safely deleted" requires.
pub fn gh_delete_remote_branch(
    owner: &str,
    repo: &str,
    branch: &str,
    expected_tip: &str,
) -> Result<()> {
    // A missing tip is left to the DELETE below: a 404 here can also mean no
    // access, which must surface as a failure.
    if let Some(tip) = gh_remote_branch_tip(owner, repo, branch)? {
        check_tip_matches(branch, &tip, expected_tip)?;
    }
    let path = format!("repos/{owner}/{repo}/git/refs/heads/{}", encode_ref(branch));
    let output = gh(&["api", "-X", "DELETE", &path])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !is_already_gone(&stderr) {
            anyhow::bail!("Failed to delete branch {branch}: {}", stderr.trim());
        }
    }
    BRANCH_EXISTS_CACHE.put(branch_key(owner, repo, branch), false);
    Ok(())
}

fn check_tip_matches(branch: &str, tip: &str, expected_tip: &str) -> Result<()> {
    if expected_tip.trim().is_empty() {
        anyhow::bail!(
            "Cannot delete {branch}: the PR head commit is unknown. Refresh and try again."
        );
    }
    if tip != expected_tip {
        let short: String = tip.chars().take(7).collect();
        anyhow::bail!(
            "{branch} has moved past this PR (now at {short}), so deleting it would drop commits no PR holds. It was kept."
        );
    }
    Ok(())
}

/// Only GitHub's 422 for a missing ref counts. A 404 is also what a viewer
/// without access to the repo gets, and reading that as "deleted" would
/// report a failed delete as done and offer Restore for a branch still there.
fn is_already_gone(stderr: &str) -> bool {
    stderr.contains("Reference does not exist")
}

/// State of PR `number` (`OPEN`, `MERGED`, `CLOSED`), read live.
pub fn gh_pr_state(owner: &str, repo: &str, number: u64) -> Result<String> {
    let slug = format!("{owner}/{repo}");
    let n = number.to_string();
    let out = gh_ok(
        &[
            "pr", "view", &n, "--repo", &slug, "--json", "state", "--jq", ".state",
        ],
        "gh pr view failed",
    )?;
    Ok(out.trim().to_string())
}

// ── Merge queue ─────────────────────────────────────────────────────────────

const MERGE_QUEUE_QUERY: &str = "query($owner: String!, $name: String!, $branch: String!) { \
    repository(owner: $owner, name: $name) { mergeQueue(branch: $branch) { id } } }";

const IN_MERGE_QUEUE_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { \
    repository(owner: $owner, name: $name) { pullRequest(number: $number) { isInMergeQueue } } }";

/// Whether `base` has a merge queue. On such a branch `gh pr merge` only
/// enqueues the PR, so the PR stays open until the queue merges it.
pub fn gh_base_has_merge_queue(owner: &str, repo: &str, base: &str) -> Result<bool> {
    let out = graphql(
        MERGE_QUEUE_QUERY,
        &[("owner", owner), ("name", repo), ("branch", base)],
        &[],
        "merge queue query failed",
    )?;
    let v: serde_json::Value = serde_json::from_str(&out).context("invalid merge queue JSON")?;
    Ok(v["data"]["repository"]["mergeQueue"].is_object())
}

/// Cached [`gh_base_has_merge_queue`]; queue setup changes about as often as
/// repo settings do.
pub fn cached_base_has_merge_queue(owner: &str, repo: &str, base: &str) -> Option<bool> {
    MERGE_QUEUE_CACHE.get_or_fetch(format!("{}:{base}", repo_key(owner, repo)), || {
        gh_base_has_merge_queue(owner, repo, base).ok()
    })
}

static MERGE_QUEUE_CACHE: TtlCache<bool> = TtlCache::new(REPO_SETTINGS_TTL);

/// Whether PR `number` sits in its base branch's merge queue.
pub fn gh_pr_in_merge_queue(owner: &str, repo: &str, number: u64) -> Result<bool> {
    let n = number.to_string();
    let out = graphql(
        IN_MERGE_QUEUE_QUERY,
        &[("owner", owner), ("name", repo)],
        &[("number", &n)],
        "merge queue status query failed",
    )?;
    parse_in_merge_queue(&out)
}

fn parse_in_merge_queue(json: &str) -> Result<bool> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid merge queue JSON")?;
    v["data"]["repository"]["pullRequest"]["isInMergeQueue"]
        .as_bool()
        .context("merge queue status missing")
}

// ── Rule bypass ─────────────────────────────────────────────────────────────

const ADMIN_MERGE_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { \
    repository(owner: $owner, name: $name) { pullRequest(number: $number) { viewerCanMergeAsAdmin } } }";

/// Whether the viewer may merge a PR into `base` without its requirements
/// met (`gh pr merge --admin`). `viewerCanMergeAsAdmin` covers classic branch
/// protection only and reads false for a ruleset bypass actor, so the
/// rulesets on `base` are asked too.
pub fn gh_viewer_can_bypass(owner: &str, repo: &str, base: &str, number: u64) -> Result<bool> {
    let n = number.to_string();
    let out = graphql(
        ADMIN_MERGE_QUERY,
        &[("owner", owner), ("name", repo)],
        &[("number", &n)],
        "admin merge query failed",
    )?;
    if parse_can_merge_as_admin(&out)? {
        return Ok(true);
    }
    let path = format!(
        "repos/{owner}/{repo}/rules/branches/{}?per_page=100",
        encode_ref(base)
    );
    let ids = parse_branch_ruleset_ids(&gh_ok(&["api", &path], "branch rules query failed")?)?;
    if ids.is_empty() {
        return Ok(false);
    }
    for id in ids {
        let path = format!("repos/{owner}/{repo}/rulesets/{id}");
        if !parse_ruleset_bypassable(&gh_ok(&["api", &path], "ruleset query failed")?)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn parse_can_merge_as_admin(json: &str) -> Result<bool> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid admin merge JSON")?;
    v["data"]["repository"]["pullRequest"]["viewerCanMergeAsAdmin"]
        .as_bool()
        .context("viewerCanMergeAsAdmin missing")
}

/// Distinct rulesets behind the rules `rules/branches/{base}` lists.
fn parse_branch_ruleset_ids(json: &str) -> Result<Vec<u64>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid branch rules JSON")?;
    let rules = v.as_array().context("branch rules response is not a list")?;
    let mut ids: Vec<u64> = rules
        .iter()
        .filter_map(|r| r["ruleset_id"].as_u64())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// `pull_requests_only` is enough: the bypass happens through a merge.
fn parse_ruleset_bypassable(json: &str) -> Result<bool> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid ruleset JSON")?;
    match v["current_user_can_bypass"].as_str() {
        Some("always" | "pull_requests_only") => Ok(true),
        Some(_) => Ok(false),
        None => anyhow::bail!("ruleset response has no current_user_can_bypass"),
    }
}

/// Cached [`gh_viewer_can_bypass`]. Bypass rights depend on the viewer's role
/// and the base branch's rules, so the answer is cached per base. `None` when
/// GitHub could not answer, and the box then offers no bypass.
pub fn cached_viewer_can_bypass(owner: &str, repo: &str, base: &str, number: u64) -> Option<bool> {
    BYPASS_CACHE.get_or_fetch(branch_key(owner, repo, base), || {
        gh_viewer_can_bypass(owner, repo, base, number).ok()
    })
}

static BYPASS_CACHE: TtlCache<bool> = TtlCache::new(REPO_SETTINGS_TTL);

/// Recreate `branch` at `sha` — GitHub's "Restore branch" after a merge.
pub fn gh_restore_remote_branch(owner: &str, repo: &str, branch: &str, sha: &str) -> Result<()> {
    if sha.trim().is_empty() {
        anyhow::bail!("Cannot restore {branch}: the PR head commit is unknown.");
    }
    let path = format!("repos/{owner}/{repo}/git/refs");
    let ref_field = format!("ref=refs/heads/{branch}");
    let sha_field = format!("sha={sha}");
    gh_ok(
        &[
            "api", "-X", "POST", &path, "-f", &ref_field, "-f", &sha_field,
        ],
        &format!("Failed to restore branch {branch}"),
    )?;
    BRANCH_EXISTS_CACHE.put(branch_key(owner, repo, branch), true);
    Ok(())
}

/// Whether `branch` still exists on `owner/repo`. `Err` when GitHub could not
/// answer, so a network failure is never read as "deleted".
pub fn gh_remote_branch_exists(owner: &str, repo: &str, branch: &str) -> Result<bool> {
    Ok(gh_remote_branch_tip(owner, repo, branch)?.is_some())
}

/// Commit `branch` points at, or `None` when GitHub answers 404.
fn gh_remote_branch_tip(owner: &str, repo: &str, branch: &str) -> Result<Option<String>> {
    // Singular `git/ref` matches exactly; plural `git/refs` would also match
    // every branch that has this one as a prefix.
    let path = format!("repos/{owner}/{repo}/git/ref/heads/{}", encode_ref(branch));
    let output = gh(&["api", &path, "--jq", ".object.sha"])?;
    if output.status.success() {
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok(Some(sha));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_not_found(&stderr) {
        return Ok(None);
    }
    anyhow::bail!("Failed to look up branch {branch}: {}", stderr.trim())
}

/// A merged PR's branch rarely changes, but the status loop asks every 30s.
/// Delete and restore through this module update the entry, so the box
/// reflects its own actions at once (ADR 0026).
const BRANCH_EXISTS_TTL: Duration = Duration::from_secs(5 * 60);
static BRANCH_EXISTS_CACHE: TtlCache<bool> = TtlCache::new(BRANCH_EXISTS_TTL);

/// Owner and repo are case-insensitive on GitHub.
fn repo_key(owner: &str, repo: &str) -> String {
    format!("{owner}/{repo}").to_ascii_lowercase()
}

/// Branch names, unlike owner and repo, are case-sensitive.
fn branch_key(owner: &str, repo: &str, branch: &str) -> String {
    format!("{}/{branch}", repo_key(owner, repo))
}

/// Cached [`gh_remote_branch_exists`]; `None` when GitHub could not answer.
///
/// `deleting_itself`: the repo deletes merged branches, a moment after the
/// merge. A "present" seen then is likely to turn false soon, so it is kept
/// only briefly; GitHub skips that delete for a protected branch or the base
/// of another PR, so it is still kept, rather than asked for every poll.
pub fn cached_remote_branch_exists(
    owner: &str,
    repo: &str,
    branch: &str,
    deleting_itself: bool,
) -> Option<bool> {
    let key = branch_key(owner, repo, branch);
    if let Some(cached) = BRANCH_EXISTS_CACHE.get(&key) {
        return Some(cached);
    }
    let exists = gh_remote_branch_exists(owner, repo, branch).ok()?;
    BRANCH_EXISTS_CACHE.put_for(key, exists, branch_exists_ttl(exists, deleting_itself));
    Some(exists)
}

const PENDING_DELETE_TTL: Duration = Duration::from_secs(60);

fn branch_exists_ttl(exists: bool, deleting_itself: bool) -> Duration {
    if exists && deleting_itself {
        PENDING_DELETE_TTL
    } else {
        BRANCH_EXISTS_TTL
    }
}

fn is_not_found(stderr: &str) -> bool {
    stderr.contains("HTTP 404") || stderr.contains("Not Found")
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
pub fn parse_repo_merge_settings(json: &str) -> Result<RepoMergeSettings> {
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

pub fn gh_repo_merge_settings(owner: &str, repo: &str) -> Result<RepoMergeSettings> {
    let out = graphql(
        REPO_SETTINGS_QUERY,
        &[("owner", owner), ("name", repo)],
        &[],
        "repo settings query failed",
    )?;
    parse_repo_merge_settings(&out)
}

/// Repo settings rarely change, and the status loop runs every 30s; one
/// query per repo per TTL keeps it off the rate limit (ADR 0026).
const REPO_SETTINGS_TTL: Duration = Duration::from_secs(15 * 60);
static REPO_SETTINGS_CACHE: TtlCache<RepoMergeSettings> = TtlCache::new(REPO_SETTINGS_TTL);

/// Cached [`gh_repo_merge_settings`].
pub fn cached_repo_merge_settings(owner: &str, repo: &str) -> Option<RepoMergeSettings> {
    REPO_SETTINGS_CACHE.get_or_fetch(repo_key(owner, repo), || {
        gh_repo_merge_settings(owner, repo).ok()
    })
}

/// Per-key cache of `gh` answers. A failed lookup (`None`) is not stored, so
/// the next status refresh asks again.
struct TtlCache<V> {
    ttl: Duration,
    /// Each entry carries its own expiry, so one entry can live shorter.
    entries: OnceLock<Mutex<HashMap<String, (Instant, V)>>>,
}

impl<V: Clone> TtlCache<V> {
    const fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: OnceLock::new(),
        }
    }

    fn map(&self) -> &Mutex<HashMap<String, (Instant, V)>> {
        self.entries.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn get(&self, key: &str) -> Option<V> {
        let g = self.map().lock().ok()?;
        let (expires, v) = g.get(key)?;
        (Instant::now() < *expires).then(|| v.clone())
    }

    fn get_or_fetch(&self, key: String, fetch: impl FnOnce() -> Option<V>) -> Option<V> {
        if let Some(v) = self.get(&key) {
            return Some(v);
        }
        let v = fetch()?;
        self.put(key, v.clone());
        Some(v)
    }

    fn put(&self, key: String, v: V) {
        self.put_for(key, v, self.ttl);
    }

    fn put_for(&self, key: String, v: V, ttl: Duration) {
        if let Ok(mut g) = self.map().lock() {
            g.insert(key, (Instant::now() + ttl, v));
        }
    }
}

fn gh<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Result<Output> {
    Command::new("gh")
        .args(args)
        .output_timed(crate::proc::GH_TIMEOUT)
        .map_err(crate::github::gh_spawn_context)
}

/// Run `gh`, returning stdout, or failing with `what` and gh's stderr.
fn gh_ok<S: AsRef<std::ffi::OsStr>>(args: &[S], what: &str) -> Result<String> {
    let output = gh(args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{what}: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `gh api graphql` with string variables (`-f`, so a numeric repo name stays
/// a string) and typed ones (`-F`, for `Int!`).
fn graphql(
    query: &str,
    strings: &[(&str, &str)],
    typed: &[(&str, &str)],
    what: &str,
) -> Result<String> {
    gh_ok(&graphql_args(query, strings, typed), what)
}

fn graphql_args(query: &str, strings: &[(&str, &str)], typed: &[(&str, &str)]) -> Vec<String> {
    let mut args = vec![
        "api".to_string(),
        "graphql".to_string(),
        "-f".to_string(),
        format!("query={query}"),
    ];
    for (k, v) in strings {
        args.extend(["-f".to_string(), format!("{k}={v}")]);
    }
    for (k, v) in typed {
        args.extend(["-F".to_string(), format!("{k}={v}")]);
    }
    args
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
        // `--delete-branch` without a working-tree-free guarantee can switch the
        // user's checkout; deletion is a separate API call.
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
    fn admin_merge_flag_is_read_from_the_pull_request() {
        let yes = r#"{"data":{"repository":{"pullRequest":{"viewerCanMergeAsAdmin":true}}}}"#;
        let no = r#"{"data":{"repository":{"pullRequest":{"viewerCanMergeAsAdmin":false}}}}"#;
        assert!(parse_can_merge_as_admin(yes).unwrap());
        assert!(!parse_can_merge_as_admin(no).unwrap());
        assert!(parse_can_merge_as_admin(r#"{"data":{"repository":null}}"#).is_err());
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
        // Unknown means no answer, so the box offers no bypass.
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
    fn not_found_detection() {
        assert!(is_not_found("gh: Not Found (HTTP 404)"));
        assert!(!is_not_found("error connecting to api.github.com"));
    }

    #[test]
    fn update_branch_on_old_gh_says_to_upgrade() {
        let msg = action_error(
            &PrAction::UpdateBranch { rebase: false },
            "unknown command \"update-branch\"",
        );
        assert!(msg.contains("Upgrade the GitHub CLI"));
        assert_eq!(
            action_error(&PrAction::Close, ""),
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
        // GitHub's default is off; guessing on would offer a refused button.
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
        let a = graphql_args("Q", &[("name", "123")], &[("number", "7")]);
        assert_eq!(
            a,
            ["api", "graphql", "-f", "query=Q", "-f", "name=123", "-F", "number=7"]
        );
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
        // Right after a merge on a delete-on-merge repo, "present" is about to
        // turn false; kept for the full TTL it would show Delete for minutes,
        // and not kept at all it would cost a `gh` call every poll.
        assert_eq!(branch_exists_ttl(true, true), PENDING_DELETE_TTL);
        assert_eq!(branch_exists_ttl(true, false), BRANCH_EXISTS_TTL);
        assert_eq!(branch_exists_ttl(false, true), BRANCH_EXISTS_TTL);
    }

    #[test]
    fn ttl_cache_entry_can_expire_sooner_than_the_default() {
        let cache: TtlCache<u32> = TtlCache::new(Duration::from_secs(60));
        cache.put_for("short".into(), 1, Duration::ZERO);
        cache.put("long".into(), 2);
        assert_eq!(cache.get("short"), None);
        assert_eq!(cache.get("long"), Some(2));
    }

    #[test]
    fn a_branch_that_moved_past_the_pr_is_kept() {
        // A long-lived branch with commits after the PR closed holds work no
        // PR has; deleting it would lose that work.
        let err = check_tip_matches("dev", "newcommit123", "prhead").unwrap_err();
        assert!(err.to_string().contains("It was kept"), "{err}");
        assert!(err.to_string().contains("newcomm"), "{err}");
        assert!(check_tip_matches("feat/x", "prhead", "prhead").is_ok());
        assert!(check_tip_matches("feat/x", "prhead", "").is_err());
    }

    #[test]
    fn already_deleted_branch_counts_as_deleted() {
        assert!(is_already_gone("gh: Reference does not exist (HTTP 422)"));
        // A 404 can mean "no access"; a failed delete must not read as done.
        assert!(!is_already_gone("gh: Not Found (HTTP 404)"));
        assert!(!is_already_gone(
            "gh: Resource not accessible by integration (HTTP 403)"
        ));
    }

    #[test]
    fn branch_key_ignores_repo_case_but_not_branch_case() {
        assert_eq!(
            branch_key("Org", "Repo", "Feat/X"),
            branch_key("org", "repo", "Feat/X")
        );
        assert_ne!(branch_key("o", "r", "Feat"), branch_key("o", "r", "feat"));
    }

    #[test]
    fn ttl_cache_serves_fresh_entries_and_skips_failures() {
        let cache: TtlCache<u32> = TtlCache::new(Duration::from_secs(60));
        let mut calls = 0;
        assert_eq!(
            cache.get_or_fetch("k".into(), || {
                calls += 1;
                None
            }),
            None
        );
        assert_eq!(
            cache.get_or_fetch("k".into(), || {
                calls += 1;
                Some(7)
            }),
            Some(7)
        );
        assert_eq!(
            cache.get_or_fetch("k".into(), || {
                calls += 1;
                Some(8)
            }),
            Some(7)
        );
        assert_eq!(calls, 2, "a failed lookup is retried, a fresh hit is not");
        cache.put("k".into(), 9);
        assert_eq!(cache.get_or_fetch("k".into(), || Some(10)), Some(9));
    }

    #[test]
    fn ttl_cache_refetches_after_expiry() {
        let cache: TtlCache<u32> = TtlCache::new(Duration::ZERO);
        cache.put("k".into(), 1);
        assert_eq!(cache.get_or_fetch("k".into(), || Some(2)), Some(2));
    }
}
