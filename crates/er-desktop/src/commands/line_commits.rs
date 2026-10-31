//! Per-line commit attribution ("Mark lines") for the active view's diff.
//!
//! This is a lazy command rather than snapshot data: blame is a git shell-out
//! per file, so it runs only for files the user asks about, off the App lock
//! (ADR 0015/0032), and is kept out of the snapshot payload and its content
//! revision. Results are cached per file `cache_key` (ADR 0013) plus the
//! resolved range oids, so a rebase that keeps a file's hunks byte-identical
//! still re-attributes it.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use er_engine::app::{DiffMode, TabState};
use er_engine::git::{
    blame_range, git_merge_base, git_rev_parse_commit, BlameEnd, BlameRequest, FileStatus,
    LineAttribution,
};
use tauri::State;

use super::AppState;

/// Bounds memory across long sessions; one entry is one file of one range.
const CACHE_CAP: usize = 256;

pub type LineCommitsCache = Arc<Mutex<LineCommitsLru>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    root: String,
    start: Option<String>,
    end: String,
    working_tree: bool,
    path: String,
    old_path: Option<String>,
    /// The file's `FileSnapshot.cache_key`.
    hunks_key: String,
}

#[derive(Default)]
pub struct LineCommitsLru {
    entries: HashMap<CacheKey, LineAttribution>,
    order: VecDeque<CacheKey>,
}

impl LineCommitsLru {
    fn get(&self, key: &CacheKey) -> Option<LineAttribution> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: CacheKey, value: LineAttribution) {
        if self.entries.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CACHE_CAP {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LineCommits {
    pub path: String,
    /// Echoed so the frontend can drop a reply for hunks it no longer shows.
    pub cache_key: String,
    /// Why this view cannot be attributed; `None` when it was.
    pub unavailable: Option<String>,
    #[serde(flatten)]
    pub attribution: LineAttribution,
}

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

/// Mirror of how each view fetches its diff (`TabState::fetch_tab_raw_diff`
/// and the branch-scope preload), so blame reads the same two trees the diff
/// compared. `Err` is a reason to show the user, not a failure.
fn resolve(tab: &TabState, path: &str) -> Result<Target, String> {
    if tab.is_remote() {
        return Err(REMOTE_REASON.into());
    }
    if tab.repo_root.trim().is_empty() {
        return Err("This tab has no local repository.".into());
    }
    let source = match tab.mode {
        DiffMode::Unstaged | DiffMode::Staged => return Err(UNCOMMITTED_REASON.into()),
        DiffMode::Conflicts | DiffMode::Hidden => {
            return Err("This view does not show a commit range.".into())
        }
        DiffMode::History => {
            let sha = tab
                .history
                .as_ref()
                .and_then(|h| h.commits.get(h.selected_commit))
                .map(|c| c.hash.clone())
                .ok_or_else(|| "No commit is selected.".to_owned())?;
            Source::Commit {
                root: tab.repo_root.clone(),
                sha,
            }
        }
        DiffMode::Branch | DiffMode::PrDiff | DiffMode::Tour => branch_source(tab)?,
    };
    let files = match tab.mode {
        DiffMode::History => tab
            .history
            .as_ref()
            .map_or(&[][..], |h| &h.commit_files[..]),
        _ => &tab.files[..],
    };
    let old_path = files
        .iter()
        .find(|f| f.path == path)
        .and_then(|f| match &f.status {
            FileStatus::Renamed(old) | FileStatus::Copied(old) => Some(old.clone()),
            _ => None,
        });
    Ok(Target { source, old_path })
}

fn branch_source(tab: &TabState) -> Result<Source, String> {
    let base = tab.base_branch.clone();
    let Some(branch) = tab.local_branch_view.as_ref() else {
        return Ok(Source::Range {
            root: tab.repo_root.clone(),
            base,
            head: tab.pr_head_ref.clone().unwrap_or_else(|| "HEAD".into()),
        });
    };
    let is_pr = tab.pr_number.is_some();
    // A PR tab compares refs (PR parity) in PR Diff and Tour, and whenever its
    // head is not checked out; a checked-out branch view diffs its working tree.
    let parity = is_pr && matches!(tab.mode, DiffMode::PrDiff | DiffMode::Tour);
    if let (false, Some(root)) = (parity, tab.local_branch_checkout_root.as_ref()) {
        return Ok(Source::WorkingTree {
            root: root.clone(),
            base,
        });
    }
    let head = if is_pr {
        tab.pr_head_ref
            .clone()
            .ok_or_else(|| "The PR's commits are still being fetched.".to_owned())?
    } else {
        branch.clone()
    };
    Ok(Source::Range {
        root: tab.repo_root.clone(),
        base,
        head,
    })
}

/// The git half: resolve oids, then hit the cache or blame.
fn attribute(
    cache: &LineCommitsCache,
    target: &Target,
    path: &str,
    cache_key: &str,
) -> Result<LineAttribution, String> {
    let err = |e: anyhow::Error| e.to_string();
    let (root, start, end, working_tree) = match &target.source {
        Source::Range { root, base, head } => {
            let end = git_rev_parse_commit(root, head).map_err(err)?;
            let start = git_merge_base(root, base, &end).map_err(err)?;
            (root, Some(start), end, false)
        }
        Source::WorkingTree { root, base } => {
            let end = git_rev_parse_commit(root, "HEAD").map_err(err)?;
            let start = git_merge_base(root, base, &end).map_err(err)?;
            (root, Some(start), end, true)
        }
        Source::Commit { root, sha } => {
            let end = git_rev_parse_commit(root, sha).map_err(err)?;
            // A root commit has no parent: every line of it is its own.
            let start = git_rev_parse_commit(root, &format!("{end}^")).ok();
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
    let attribution = blame_range(&BlameRequest {
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

fn line_commits_for(
    app: &Mutex<er_engine::app::App>,
    cache: &LineCommitsCache,
    path: String,
    cache_key: String,
) -> Result<LineCommits, String> {
    let target = {
        let app = app.lock().map_err(|e| e.to_string())?;
        resolve(app.tab(), &path)
    };
    let (unavailable, attribution) =
        match target.and_then(|target| attribute(cache, &target, &path, &cache_key)) {
            Ok(attribution) => (None, attribution),
            Err(reason) => (Some(reason), LineAttribution::default()),
        };
    Ok(LineCommits {
        path,
        cache_key,
        unavailable,
        attribution,
    })
}

/// Which commit introduced (or removed) each changed line of `path` in the
/// active view. `cache_key` is the file's `FileSnapshot.cache_key`.
#[tauri::command]
pub async fn line_commits(
    path: String,
    cache_key: String,
    state: State<'_, AppState>,
) -> Result<LineCommits, String> {
    let state = state.inner().clone();
    super::run_blocking(move || {
        line_commits_for(&state.app, &state.line_commits_cache, path, cache_key)
    })
    .await
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
        app.tabs[0].repo_root = root.to_str().unwrap().into();
        app.tabs[0].base_branch = "main".into();
        app.tabs[0].mode = DiffMode::Branch;
        let app = Mutex::new(app);
        let cache = LineCommitsCache::default();

        let first = line_commits_for(&app, &cache, "f".into(), "k1".into()).unwrap();
        assert_eq!(first.unavailable, None);
        assert_eq!(first.attribution.commits, std::slice::from_ref(&head));
        assert_eq!(first.attribution.added.len(), 1);
        assert_eq!(cache.lock().unwrap().entries.len(), 1);

        let again = line_commits_for(&app, &cache, "f".into(), "k1".into()).unwrap();
        assert_eq!(again.attribution, first.attribution);
        assert_eq!(cache.lock().unwrap().entries.len(), 1);

        line_commits_for(&app, &cache, "f".into(), "k2".into()).unwrap();
        assert_eq!(cache.lock().unwrap().entries.len(), 2);

        let commit = Target {
            source: Source::Commit {
                root: root.to_str().unwrap().into(),
                sha: head.clone(),
            },
            old_path: None,
        };
        let single = attribute(&cache, &commit, "f", "k1").unwrap();
        assert_eq!(single.commits, std::slice::from_ref(&head));
        assert_eq!(single.added.len(), 1);
        assert_eq!(single.added[0].start, 2);

        app.lock().unwrap().tabs[0].mode = DiffMode::Unstaged;
        let off = line_commits_for(&app, &cache, "f".into(), "k1".into()).unwrap();
        assert_eq!(off.unavailable.as_deref(), Some(UNCOMMITTED_REASON));
        assert!(off.attribution.commits.is_empty());
    }

    #[test]
    fn cache_evicts_oldest_past_the_cap() {
        let mut lru = LineCommitsLru::default();
        let key = |i: usize| CacheKey {
            root: String::new(),
            start: None,
            end: String::new(),
            working_tree: false,
            path: i.to_string(),
            old_path: None,
            hunks_key: String::new(),
        };
        for i in 0..=CACHE_CAP {
            lru.insert(key(i), LineAttribution::default());
        }
        assert_eq!(lru.entries.len(), CACHE_CAP);
        assert!(lru.get(&key(0)).is_none());
        assert!(lru.get(&key(CACHE_CAP)).is_some());
    }
}
