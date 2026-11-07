//! Managed review-artifact storage shared by TUI and Desktop.
//!
//! Flat branch-level layout under
//! `<app_data>/easy-review/repos/<repo_slug>/branches/<branch_slug>/`.

use std::path::PathBuf;

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

/// The key a repo's `[risk.<repo>]` / `[file_kinds]` tables live under.
///
/// Pass `remote_repo` (`owner/repo`) only for a target with no clone: its
/// `repo_root` is whatever directory the process runs in, so the key comes from
/// the repo name, which is what [`slug_repo`] returns for a clone of it. With
/// `None` the clone is asked, so every view of one clone shares a key.
pub fn rules_key(git: &dyn er_git::Git, repo_root: &str, remote_repo: Option<&str>) -> String {
    match remote_repo.map(str::trim).filter(|r| !r.is_empty()) {
        Some(remote) => {
            let name = remote.rsplit('/').next().unwrap_or(remote);
            slugify(name.trim_end_matches(".git"))
        }
        None => slug_repo(git, repo_root),
    }
}

/// Derive a stable repo slug. Prefer the basename of the git remote origin URL;
/// fall back to the basename of `repo_root`.
pub fn slug_repo(git: &dyn er_git::Git, repo_root: &str) -> String {
    if let Ok(url) = git.remote_url(repo_root, "origin") {
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
///
/// A failed `create_dir_all` still returns the computed managed path; later
/// writes fail visibly where they handle errors.
pub fn resolve_managed_root_from_slugs(repo_slug: &str, branch_slug: &str) -> ErRoot {
    let branch_path = branch_dir(repo_slug, branch_slug);
    let _ = std::fs::create_dir_all(&branch_path);
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
/// This follows the same storage rules as a local `er` tab: the `branch` view
/// bucket under managed storage.
pub fn local_branch_bucket_dir(git: &dyn er_git::Git, repo_root: &str, branch: &str) -> PathBuf {
    view_bucket_dir(&slug_repo(git, repo_root), &slug_branch(branch), "branch")
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
/// Creates the directory if it does not exist. A failed `create_dir_all` still
/// returns the computed managed path; later writes fail visibly where they
/// handle errors.
pub fn resolve_managed_root_for_view_bucket(
    repo_slug: &str,
    branch_slug: &str,
    bucket: &str,
) -> ErRoot {
    let dir = view_bucket_dir(repo_slug, branch_slug, bucket);
    let _ = std::fs::create_dir_all(&dir);
    let path_str = dir.to_string_lossy().into_owned();
    ErRoot::Managed {
        agent_dir: path_str.clone(),
        session_dir: path_str,
    }
}

/// Resolve managed storage for a PR bucket.
///
/// Creates the directory if it does not exist. A failed `create_dir_all` still
/// returns the computed managed path; later writes fail visibly where they
/// handle errors.
pub fn resolve_managed_root_for_pr_bucket(owner_repo_slug: &str, pr_number: u64) -> ErRoot {
    let dir = pr_bucket_dir(owner_repo_slug, pr_number);
    let _ = std::fs::create_dir_all(&dir);
    let path_str = dir.to_string_lossy().into_owned();
    ErRoot::Managed {
        agent_dir: path_str.clone(),
        session_dir: path_str,
    }
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
        } = root;
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
        let dir = local_branch_bucket_dir(
            &*crate::ports::Ports::for_test().git,
            "/tmp/example-repo",
            "feature/foo",
        );
        let s = dir.to_string_lossy();
        assert!(s.contains("repos/example-repo/branches/feature-foo/view-buckets/branch"));
    }

    #[test]
    fn slug_repo_uses_the_remote_url_from_the_handle() {
        let fake = er_git::FakeGit::default()
            .with_remote_url(Ok("git@github.com:Acme/My-Repo.git".into()));
        assert_eq!(slug_repo(&fake, "/anywhere"), "My-Repo");
    }

    #[test]
    fn slug_repo_falls_back_to_the_directory_name_when_the_remote_is_missing() {
        let fake = er_git::FakeGit::default().with_remote_url(Err(er_git::GitError::Failed {
            context: "no origin".into(),
            stderr: String::new(),
        }));
        assert_eq!(slug_repo(&fake, "/tmp/Some-Dir"), "Some-Dir");
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
        } = root;
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
        } = root;
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

    #[test]
    fn repo_local_env_var_is_ignored() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        std::env::set_var("ER_REPO_LOCAL", "1");
        let dir = local_branch_bucket_dir(
            &*crate::ports::Ports::for_test().git,
            "/some/repo",
            "feature",
        );
        std::env::remove_var("ER_REPO_LOCAL");
        std::env::remove_var("ER_STORAGE_ROOT");

        let s = dir.to_string_lossy();
        assert!(
            s.starts_with(&*tmp.path().to_string_lossy()),
            "not under the storage root: {s}"
        );
        assert!(s.ends_with("view-buckets/branch"), "unexpected path: {s}");
    }

    #[test]
    fn a_failed_mkdir_still_names_the_managed_path() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        std::env::set_var("ER_STORAGE_ROOT", blocker.join("root"));
        let root = resolve_managed_root_for_view_bucket("r", "b", "branch");
        let expected = view_bucket_dir("r", "b", "branch");
        std::env::remove_var("ER_STORAGE_ROOT");

        assert_eq!(root.er_dir(), expected.to_string_lossy());
    }
}
