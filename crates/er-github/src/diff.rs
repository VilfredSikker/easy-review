//! PR diffs: the remote endpoint, the local and shallow-clone fallbacks for
//! oversized PRs, and bounded file reads at a ref.

use anyhow::Result;
use er_git::{Git, GitError};

use crate::repo::parse_origin_remote;
use crate::{default_handle, CliGitHub, GhError};

/// Remote-only PRs above these limits are rejected up front with a clear message.
/// The IPC line budget (SNAPSHOT_DIFF_LINE_BUDGET) enforces what reaches the UI;
/// these gates only guard against pathological network / memory usage.
const REMOTE_PR_MAX_CHANGED_FILES: u64 = 10_000;
const REMOTE_PR_MAX_LINE_CHANGES: u64 = 5_000_000;

impl CliGitHub {
    /// Quick size check via `gh pr view` so we do not clone multi-GB repos for
    /// hopeless PRs.
    pub(crate) fn pr_size_check_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(), GhError> {
        let repo_slug = format!("{owner}/{repo}");
        let n = number.to_string();
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &n,
                "--repo",
                &repo_slug,
                "--json",
                "additions,deletions,changedFiles",
                "--jq",
                r#"{additions: .additions, deletions: .deletions, files: .changedFiles}"#,
            ],
            None,
        )?;
        let v: serde_json::Value =
            serde_json::from_slice(&out.body).map_err(|error| GhError::Local {
                message: format!("PR size JSON: {error}"),
            })?;
        let additions = v["additions"].as_u64().unwrap_or(0);
        let deletions = v["deletions"].as_u64().unwrap_or(0);
        let files = v["files"].as_u64().unwrap_or(0);
        let lines = additions.saturating_add(deletions);

        if files > REMOTE_PR_MAX_CHANGED_FILES || lines > REMOTE_PR_MAX_LINE_CHANGES {
            return Err(GhError::Local {
                message: format!(
                    "PR #{number} is too large to review here ({files} files, +{additions}/-{deletions} lines). \
                     Open a smaller PR, or clone {owner}/{repo} locally."
                ),
            });
        }
        Ok(())
    }
}

/// Whether a failed `gh pr diff` means GitHub refused to render the diff.
fn diff_is_too_large(error: &GhError) -> bool {
    match error {
        GhError::TooLarge { .. } => true,
        GhError::Tool { stderr, .. } => gh_diff_too_large(stderr),
        _ => false,
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_size_check_remote(owner: &str, repo: &str, number: u64) -> Result<()> {
    default_handle()
        .pr_size_check_remote(owner, repo, number)
        .map_err(anyhow::Error::from)
}

/// Get raw unified diff for a PR via `gh pr diff N --repo owner/repo`.
///
/// Works without a local clone — uses GitHub API via gh CLI.
/// Falls back to shallow clone + local git diff when the PR exceeds GitHub's
/// API line limit (HTTP 406 / diff_too_large).
impl CliGitHub {
    pub(crate) fn pr_diff_remote(
        &self,
        git: &dyn Git,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<String, GhError> {
        self.pr_size_check_remote(owner, repo, number)?;
        let repo_slug = format!("{owner}/{repo}");
        let n = number.to_string();
        match self.run_gh_args(&["pr", "diff", &n, "--repo", &repo_slug], None) {
            Ok(out) => Ok(String::from_utf8_lossy(&out.body).to_string()),
            Err(error) if diff_is_too_large(&error) => {
                self.pr_diff_via_clone(git, owner, repo, number)
            }
            Err(error) => Err(error),
        }
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_diff_remote(git: &dyn Git, owner: &str, repo: &str, number: u64) -> Result<String> {
    default_handle()
        .pr_diff_remote(git, owner, repo, number)
        .map_err(anyhow::Error::from)
}

/// Get raw unified diff for a PR using the local repo context.
///
/// This intentionally delegates PR-source rendering to GitHub instead of
/// reconstructing the range with local refs. GitHub PR diffs are not just
/// "whatever local `origin/<base>...refs/pull/N/head` happens to produce" in
/// every edge case, and using `gh pr diff` keeps Easy Review aligned with the
/// Files changed view.
impl CliGitHub {
    pub(crate) fn pr_diff(
        &self,
        git: &dyn Git,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<String, GhError> {
        let n = pr_number.to_string();
        match self.run_gh_args(&["pr", "diff", &n], Some(repo_root)) {
            Ok(out) => Ok(String::from_utf8_lossy(&out.body).to_string()),
            Err(error) if diff_is_too_large(&error) => {
                self.pr_diff_too_large_fallback(git, pr_number, repo_root)
            }
            Err(error) => Err(error),
        }
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_diff(git: &dyn Git, pr_number: u64, repo_root: &str) -> Result<String> {
    default_handle()
        .pr_diff(git, pr_number, repo_root)
        .map_err(anyhow::Error::from)
}

/// Whether `gh pr diff` stderr says GitHub refused to render the diff.
///
/// GitHub caps the diff endpoint by file count (300) and by size, and the cap
/// has nothing to do with how many lines changed: a few thousand lines spread
/// over more than 300 files is refused, as is a small change to one huge file.
pub(crate) fn gh_diff_too_large(stderr: &str) -> bool {
    stderr.contains("HTTP 406")
        || stderr.contains("too_large")
        || stderr.contains("exceeded the maximum number")
}

/// A PR the diff endpoint refused: build the diff from the local checkout, and
/// only if that cannot work (no usable `origin`, a shallow clone with no merge
/// base) clone the repo into a temp dir.
impl CliGitHub {
    fn pr_diff_too_large_fallback(
        &self,
        git: &dyn Git,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<String, GhError> {
        let local = self.pr_diff_via_local_git(git, pr_number, repo_root);
        let Err(local_err) = local else {
            return local;
        };
        let Some((owner, repo)) = parse_origin_remote(git, repo_root) else {
            return Err(GhError::Local {
                message: format!("{local_err}: GitHub refused to render this large PR diff"),
            });
        };
        match self.pr_diff_via_clone(git, &owner, &repo, pr_number) {
            Ok(diff) => Ok(diff),
            Err(clone_err) => Err(GhError::Local {
                message: format!(
                    "GitHub refused to render this large PR diff; local git diff also failed: {local_err:#}: {clone_err}"
                ),
            }),
        }
    }

    fn pr_diff_via_local_git(
        &self,
        git: &dyn Git,
        pr_number: u64,
        repo_root: &str,
    ) -> Result<String, GhError> {
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &pr_number.to_string(),
                "--json",
                "baseRefOid,headRefOid",
                "--jq",
                r#"[.baseRefOid, .headRefOid] | @tsv"#,
            ],
            Some(repo_root),
        )?;
        let text = String::from_utf8_lossy(&out.body).trim().to_string();
        let (base_sha, head_sha) = text
            .split_once('\t')
            .filter(|(base, head)| !base.is_empty() && !head.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Unexpected gh pr view output: {text}"))?;

        // By SHA rather than by branch name: it works for forks and for a head
        // branch that has since been deleted. No refs are created.
        match git.fetch(repo_root, &["--no-tags", "origin", base_sha, head_sha]) {
            Ok(()) => {}
            Err(GitError::Failed { stderr, .. }) => {
                return Err(GhError::Local {
                    message: format!("Failed to fetch PR commits: {}", stderr.trim()),
                });
            }
            Err(e) => {
                return Err(GhError::from(
                    anyhow::Error::from(e).context("Failed to fetch PR commits"),
                ));
            }
        }
        git_merge_base_diff(git, repo_root, base_sha, head_sha).map_err(GhError::from)
    }
}

/// `git diff base...head`: against the merge base, which is what GitHub's
/// Files changed view shows. A two-dot diff against the base tip would also
/// show the base branch's own newer commits, reversed.
pub(crate) fn git_merge_base_diff(
    git: &dyn Git,
    repo_root: &str,
    base: &str,
    head: &str,
) -> Result<String> {
    let range = format!("{base}...{head}");
    match git.diff_revs(repo_root, &[&range]) {
        Ok(diff) => Ok(diff),
        Err(GitError::Failed { stderr, .. }) => {
            anyhow::bail!("git diff failed for large PR: {}", stderr.trim());
        }
        Err(e) => Err(anyhow::Error::from(e).context("Failed to run git diff for large PR")),
    }
}

/// Base and head commit SHAs for a PR (from `gh pr view`). Works for merged PRs and
/// deleted head branches — unlike branch names, which may no longer exist on origin.
impl CliGitHub {
    /// Base and head commit SHAs for a PR (from `gh pr view`). Works for merged
    /// PRs and deleted head branches.
    pub(crate) fn commit_shas_remote(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<(String, String), GhError> {
        let repo_slug = format!("{owner}/{repo}");
        let n = number.to_string();
        let out = self.run_gh_args(
            &[
                "pr",
                "view",
                &n,
                "--repo",
                &repo_slug,
                "--json",
                "baseRefOid,headRefOid",
                "--jq",
                r#"[.baseRefOid, .headRefOid] | @tsv"#,
            ],
            None,
        )?;
        let text = String::from_utf8_lossy(&out.body).trim().to_string();
        let (base_sha, head_sha) = text
            .split_once('\t')
            .ok_or_else(|| anyhow::anyhow!("Unexpected gh pr view output: {text}"))?;
        if base_sha.is_empty() || head_sha.is_empty() {
            return Err(GhError::Local {
                message: format!("Missing base or head SHA for PR #{number}"),
            });
        }
        Ok((base_sha.to_string(), head_sha.to_string()))
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_pr_commit_shas_remote(owner: &str, repo: &str, number: u64) -> Result<(String, String)> {
    default_handle()
        .commit_shas_remote(owner, repo, number)
        .map_err(anyhow::Error::from)
}

impl CliGitHub {
    /// Fallback for large PRs: shallow-clone the repo into a temp dir, fetch
    /// both commits by SHA, and produce the diff locally. The temp dir is
    /// cleaned up automatically.
    fn pr_diff_via_clone(
        &self,
        git: &dyn Git,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<String, GhError> {
        let (base_sha, head_sha) = self.commit_shas_remote(owner, repo, number)?;
        let repo_url = format!("https://github.com/{owner}/{repo}.git");
        // tempfile::Builder::tempdir: random name + 0700, so an attacker cannot
        // pre-create the clone dir (the old fixed `er-remote-{owner}-{repo}-{n}`
        // name with remove_dir_all + clone let an attacker win the dir, then swap
        // .git/config — core.sshCommand + ssh origin → code execution on the
        // fetch below; or poison the diff). 0700 also blocks reading the clone.
        let tmp_dir = tempfile::Builder::new()
            .prefix("er-remote-")
            .tempdir()
            .map_err(|error| GhError::Local {
                message: format!("Failed to create temp dir for large PR diff: {error}"),
            })?;
        let tmp_path = tmp_dir
            .path()
            .to_str()
            .ok_or_else(|| GhError::Local {
                message: "Temp dir path is not valid UTF-8".to_string(),
            })?
            .to_string();

        // Shallow clone with only the two refs we need
        match git.shallow_clone(&repo_url, &tmp_path) {
            Ok(()) => {}
            Err(GitError::Failed { stderr, .. }) => {
                return Err(GhError::Local {
                    message: format!("Shallow clone failed: {}", stderr.trim()),
                });
            }
            Err(e) => {
                return Err(GhError::from(
                    anyhow::Error::from(e).context("Failed to shallow clone for large PR diff"),
                ));
            }
        }

        // Fetch exact base/head commits (branch names fail for merged/deleted heads and some forks).
        match git.fetch(&tmp_path, &["--depth=1", "origin", &base_sha, &head_sha]) {
            Ok(()) => {}
            Err(GitError::Failed { stderr, .. }) => {
                return Err(GhError::Local {
                    message: format!("Failed to fetch PR commits: {}", stderr.trim()),
                });
            }
            Err(e) => {
                return Err(GhError::from(
                    anyhow::Error::from(e).context("Failed to fetch PR commits"),
                ));
            }
        }

        // Generate the diff
        let diff = match git.diff_revs(&tmp_path, &[&base_sha, &head_sha]) {
            Ok(diff) => diff,
            Err(e) => {
                // Clean up (TempDir removes the clone on drop — including on error).
                drop(tmp_dir);
                return Err(match e {
                    GitError::Failed { stderr, .. } => GhError::Local {
                        message: format!("git diff failed in cloned repo: {}", stderr.trim()),
                    },
                    other => GhError::from(
                        anyhow::Error::from(other)
                            .context("Failed to generate diff from cloned repo"),
                    ),
                });
            }
        };

        // Clean up (TempDir removes the clone on drop — including on error).
        drop(tmp_dir);

        Ok(diff)
    }
}

impl CliGitHub {
    /// One bounded `gh api` read whose body is returned without headers.
    pub(crate) fn api_get_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, GhError> {
        let args = vec![
            "api".to_string(),
            path.to_string(),
            "-H".to_string(),
            "Accept: application/vnd.github.raw+json".to_string(),
        ];
        let output = self.run_gh(&args, None, Some(max_bytes))?;
        Ok(output.body)
    }

    /// Raw contents of `path` at `git_ref` on GitHub, capped at `max_bytes`.
    pub(crate) fn file_at_ref(
        &self,
        repo_slug: &str,
        path: &str,
        git_ref: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, GhError> {
        fn encode(s: &str) -> String {
            s.bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.') {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect()
        }
        self.api_get_bounded(
            &format!(
                "repos/{repo_slug}/contents/{}?ref={}",
                encode(path),
                encode(git_ref)
            ),
            max_bytes,
        )
    }
}

/// Compatibility wrapper for front ends and engine callers.
pub fn gh_file_at_ref(
    repo_slug: &str,
    path: &str,
    git_ref: &str,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    default_handle()
        .file_at_ref(repo_slug, path, git_ref, max_bytes)
        .map_err(anyhow::Error::from)
}
