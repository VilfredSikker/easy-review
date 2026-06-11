//! Managed review-artifact storage shared by TUI and Desktop.
//!
//! Flat branch-level layout under
//! `<app_data>/easy-review/repos/<repo_slug>/branches/<branch_slug>/`.

use std::path::{Path, PathBuf};

use crate::ErRoot;

const MARKER_FILES: &[&str] = &[
    "review.json",
    "order.json",
    "questions.json",
    "github-comments.json",
    "checklist.json",
    "summary.md",
    "reviewed",
    "session.json",
];

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

/// Directory-name slug for a remote `"owner/repo"` string.
///
/// Shared by the nearest-PR cache (`pr_cache::cache_path`) and the persistent
/// diff store (`diff_store`) so the two can never drift apart on disk layout
/// (`<storage_root>/repos/<slug>/…`).
pub fn remote_repo_dir_slug(remote: &str) -> String {
    slugify(&remote.replace('/', "-"))
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

/// Whether the managed directory has any review artifacts yet.
pub fn managed_dir_has_artifacts(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    for name in MARKER_FILES {
        if dir.join(name).exists() {
            return true;
        }
    }
    dir.join("experts").is_dir()
}

/// Legacy `~/.cache/er/...` path for a tab (pre-unification).
pub fn legacy_cache_dir(
    repo_root: &str,
    remote_repo: Option<&str>,
    pr_number: Option<u64>,
    local_branch_view: Option<&str>,
) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    if let (Some(slug), Some(n)) = (remote_repo, pr_number) {
        let safe_slug = slug.replace('/', "-");
        return Some(
            PathBuf::from(&home)
                .join(".cache/er/remote")
                .join(format!("{safe_slug}-{n}")),
        );
    }
    if let Some(branch) = local_branch_view {
        let repo_slug = Path::new(repo_root)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo");
        if let Some(pr_num) = pr_number {
            return Some(
                PathBuf::from(&home)
                    .join(".cache/er/local")
                    .join(repo_slug)
                    .join(format!("pr-{pr_num}")),
            );
        }
        let safe_branch = branch.replace('/', "-");
        return Some(
            PathBuf::from(&home)
                .join(".cache/er/local")
                .join(repo_slug)
                .join(safe_branch),
        );
    }
    None
}

/// Copy review artifacts from `src` into `dst` when `dst` is empty. Returns true if anything was copied.
pub fn migrate_dir_if_empty(dst: &Path, src: &Path) -> std::io::Result<bool> {
    if !src.is_dir() || managed_dir_has_artifacts(dst) {
        return Ok(false);
    }
    let mut copied = false;
    copy_dir_merge(src, dst, &mut copied)?;
    Ok(copied)
}

fn copy_dir_merge(src: &Path, dst: &Path, copied: &mut bool) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_merge(&src_path, &dst_path, copied)?;
        } else if ty.is_file() && !dst_path.exists() {
            std::fs::copy(&src_path, &dst_path)?;
            *copied = true;
        }
    }
    Ok(())
}

/// Migrate from repo `.er/` and legacy cache into managed dir when managed is empty.
///
/// Repo-root `.er/` is only used for working-tree tabs (`local_branch_view` is `None`).
/// Read-only branch views must not inherit that directory — it usually belongs to
/// whichever branch was last reviewed in the clone, not the branch being opened.
pub fn migrate_into_managed(
    managed_dir: &Path,
    repo_root: &str,
    remote_repo: Option<&str>,
    pr_number: Option<u64>,
    local_branch_view: Option<&str>,
) -> std::io::Result<bool> {
    if managed_dir_has_artifacts(managed_dir) {
        return Ok(false);
    }
    let mut any = false;
    if local_branch_view.is_none() {
        let repo_er = PathBuf::from(repo_root).join(".er");
        any |= migrate_dir_if_empty(managed_dir, &repo_er)?;
    }
    if let Some(legacy) = legacy_cache_dir(repo_root, remote_repo, pr_number, local_branch_view) {
        any |= migrate_dir_if_empty(managed_dir, &legacy)?;
    }
    Ok(any)
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
    fn migrate_skips_when_managed_has_files() {
        let tmp = TempDir::new().unwrap();
        let managed = tmp.path().join("managed");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("review.json"), "{}").unwrap();
        std::fs::create_dir_all(&managed).unwrap();
        std::fs::write(managed.join("questions.json"), "[]").unwrap();
        assert!(!migrate_dir_if_empty(&managed, &src).unwrap());
        assert!(!managed.join("review.json").exists());
    }

    #[test]
    fn migrate_copies_when_managed_empty() {
        let tmp = TempDir::new().unwrap();
        let managed = tmp.path().join("managed");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("review.json"), r#"{"version":1}"#).unwrap();
        assert!(migrate_dir_if_empty(&managed, &src).unwrap());
        assert!(managed.join("review.json").exists());
    }

    #[test]
    fn migrate_into_managed_skips_repo_er_for_local_branch_view() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("repo");
        let repo_er = repo_root.join(".er");
        let managed = tmp.path().join("managed-branch");
        std::fs::create_dir_all(&repo_er).unwrap();
        std::fs::write(repo_er.join("review.json"), r#"{"version":1}"#).unwrap();
        std::fs::create_dir_all(&managed).unwrap();

        let copied = migrate_into_managed(
            &managed,
            &repo_root.to_string_lossy(),
            None,
            None,
            Some("feature/foo"),
        )
        .unwrap();
        assert!(!copied);
        assert!(!managed.join("review.json").exists());
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

    #[test]
    fn migrate_into_managed_uses_repo_er_for_working_tree() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("repo");
        let repo_er = repo_root.join(".er");
        let managed = tmp.path().join("managed-main");
        std::fs::create_dir_all(&repo_er).unwrap();
        std::fs::write(repo_er.join("review.json"), r#"{"version":1}"#).unwrap();

        let copied =
            migrate_into_managed(&managed, &repo_root.to_string_lossy(), None, None, None).unwrap();
        assert!(copied);
        assert!(managed.join("review.json").exists());
    }
}
