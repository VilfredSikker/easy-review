//! Managed review-artifact storage shared by TUI and Desktop.
//!
//! Flat branch-level layout under
//! `<app_data>/easy-review/v1/repos/<repo_slug>/branches/<branch_slug>/`.
//! Version 1.0.0 keeps its data under `v1/` so a 0.6 install's files are left
//! untouched and never half-read.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::ErRoot;

/// Directory the 1.0.0 data lives in, under the storage base.
const ROOT_VERSION_DIR: &str = "v1";

/// Longest readable part of a hashed slug, in bytes.
const READABLE_MAX_BYTES: usize = 80;

#[cfg(feature = "test-support")]
thread_local! {
    static TEST_STORAGE_ROOT: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Run `f` with the storage root overridden on this thread.
///
/// Tests that need an isolated root use this instead of setting
/// `ER_STORAGE_ROOT`: `set_var` rewrites the process environment while
/// parallel tests read it, which is a data race.
#[cfg(feature = "test-support")]
pub fn with_test_storage_root<R>(root: impl Into<PathBuf>, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<PathBuf>);

    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_STORAGE_ROOT.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }

    let previous = TEST_STORAGE_ROOT.with(|slot| slot.borrow_mut().replace(root.into()));
    let _reset = Reset(previous);
    f()
}

/// The storage base: the directory that holds `v1/` and any older data.
///
/// Overridden by `ER_STORAGE_ROOT` when set — used by tests to write under a
/// temporary directory without touching the real user data dir.
pub fn storage_base() -> PathBuf {
    #[cfg(feature = "test-support")]
    if let Some(root) = TEST_STORAGE_ROOT.with(|slot| slot.borrow().clone()) {
        return root;
    }
    storage_base_from(
        std::env::var("ER_STORAGE_ROOT").ok().as_deref(),
        dirs::data_dir(),
        dirs::home_dir(),
    )
}

/// Root of all managed review storage: the base, plus `v1/` unless the
/// override names the root directly.
///
/// `ER_STORAGE_ROOT` is used as given so every test that points it at a temp
/// directory keeps that directory as the root.
pub fn storage_root() -> PathBuf {
    #[cfg(feature = "test-support")]
    if let Some(root) = TEST_STORAGE_ROOT.with(|slot| slot.borrow().clone()) {
        return root;
    }
    storage_root_from(
        std::env::var("ER_STORAGE_ROOT").ok().as_deref(),
        dirs::data_dir(),
        dirs::home_dir(),
    )
}

/// [`storage_base`] with its inputs passed in, so tests need no environment.
pub fn storage_base_from(
    override_path: Option<&str>,
    data_dir: Option<PathBuf>,
    home: Option<PathBuf>,
) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    data_dir
        .unwrap_or_else(|| {
            home.unwrap_or_else(|| PathBuf::from("."))
                .join(".local")
                .join("share")
        })
        .join("easy-review")
}

/// [`storage_root`] with its inputs passed in, so tests need no environment.
pub fn storage_root_from(
    override_path: Option<&str>,
    data_dir: Option<PathBuf>,
    home: Option<PathBuf>,
) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    storage_base_from(None, data_dir, home).join(ROOT_VERSION_DIR)
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

/// The folder name for a repo, from its `owner/repo` identity.
///
/// `<readable>-<8 hex>`, where the hash covers the lowercased `owner/repo`
/// with no host, so SSH, HTTPS and an SSH host alias of one remote resolve to
/// the same folder. Two owners with the same repo name get different folders.
pub fn repo_storage_slug(owner: &str, repo: &str) -> String {
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let raw = format!("{owner}/{repo}").to_lowercase();
    named_slug(&raw, &readable_part(&raw))
}

/// The folder name for a repo, read from its `origin` remote.
///
/// Without a remote the canonical repo path is hashed, so two clones of
/// differently-named directories still get separate folders.
pub fn repo_storage_slug_from_root(git: &dyn er_git::Git, repo_root: &str) -> String {
    if let Some((owner, repo)) = crate::github::parse_origin_remote(git, repo_root) {
        return repo_storage_slug(&owner, &repo);
    }
    let canonical = std::fs::canonicalize(repo_root).unwrap_or_else(|_| PathBuf::from(repo_root));
    let raw = canonical.to_string_lossy().into_owned();
    let readable = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    named_slug(&raw, &readable_part(readable))
}

/// The folder name for a repo, preferring an explicit `owner/repo` over the
/// clone's origin.
pub fn repo_storage_slug_for(
    git: &dyn er_git::Git,
    repo_root: &str,
    remote_repo: Option<&str>,
) -> String {
    match remote_repo
        .map(str::trim)
        .filter(|remote| !remote.is_empty())
    {
        Some(remote) => match remote.split_once('/') {
            Some((owner, repo)) => repo_storage_slug(owner, repo),
            None => repo_storage_slug_from_root(git, repo_root),
        },
        None => repo_storage_slug_from_root(git, repo_root),
    }
}

/// Sanitize a branch name for use as a directory component.
///
/// The readable part is the slugified name; the hash keeps branches that
/// slugify alike (`feat/x` and `feat-x`) in separate folders.
pub fn slug_branch(branch: &str) -> String {
    named_slug(branch, &readable_part(branch))
}

/// The slugified name, lowercased and cut to at most [`READABLE_MAX_BYTES`]
/// on a character boundary.
fn readable_part(raw: &str) -> String {
    let slug = slugify(raw).to_lowercase();
    if slug.len() <= READABLE_MAX_BYTES {
        return slug;
    }
    let mut end = READABLE_MAX_BYTES;
    while !slug.is_char_boundary(end) {
        end -= 1;
    }
    slug.get(..end).map(str::to_string).unwrap_or(slug)
}

/// The first 8 hex characters of SHA-256 over `raw`.
fn short_hash(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    format!(
        "{:08x}",
        u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
    )
}

/// `<readable>-<8 hex>`; an empty readable part still yields a valid name.
fn named_slug(raw: &str, readable: &str) -> String {
    format!("{readable}-{}", short_hash(raw))
}

/// Every child of the storage base that belongs to a previous version.
///
/// The override, when set, names the root directly, so there is no old data.
pub fn legacy_root_entries() -> Vec<PathBuf> {
    let override_set = std::env::var("ER_STORAGE_ROOT").is_ok_and(|value| !value.is_empty());
    legacy_root_entries_in(&storage_base(), override_set)
}

/// [`legacy_root_entries`] with its inputs passed in, so tests need no environment.
pub fn legacy_root_entries_in(base: &Path, override_set: bool) -> Vec<PathBuf> {
    if override_set {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some(ROOT_VERSION_DIR))
        .collect();
    out.sort();
    out
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
    view_bucket_dir(
        &repo_storage_slug_from_root(git, repo_root),
        &slug_branch(branch),
        "branch",
    )
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
    fn storage_root_is_the_v1_directory_under_the_base() {
        assert_eq!(
            storage_root_from(None, Some(PathBuf::from("/d")), None),
            PathBuf::from("/d/easy-review/v1")
        );
        assert_eq!(
            storage_base_from(None, Some(PathBuf::from("/d")), None),
            PathBuf::from("/d/easy-review")
        );
    }

    #[test]
    fn an_explicit_storage_root_is_used_as_given() {
        assert_eq!(
            storage_root_from(Some("/tmp/x"), Some(PathBuf::from("/d")), None),
            PathBuf::from("/tmp/x")
        );
        assert_eq!(
            storage_base_from(Some("/tmp/x"), Some(PathBuf::from("/d")), None),
            PathBuf::from("/tmp/x")
        );
    }

    #[test]
    fn an_empty_storage_root_override_is_ignored() {
        assert_eq!(
            storage_root_from(Some(""), Some(PathBuf::from("/d")), None),
            PathBuf::from("/d/easy-review/v1")
        );
        assert_eq!(
            storage_base_from(Some(""), Some(PathBuf::from("/d")), None),
            PathBuf::from("/d/easy-review")
        );
    }

    #[test]
    fn legacy_entries_are_every_child_of_the_base_except_v1() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("repos")).unwrap();
        std::fs::write(tmp.path().join("config.toml"), "x").unwrap();
        std::fs::create_dir_all(tmp.path().join("v1")).unwrap();

        let entries = legacy_root_entries_in(tmp.path(), false);
        let names: Vec<String> = entries
            .iter()
            .map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap()
                    .to_string()
            })
            .collect();

        assert_eq!(names, ["config.toml", "repos"]);
    }

    #[test]
    fn no_legacy_entries_when_the_base_is_missing_or_holds_only_v1_or_the_override_is_set() {
        let missing = TempDir::new().unwrap().path().join("not-there");
        assert!(legacy_root_entries_in(&missing, false).is_empty());

        let only_v1 = TempDir::new().unwrap();
        std::fs::create_dir_all(only_v1.path().join("v1")).unwrap();
        assert!(legacy_root_entries_in(only_v1.path(), false).is_empty());

        let with_data = TempDir::new().unwrap();
        std::fs::create_dir_all(with_data.path().join("repos")).unwrap();
        assert!(legacy_root_entries_in(with_data.path(), true).is_empty());
    }

    #[test]
    fn slug_branch_replaces_slashes_and_hashes_the_raw_name() {
        let slug = slug_branch("feature/foo");
        assert_eq!(slug, "feature-foo-f9320326");
        assert_eq!(slug, slug_branch("feature/foo"));
    }

    #[test]
    fn branch_slugs_differ_for_names_that_slugify_alike() {
        assert_ne!(slug_branch("feat/x"), slug_branch("feat-x"));
        assert_ne!(slug_branch("Feature"), slug_branch("feature"));
        assert_ne!(slug_branch("é"), slug_branch("e\u{301}"));
    }

    #[test]
    fn a_long_branch_name_gives_a_component_of_at_most_89_bytes() {
        let name = "a".repeat(300);
        let slug = slug_branch(&name);
        assert_eq!(slug.len(), 89, "{slug}");
        assert!(slug.ends_with(&short_hash(&name)), "{slug}");
    }

    #[test]
    fn different_owners_with_the_same_repo_name_get_different_folders() {
        let slug = repo_storage_slug("acme", "widgets");
        assert_eq!(slug, "acme-widgets-d782c874");
        assert_eq!(slug.len(), "acme-widgets-".len() + 8);
        assert_ne!(slug, repo_storage_slug("other", "widgets"));
        assert_eq!(
            repo_storage_slug("Acme", "My-Repo.git"),
            "acme-my-repo-80b59845"
        );
    }

    #[test]
    fn ssh_https_and_host_alias_remotes_give_one_folder() {
        let parse = |url: &str| crate::github::parse_remote_url(url).expect(url);
        let (owner, repo) = parse("git@github.com:o/r.git");
        let expected = repo_storage_slug(&owner, &repo);

        for url in [
            "git@github.com:o/r.git",
            "https://github.com/o/r",
            "git@github-work:o/r.git",
        ] {
            let (owner, repo) = parse(url);
            assert_eq!(repo_storage_slug(&owner, &repo), expected, "{url}");
        }
    }

    fn repo_with_origin(origin: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(tmp.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["remote", "add", "origin", origin]);
        tmp
    }

    #[test]
    fn every_producer_gives_one_folder_for_one_pr() {
        let tmp = repo_with_origin("git@github.com:acme/widgets.git");
        let root = tmp.path().to_str().unwrap();
        let git = er_git::CliGit;
        let expected = "acme-widgets-d782c874";

        assert_eq!(
            crate::github::owner_repo_storage_slug("acme", "widgets"),
            expected
        );
        assert_eq!(
            crate::github::canonical_owner_repo_slug(&git, root).as_deref(),
            Some(expected)
        );
        assert_eq!(repo_storage_slug_for(&git, root, None), expected);
        assert_eq!(
            repo_storage_slug_for(&git, root, Some("acme/widgets")),
            expected
        );
        assert_eq!(repo_storage_slug_from_root(&git, root), expected);
    }

    #[test]
    fn a_clone_without_a_remote_hashes_its_canonical_path() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        std::fs::create_dir_all(first.path().join("same-name")).unwrap();
        std::fs::create_dir_all(second.path().join("same-name")).unwrap();
        let git = er_git::FakeGit::default();

        let one =
            repo_storage_slug_from_root(&git, first.path().join("same-name").to_str().unwrap());
        let two =
            repo_storage_slug_from_root(&git, second.path().join("same-name").to_str().unwrap());

        assert!(one.starts_with("same-name-"), "{one}");
        assert_eq!(one.len(), "same-name-".len() + 8, "{one}");
        assert_ne!(one, two, "two canonical paths must get different folders");
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
        let ports = crate::ports::Ports::for_test();
        let repo = repo_storage_slug_from_root(&*ports.git, "/tmp/example-repo");
        let branch = slug_branch("feature/foo");
        let dir = local_branch_bucket_dir(&*ports.git, "/tmp/example-repo", "feature/foo");
        let s = dir.to_string_lossy();
        assert!(
            s.contains(&format!(
                "repos/{repo}/branches/{branch}/view-buckets/branch"
            )),
            "{s}"
        );
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

#[cfg(test)]
mod store_scope_tests {
    use super::{pr_bucket_dir, storage_root, view_bucket_dir, STORAGE_TEST_ENV_LOCK};
    use er_store::{Artifact, ArtifactKey, ArtifactStore, BranchView, Mode, Scope, Slug};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct ScopeTestArtifact {
        version: u32,
        diff_hash: String,
    }

    impl Artifact for ScopeTestArtifact {
        const KIND: &'static str = "scope-test";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "scope-test.json";
        type Slot = ();
    }

    #[test]
    fn store_scope_paths_equal_the_engine_bucket_dirs() {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", tmp.path());
        let store = ArtifactStore::new(storage_root());

        let repo = Slug::new("my-repo").unwrap();
        let branch = Slug::new("feature-x").unwrap();
        for view in [
            BranchView::Branch,
            BranchView::Unstaged,
            BranchView::Staged,
            BranchView::History,
        ] {
            let key = ArtifactKey::<ScopeTestArtifact>::new(
                Scope::Branch {
                    repo: repo.clone(),
                    branch: branch.clone(),
                    view,
                },
                (),
            )
            .unwrap();
            assert_eq!(
                store.path(&key).parent().unwrap(),
                view_bucket_dir("my-repo", "feature-x", view.as_str()).as_path()
            );
        }

        let pr = Scope::Pr {
            repo: Slug::new("owner-repo").unwrap(),
            number: 42,
        };
        let key = ArtifactKey::<ScopeTestArtifact>::new(pr, ()).unwrap();
        assert_eq!(
            store.path(&key).parent().unwrap(),
            pr_bucket_dir("owner-repo", 42).as_path()
        );

        std::env::remove_var("ER_STORAGE_ROOT");
    }
}
