//! Per-line commit attribution ("Mark lines") for the active view's diff.
//!
//! This is a lazy command rather than snapshot data: blame is a git shell-out
//! per file, so it runs only for files the user asks about, off the App lock
//! (ADR 0015/0032), and is kept out of the snapshot payload and its content
//! revision. Results are cached per file `cache_key` (ADR 0013) plus the
//! resolved range oids, so a rebase that keeps a file's hunks byte-identical
//! still re-attributes it.

#[cfg(test)]
use std::sync::Mutex;

use er_engine::app::DiffMode;
#[cfg(test)]
use er_engine::app::TabState;
use er_engine::git::{
    BlameEnd, BlameRequest, BlameSpan, DiffFile, FileStatus, GitError, LineAttribution,
};
use er_protocol::{ErError, LineCommits};
use tauri::State;

use crate::line_commits_cache::CacheKey;
pub use crate::line_commits_cache::{LineCommitsCache, LineCommitsLru};

/// Where the diff's lines come from, resolved from the tab under the lock so
/// the git work after it never needs the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// `git diff <base>...<head>`.
    Range {
        root: String,
        base: String,
        head: String,
    },
    /// A checked-out branch diffed with its working tree against the base.
    WorkingTree { root: String, base: String },
    /// One commit (History).
    Commit { root: String, sha: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    source: Source,
    old_path: Option<String>,
}

const REMOTE_REASON: &str = "Remote PR tabs have no local commits to attribute lines to.";
const UNCOMMITTED_REASON: &str =
    "Unstaged and staged changes are not committed yet. Switch to the branch view.";

/// What blame reads of a tab: the App's tab, or a tab's published content.
struct ShownTab<'a> {
    remote: bool,
    repo_root: &'a str,
    mode: DiffMode,
    /// The commit History shows.
    history_sha: Option<&'a str>,
    base_branch: &'a str,
    local_branch_view: Option<&'a str>,
    checkout_root: Option<&'a str>,
    pr_number: Option<u64>,
    pr_head_ref: Option<&'a str>,
    /// The file's rename or copy source in the diff the view shows.
    old_path: Option<String>,
}

fn rename_source<'a>(files: impl IntoIterator<Item = &'a DiffFile>, path: &str) -> Option<String> {
    files
        .into_iter()
        .find(|f| f.path == path)
        .and_then(|f| match &f.status {
            FileStatus::Renamed(old) | FileStatus::Copied(old) => Some(old.clone()),
            _ => None,
        })
}

/// The App's active tab, as blame reads it.
#[cfg(test)]
fn resolve(tab: &TabState, path: &str) -> Result<Target, String> {
    let history = tab.history.as_ref();
    let files = match tab.mode {
        DiffMode::History => history.map_or(&[][..], |h| &h.commit_files[..]),
        _ => &tab.files[..],
    };
    resolve_view(ShownTab {
        remote: tab.is_remote(),
        repo_root: &tab.repo_root,
        mode: tab.mode,
        history_sha: history
            .and_then(|h| h.commits.get(h.selected_commit))
            .map(|c| c.hash.as_str()),
        base_branch: &tab.base_branch,
        local_branch_view: tab.local_branch_view.as_deref(),
        checkout_root: tab.local_branch_checkout_root.as_deref(),
        pr_number: tab.pr_number,
        pr_head_ref: tab.pr_head_ref.as_deref(),
        old_path: rename_source(files, path),
    })
}

/// A tab's published content, as blame reads it. In History its diff section
/// holds the selected commit's diff.
fn resolve_content(content: &er_service::TabContent, path: &str) -> Result<Target, String> {
    let tab = &content.tab;
    resolve_view(ShownTab {
        remote: tab.identity().is_remote(),
        repo_root: &tab.repo_root,
        mode: tab.mode,
        history_sha: content.history.as_ref().and_then(|h| h.selected.as_deref()),
        base_branch: &tab.base_branch,
        local_branch_view: tab.local_branch_view.as_deref(),
        checkout_root: tab.checkout_root.as_deref(),
        pr_number: tab.pr_number,
        pr_head_ref: tab.pr_head_ref.as_deref(),
        old_path: rename_source(content.diff.files.iter().map(|f| &**f), path),
    })
}

/// Mirror of how each view fetches its diff (`TabState::fetch_tab_raw_diff`
/// and the branch-scope preload), so blame reads the same two trees the diff
/// compared. `Err` is a reason to show the user, not a failure.
fn resolve_view(view: ShownTab<'_>) -> Result<Target, String> {
    if view.remote {
        return Err(REMOTE_REASON.into());
    }
    if view.repo_root.trim().is_empty() {
        return Err("This tab has no local repository.".into());
    }
    let source = match view.mode {
        DiffMode::Unstaged | DiffMode::Staged => return Err(UNCOMMITTED_REASON.into()),
        DiffMode::Conflicts | DiffMode::Hidden => {
            return Err("This view does not show a commit range.".into())
        }
        DiffMode::History => {
            let sha = view
                .history_sha
                .ok_or_else(|| "No commit is selected.".to_owned())?;
            Source::Commit {
                root: view.repo_root.to_owned(),
                sha: sha.to_owned(),
            }
        }
        DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour => branch_source(&view)?,
    };
    Ok(Target {
        source,
        old_path: view.old_path,
    })
}

fn branch_source(view: &ShownTab<'_>) -> Result<Source, String> {
    let base = view.base_branch.to_owned();
    let root = view.repo_root.to_owned();
    let Some(branch) = view.local_branch_view else {
        return Ok(Source::Range {
            root,
            base,
            head: view.pr_head_ref.unwrap_or("HEAD").to_owned(),
        });
    };
    let is_pr = view.pr_number.is_some();
    // A PR tab compares refs (PR parity) in PR Diff and Tour, and whenever its
    // head is not checked out; a checked-out branch view diffs its working tree.
    let parity = is_pr && matches!(view.mode, DiffMode::PrDiff | DiffMode::Tour);
    if let (false, Some(checkout)) = (parity, view.checkout_root) {
        return Ok(Source::WorkingTree {
            root: checkout.to_owned(),
            base,
        });
    }
    let head = if is_pr {
        view.pr_head_ref
            .ok_or_else(|| "The PR's commits are still being fetched.".to_owned())?
    } else {
        branch
    };
    Ok(Source::Range {
        root,
        base,
        head: head.to_owned(),
    })
}

/// The git half: resolve oids, then hit the cache or blame.
fn attribute(
    git: &dyn er_engine::git::Git,
    cache: &LineCommitsCache,
    target: &Target,
    path: &str,
    cache_key: &str,
) -> Result<LineAttribution, String> {
    let err = |e: GitError| e.to_string();
    let (root, start, end, working_tree) = match &target.source {
        Source::Range { root, base, head } => {
            let end = git.rev_parse_commit(root, head).map_err(err)?;
            let start = git.merge_base(root, base, &end).map_err(err)?;
            (root, Some(start), end, false)
        }
        Source::WorkingTree { root, base } => {
            let end = git.rev_parse_commit(root, "HEAD").map_err(err)?;
            let start = git.merge_base(root, base, &end).map_err(err)?;
            (root, Some(start), end, true)
        }
        Source::Commit { root, sha } => {
            let end = git.rev_parse_commit(root, sha).map_err(err)?;
            // A root commit has no parent: every line of it is its own.
            let start = git.rev_parse_commit(root, &format!("{end}^")).ok();
            (root, start, end, false)
        }
    };
    let key = CacheKey {
        root: root.clone(),
        start: start.clone(),
        end: end.clone(),
        working_tree,
        path: path.to_owned(),
        old_path: target.old_path.clone(),
        hunks_key: cache_key.to_owned(),
    };
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key)) {
        return Ok(hit);
    }
    let attribution = git
        .blame_range(&BlameRequest {
            repo_root: root,
            start: start.as_deref(),
            end: if working_tree {
                BlameEnd::WorkingTree
            } else {
                BlameEnd::Rev(&end)
            },
            path,
            old_path: target.old_path.as_deref(),
        })
        .map_err(err)?;
    if let Ok(mut c) = cache.lock() {
        c.insert(key, attribution.clone());
    }
    Ok(attribution)
}

#[cfg(test)]
fn line_commits_for(
    ports: &er_engine::ports::Ports,
    app: &Mutex<er_engine::app::App>,
    cache: &LineCommitsCache,
    path: String,
    cache_key: String,
) -> Result<LineCommits, String> {
    let target = {
        let app = app.lock().map_err(|e| e.to_string())?;
        resolve(app.tab(), &path)
    };
    Ok(answer(&*ports.git, cache, target, path, cache_key))
}

/// The attribution of `path` in the tab `wire` names, resolved from its
/// published content.
pub(crate) fn keyed_line_commits(
    desktop: &crate::state::DesktopState,
    wire: er_protocol::TabKey,
    path: String,
    cache_key: String,
) -> Result<LineCommits, ErError> {
    let content = super::content_of(desktop, wire)?;
    let target = resolve_content(&content, &path);
    let cache = &desktop.line_commits_cache;
    Ok(answer(&*desktop.ports.git, cache, target, path, cache_key))
}

/// The reply for `target`: its attribution, or the reason it has none.
fn answer(
    git: &dyn er_engine::git::Git,
    cache: &LineCommitsCache,
    target: Result<Target, String>,
    path: String,
    cache_key: String,
) -> LineCommits {
    let (unavailable, attribution) =
        match target.and_then(|target| attribute(git, cache, &target, &path, &cache_key)) {
            Ok(attribution) => (None, attribution),
            Err(reason) => (Some(reason), LineAttribution::default()),
        };
    LineCommits {
        path,
        cache_key,
        unavailable,
        commits: attribution.commits,
        added: wire_spans(attribution.added),
        deleted: wire_spans(attribution.deleted),
        deleted_attributed: attribution.deleted_attributed,
    }
}

fn wire_spans(spans: Vec<BlameSpan>) -> Vec<er_protocol::BlameSpan> {
    spans
        .into_iter()
        .map(|span| er_protocol::BlameSpan {
            start: span.start,
            len: span.len,
            commit: u32::try_from(span.commit).unwrap_or(u32::MAX),
        })
        .collect()
}

/// Which commit introduced (or removed) each changed line of `path` in the
/// view the tab `tab` shows. `cache_key` is the file row's `cache_key`.
#[tauri::command]
pub async fn line_commits(
    tab: er_protocol::TabKey,
    path: String,
    cache_key: String,
    desktop: State<'_, crate::state::DesktopState>,
) -> Result<LineCommits, ErError> {
    let desktop = desktop.inner().clone();
    crate::state::run_blocking(move || Ok(keyed_line_commits(&desktop, tab, path, cache_key)))
        .await
        .map_err(super::failed)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::git::DiffFile;
    use std::path::Path;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn tab() -> TabState {
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = "/repo".into();
        tab.base_branch = "main".into();
        tab
    }

    fn renamed(path: &str, old: &str) -> DiffFile {
        DiffFile {
            path: path.into(),
            status: FileStatus::Renamed(old.into()),
            hunks: vec![],
            adds: 0,
            dels: 0,
            compacted: false,
            raw_hunk_count: 0,
            similarity: Some(90),
        }
    }

    #[test]
    fn remote_and_uncommitted_views_are_unavailable_with_a_reason() {
        let mut remote = tab();
        remote.remote_repo = Some("o/r".into());
        assert_eq!(resolve(&remote, "f").unwrap_err(), REMOTE_REASON);
        for mode in [DiffMode::Unstaged, DiffMode::Staged] {
            let mut t = tab();
            t.mode = mode;
            assert_eq!(resolve(&t, "f").unwrap_err(), UNCOMMITTED_REASON);
        }
    }

    #[test]
    fn branch_view_blames_base_to_head_and_carries_the_rename_source() {
        let mut t = tab();
        t.mode = DiffMode::Branch;
        t.files = vec![renamed("new.rs", "old.rs")];
        let target = resolve(&t, "new.rs").unwrap();
        assert_eq!(
            target.source,
            Source::Range {
                root: "/repo".into(),
                base: "main".into(),
                head: "HEAD".into()
            }
        );
        assert_eq!(target.old_path.as_deref(), Some("old.rs"));
    }

    #[test]
    fn pr_tabs_use_the_head_ref_in_pr_diff_and_the_checkout_in_branch() {
        let mut t = tab();
        t.local_branch_view = Some("feat".into());
        t.local_branch_checkout_root = Some("/wt".into());
        t.pr_number = Some(7);
        t.pr_head_ref = Some("refs/er/pr/7/head".into());
        t.mode = DiffMode::PrDiff;
        assert_eq!(
            resolve(&t, "f").unwrap().source,
            Source::Range {
                root: "/repo".into(),
                base: "main".into(),
                head: "refs/er/pr/7/head".into()
            }
        );
        t.mode = DiffMode::Branch;
        assert_eq!(
            resolve(&t, "f").unwrap().source,
            Source::WorkingTree {
                root: "/wt".into(),
                base: "main".into()
            }
        );
        t.local_branch_checkout_root = None;
        t.pr_head_ref = None;
        assert!(resolve(&t, "f")
            .unwrap_err()
            .contains("still being fetched"));
    }

    #[test]
    fn history_without_a_selected_commit_is_unavailable() {
        let mut t = tab();
        t.mode = DiffMode::History;
        assert_eq!(resolve(&t, "f").unwrap_err(), "No commit is selected.");
    }

    #[test]
    fn attribution_is_cached_per_cache_key_and_range() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.invalid"]);
        git(root, &["config", "user.name", "Tests"]);
        std::fs::write(root.join("f"), "a\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "base"]);
        git(root, &["checkout", "-q", "-b", "feat"]);
        std::fs::write(root.join("f"), "a\nb\n").unwrap();
        git(root, &["commit", "-q", "-am", "add b"]);
        let head = git(root, &["rev-parse", "HEAD"]);

        let mut app = er_engine::app::App::new_for_test(vec![]);
        let ports = er_engine::ports::Ports::cli();
        app.ports = ports.clone();
        app.tabs[0].ports = ports.clone();
        app.tabs[0].repo_root = root.to_str().unwrap().into();
        app.tabs[0].base_branch = "main".into();
        app.tabs[0].mode = DiffMode::Branch;
        let app = Mutex::new(app);
        let cache = LineCommitsCache::default();

        let first = line_commits_for(&ports, &app, &cache, "f".into(), "k1".into()).unwrap();
        assert_eq!(first.unavailable, None);
        assert_eq!(first.commits, std::slice::from_ref(&head));
        assert_eq!(first.added.len(), 1);
        assert_eq!(cache.lock().unwrap().entries.len(), 1);

        let again = line_commits_for(&ports, &app, &cache, "f".into(), "k1".into()).unwrap();
        assert_eq!(again, first);
        assert_eq!(cache.lock().unwrap().entries.len(), 1);

        line_commits_for(&ports, &app, &cache, "f".into(), "k2".into()).unwrap();
        assert_eq!(cache.lock().unwrap().entries.len(), 2);

        let commit = Target {
            source: Source::Commit {
                root: root.to_str().unwrap().into(),
                sha: head.clone(),
            },
            old_path: None,
        };
        let single = attribute(&*ports.git, &cache, &commit, "f", "k1").unwrap();
        assert_eq!(single.commits, std::slice::from_ref(&head));
        assert_eq!(single.added.len(), 1);
        assert_eq!(single.added[0].start, 2);

        app.lock().unwrap().tabs[0].mode = DiffMode::Unstaged;
        let off = line_commits_for(&ports, &app, &cache, "f".into(), "k1".into()).unwrap();
        assert_eq!(off.unavailable.as_deref(), Some(UNCOMMITTED_REASON));
        assert!(off.commits.is_empty());
    }
}
