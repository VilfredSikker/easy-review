//! Managed review-artifact storage shared by TUI and Desktop.
//!
//! Flat branch-level layout under
//! `<app_data>/easy-review/repos/<repo_slug>/branches/<branch_slug>/`.

use std::path::{Path, PathBuf};

use crate::ErRoot;

/// Root of all managed review storage.
///
/// Overridden by `ER_STORAGE_ROOT` when set — used by tests to write under a
/// temporary directory without touching the real user data dir.
pub fn storage_root() -> PathBuf {
    if let Ok(override_path) = std::env::var("ER_STORAGE_ROOT") {
        if !override_path.is_empty() {
            return PathBuf::from(override_path);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local")
                .join("share")
        })
        .join("easy-review")
}

/// Sanitize a string for use as a directory name component.
pub fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Derive a stable repo slug. Prefer the basename of the git remote origin URL;
/// fall back to the basename of `repo_root`.
pub fn slug_repo(repo_root: &str) -> String {
    if let Ok(out) = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
    {
        if out.status.success() {
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let url = url.trim_end_matches(".git").to_string();
            let name = url
                .rsplit('/')
                .next()
                .or_else(|| url.rsplit(':').next())
                .unwrap_or(&url)
                .to_string();
            if !name.is_empty() {
                return slugify(&name);
            }
        }
    }
    std::path::Path::new(repo_root)
        .file_name()
        .and_then(|s| s.to_str())
        .map(slugify)
        .unwrap_or_else(|| "repo".to_string())
}

/// Sanitize a branch name for use as a directory component.
pub fn slug_branch(branch: &str) -> String {
    slugify(&branch.replace('/', "-"))
}

/// Label a PR tab carries while its head branch is still unknown (`pr/<N>`).
///
/// It stands in for the branch in the tab title and the branch-bucket slug,
/// but it is not a branch: sidecar scope checks must treat it as "no branch"
/// (see [`is_pr_placeholder_branch`]).
pub fn pr_placeholder_branch(pr_number: u64) -> String {
    format!("pr/{pr_number}")
}

/// True when `name` is a [`pr_placeholder_branch`] label rather than a real branch.
pub fn is_pr_placeholder_branch(name: &str) -> bool {
    name.strip_prefix("pr/")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Directory for a specific branch under the managed storage root.
pub fn branch_dir(repo_slug: &str, branch_slug: &str) -> PathBuf {
    storage_root()
        .join("repos")
        .join(repo_slug)
        .join("branches")
        .join(branch_slug)
}

/// Resolve managed storage from already-slugged components.
pub fn resolve_managed_root_from_slugs(repo_slug: &str, branch_slug: &str) -> ErRoot {
    let branch_path = branch_dir(repo_slug, branch_slug);
    if std::fs::create_dir_all(&branch_path).is_err() {
        return ErRoot::RepoLocal(String::new());
    }
    let path_str = branch_path.to_string_lossy().into_owned();
    ErRoot::Managed {
        agent_dir: path_str.clone(),
        session_dir: path_str,
    }
}

/// Directory for a specific view bucket under the managed storage root.
///
/// Layout: `<storage_root>/repos/<repo_slug>/branches/<branch_slug>/view-buckets/<bucket>`
/// Bucket is one of `"branch"`, `"unstaged"`, `"staged"`, or `"history"`.
pub fn view_bucket_dir(repo_slug: &str, branch_slug: &str, bucket: &str) -> PathBuf {
    storage_root()
        .join("repos")
        .join(repo_slug)
        .join("branches")
        .join(branch_slug)
        .join("view-buckets")
        .join(bucket)
}

/// Directory for the checked-out branch's local review bucket.
///
/// This follows the same storage rules as a local `er` tab: repo-local mode
/// uses `<repo_root>/.er`, while managed storage uses the `branch` view bucket.
pub fn local_branch_bucket_dir(repo_root: &str, branch: &str) -> PathBuf {
    if use_repo_local_storage() {
        return Path::new(repo_root).join(".er");
    }

    view_bucket_dir(&slug_repo(repo_root), &slug_branch(branch), "branch")
}

/// Directory for a PR bucket under the managed storage root.
///
/// Layout: `<storage_root>/repos/<owner_repo_slug>/prs/pr-<N>`
/// `owner_repo_slug` is the slugified `owner-repo` string (e.g. `"myorg-myrepo"`).
pub fn pr_bucket_dir(owner_repo_slug: &str, pr_number: u64) -> PathBuf {
    storage_root()
        .join("repos")
        .join(owner_repo_slug)
        .join("prs")
        .join(format!("pr-{pr_number}"))
}

/// Resolve managed storage for a local view bucket (branch/unstaged/staged/history).
///
/// Creates the directory if it does not exist. Falls back to `ErRoot::RepoLocal("")`
/// on failure (mirrors `resolve_managed_root_from_slugs`).
pub fn resolve_managed_root_for_view_bucket(
    repo_slug: &str,
    branch_slug: &str,
    bucket: &str,
) -> ErRoot {
    let dir = view_bucket_dir(repo_slug, branch_slug, bucket);
    if std::fs::create_dir_all(&dir).is_err() {
        return ErRoot::RepoLocal(String::new());
    }
    let path_str = dir.to_string_lossy().into_owned();
    ErRoot::Managed {
        agent_dir: path_str.clone(),
        session_dir: path_str,
    }
}

/// Resolve managed storage for a PR bucket.
///
/// Creates the directory if it does not exist. Falls back to `ErRoot::RepoLocal("")`
/// on failure.
pub fn resolve_managed_root_for_pr_bucket(owner_repo_slug: &str, pr_number: u64) -> ErRoot {
    let dir = pr_bucket_dir(owner_repo_slug, pr_number);
    if std::fs::create_dir_all(&dir).is_err() {
        return ErRoot::RepoLocal(String::new());
    }
    let path_str = dir.to_string_lossy().into_owned();
    ErRoot::Managed {
        agent_dir: path_str.clone(),
        session_dir: path_str,
    }
}

/// True when `ER_REPO_LOCAL=1` — use repo `.er/` instead of managed storage.
pub fn use_repo_local_storage() -> bool {
    std::env::var("ER_REPO_LOCAL").as_deref() == Ok("1")
}

/// Shared mutex for tests that mutate `ER_STORAGE_ROOT`.
///
/// All tests setting `ER_STORAGE_ROOT` must hold this lock for the duration of the
/// test.  Use `.lock().unwrap_or_else(|e| e.into_inner())` so a panicking test does
/// not poison the mutex and cascade into sibling tests.
#[cfg(test)]
pub static STORAGE_TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn pr_placeholder_branch_round_trips_and_rejects_real_branches() {
        assert_eq!(pr_placeholder_branch(1560), "pr/1560");
        assert!(is_pr_placeholder_branch("pr/1560"));
        assert!(!is_pr_placeholder_branch("pr/"));
        assert!(!is_pr_placeholder_branch("pr/1560-fix"));
        assert!(!is_pr_placeholder_branch("fix/superviewer-manager"));
        assert!(!is_pr_placeholder_branch("unknown"));
        assert!(!is_pr_placeholder_branch(""));
    }

    #[test]
    fn slug_branch_replaces_slashes() {
        assert_eq!(slug_branch("feature/foo"), "feature-foo");
    }

    #[test]
    fn resolve_managed_root_creates_branch_dir() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        let root = resolve_managed_root_from_slugs("test-repo", "feature-branch");
        std::env::remove_var("ER_STORAGE_ROOT");
        let crate::ErRoot::Managed {
            agent_dir,
            session_dir,
        } = root
        else {
            panic!("expected Managed root");
        };
        assert_eq!(agent_dir, session_dir);
        assert!(agent_dir.contains("test-repo"));
        assert!(agent_dir.contains("feature-branch"));
    }

    #[test]
    fn view_bucket_dir_contains_expected_components() {
        let dir = view_bucket_dir("my-repo", "feature-branch", "unstaged");
        let s = dir.to_string_lossy();
        assert!(s.contains("my-repo"), "missing repo slug: {s}");
        assert!(s.contains("feature-branch"), "missing branch slug: {s}");
        assert!(
            s.contains("view-buckets/unstaged"),
            "missing bucket path: {s}"
        );
    }

    #[test]
    fn local_branch_bucket_dir_uses_branch_view_bucket() {
        let dir = local_branch_bucket_dir("/tmp/example-repo", "feature/foo");
        let s = dir.to_string_lossy();
        assert!(s.contains("repos/example-repo/branches/feature-foo/view-buckets/branch"));
    }

    #[test]
    fn pr_bucket_dir_contains_expected_components() {
        let dir = pr_bucket_dir("myorg-myrepo", 42);
        let s = dir.to_string_lossy();
        assert!(s.contains("myorg-myrepo"), "missing owner-repo slug: {s}");
        assert!(s.contains("prs/pr-42"), "missing pr path: {s}");
    }

    #[test]
    fn resolve_managed_root_for_view_bucket_creates_dir() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        let root = resolve_managed_root_for_view_bucket("test-repo", "main", "branch");
        std::env::remove_var("ER_STORAGE_ROOT");
        let crate::ErRoot::Managed {
            agent_dir,
            session_dir,
        } = root
        else {
            panic!("expected Managed root");
        };
        assert_eq!(agent_dir, session_dir);
        assert!(
            agent_dir.contains("view-buckets/branch"),
            "unexpected path: {agent_dir}"
        );
        assert!(
            agent_dir.contains("test-repo"),
            "missing repo slug: {agent_dir}"
        );
    }

    #[test]
    fn resolve_managed_root_for_pr_bucket_creates_dir() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        let root = resolve_managed_root_for_pr_bucket("myorg-myrepo", 7);
        std::env::remove_var("ER_STORAGE_ROOT");
        let crate::ErRoot::Managed {
            agent_dir,
            session_dir,
        } = root
        else {
            panic!("expected Managed root");
        };
        assert_eq!(agent_dir, session_dir);
        assert!(
            agent_dir.contains("prs/pr-7"),
            "unexpected path: {agent_dir}"
        );
        assert!(
            agent_dir.contains("myorg-myrepo"),
            "missing slug: {agent_dir}"
        );
    }
}
