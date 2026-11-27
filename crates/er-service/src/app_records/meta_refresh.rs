//! The metadata refresh: each project's branches from git, and the
//! worktree rows the tick lists for an open root. Only the owner writes the
//! metadata cache, through `AppOwner::refresh_meta` and
//! `AppOwner::refresh_project_meta`.

use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::Mutex;

use crate::app_records::app_owner::ChangeHook;
use crate::app_records::projects;
use crate::app_records::sidebar::{BranchInfo, MetaCache, ProjectMeta, WorktreeSnapshot};

/// Refresh the per-project metadata cache by shelling out to git for each
/// known project. MUST NOT hold `AppState.app` — runs on a background thread.
pub(crate) fn refresh_meta_cache(
    git: &dyn er_engine::git::Git,
    active_root: &str,
    cache: &MetaCache,
    on_change: &ChangeHook,
) -> bool {
    refresh_meta_cache_filtered(git, active_root, cache, None, on_change)
}

/// Variant that refreshes a single project (by id), leaving entries for other
///
/// projects untouched. Used at startup so the active project's branches show
/// up immediately without paying for `git branch / worktree list / base
/// detection` on every other registered project first.
pub(crate) fn refresh_meta_cache_for_project(
    git: &dyn er_engine::git::Git,
    project_id: &str,
    cache: &MetaCache,
    on_change: &ChangeHook,
) -> bool {
    refresh_meta_cache_filtered(git, "", cache, Some(project_id), on_change)
}

fn meta_cache_fingerprint(cache: &MetaCache) -> u64 {
    use std::hash::Hash;
    let Ok(g) = cache.lock() else {
        return 0;
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    g.len().hash(&mut h);
    for (id, m) in g.iter() {
        id.hash(&mut h);
        m.current_branch.hash(&mut h);
        m.local_branches.len().hash(&mut h);
        m.auto_branches.len().hash(&mut h);
    }
    h.finish()
}

fn refresh_meta_cache_filtered(
    git: &dyn er_engine::git::Git,
    active_root: &str,
    cache: &MetaCache,
    only_project_id: Option<&str>,
    on_change: &ChangeHook,
) -> bool {
    let t0 = std::time::Instant::now();
    let fp_before = meta_cache_fingerprint(cache);
    let file = projects::load();
    let updates: Vec<(String, ProjectMeta)> = file
        .projects
        .iter()
        .filter(|p| !p.root_path.is_empty())
        .filter(|p| only_project_id.is_none_or(|id| p.id == id))
        .map(|p| {
            let current_branch = detect_current_branch(git, &p.root_path);
            let base_branch = detect_base_branch(git, &p.root_path);
            let raw_worktrees = git.list_worktrees(&p.root_path).unwrap_or_default();
            let local_branches = build_tracked_branches(
                git,
                &p.root_path,
                &base_branch,
                &current_branch,
                &p.tracked_branches,
                &p.dismissed_branches,
                &raw_worktrees,
            );
            let auto_branches = build_auto_branches(
                git,
                &p.root_path,
                &base_branch,
                &current_branch,
                &p.tracked_branches,
                &p.dismissed_branches,
                10,
                &raw_worktrees,
            );
            (
                p.id.clone(),
                ProjectMeta {
                    current_branch,
                    local_branches,
                    auto_branches,
                },
            )
        })
        .collect();
    let _ = active_root; // active_root is unused now that ProjectMeta drops worktrees.
    if let Ok(mut g) = cache.lock() {
        if only_project_id.is_some() {
            // Partial update — merge into the existing cache.
            for (id, meta) in updates {
                g.insert(id, meta);
            }
        } else {
            // Full sweep — replace, so deleted projects drop out.
            let mut next: HashMap<String, ProjectMeta> = HashMap::new();
            for (id, meta) in updates {
                next.insert(id, meta);
            }
            *g = next;
        }
    }
    let fp_after = meta_cache_fingerprint(cache);
    let projects_count = file
        .projects
        .iter()
        .filter(|p| !p.root_path.is_empty())
        .filter(|p| only_project_id.is_none_or(|id| p.id == id))
        .count();
    on_change(
        "meta_refresh",
        &[
            ("refresh_ms", t0.elapsed().as_millis().to_string()),
            ("projects", projects_count.to_string()),
            (
                "changed",
                if fp_before != fp_after {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            ),
            (
                "full_sweep",
                if only_project_id.is_none() {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            ),
        ],
    );
    fp_before != fp_after
}

#[derive(PartialEq, Eq)]
struct WorktreesMetaKey {
    repo_root: String,
    base_branch: String,
    fingerprint: u64,
}

/// Per-worktree PR metadata `(is_pr, pr_number, is_merged, remote)` keyed by worktree path.
type WorktreeMetaMap = HashMap<String, (bool, Option<u64>, bool, Option<String>)>;

/// Cached per-worktree PR metadata `(is_pr, pr_number, is_merged, remote)` keyed
/// by path.
///
/// `build_worktrees` runs on every snapshot build that carries content. The
/// cheap `list_worktrees` call stays live so add/remove/branch-switch is
/// reflected within one poll, while the per-worktree metadata is cached keyed on
/// a fingerprint of the worktree set: add/remove/switch changes the fingerprint
/// → cache miss → recompute, and a TTL backstops `is_merged` drift when the set
/// is unchanged.
///
/// A miss costs four git processes for the whole list — `list_worktrees`, the
/// remote, the branch merges, and `git branch --merged` — and no network. What
/// the builder must not do is move any of those inside the per-worktree loop:
/// every worktree of a repository shares its config and refs, so per-row
/// lookups ask the same question N times, on the snapshot path, with the App
/// lock held. See `build_worktrees`.
///
/// More than one entry, because reviewing a few PRs at once means alternating
/// between tabs whose `base_branch` differs, and `base_branch` is part of the
/// key: with a single slot, every switch drops the entry the other tab needs and
/// rebuilds it on the way back. Entries are about a kilobyte for a nine-worktree
/// repo, so the cap costs nothing worth measuring.
static WORKTREES_META_CACHE: Mutex<Vec<(WorktreesMetaKey, std::time::Instant, WorktreeMetaMap)>> =
    Mutex::new(Vec::new());

/// Entries kept before the least recently used is dropped.
const WORKTREES_META_CACHE_CAP: usize = 8;

/// How long an entry may go unrefreshed before the next use rebuilds it.
///
/// Generous on purpose, because the fingerprint carries each worktree's tip: a
/// commit, a rebase, or a branch switch invalidates the exact key immediately,
/// with no help from this timer. All the TTL still covers is drift the
/// fingerprint cannot see — the base branch advancing from somewhere other than
/// this machine — and a few minutes of staleness in a merged-branch colouring
/// costs far less than a rebuild on every tab switch.
const WORKTREES_META_TTL: std::time::Duration = std::time::Duration::from_secs(300);

fn worktrees_meta_cached(
    key: WorktreesMetaKey,
    compute: impl FnOnce() -> WorktreeMetaMap,
) -> WorktreeMetaMap {
    if let Ok(mut guard) = WORKTREES_META_CACHE.lock() {
        if let Some(idx) = guard.iter().position(|(cached_key, computed_at, _)| {
            *cached_key == key && computed_at.elapsed() < WORKTREES_META_TTL
        }) {
            // Move to the back so the cap drops the least recently used entry,
            // not merely the oldest.
            let entry = guard.remove(idx);
            let value = entry.2.clone();
            guard.push(entry);
            return value;
        }
    }
    let value = compute();
    if let Ok(mut guard) = WORKTREES_META_CACHE.lock() {
        guard.retain(|(cached_key, _, _)| *cached_key != key);
        guard.push((key, std::time::Instant::now(), value.clone()));
        while guard.len() > WORKTREES_META_CACHE_CAP {
            guard.remove(0);
        }
    }
    value
}

/// The worktree list, plus the cache key its contents fingerprint to.
fn worktrees_list_and_key(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
) -> (Vec<er_engine::git::Worktree>, WorktreesMetaKey) {
    let wts = git.list_worktrees(repo_root).unwrap_or_default();

    let fingerprint = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for wt in &wts {
            wt.path.hash(&mut h);
            wt.branch.hash(&mut h);
            // The tip, not only the branch name. `is_merged` compares tips, so
            // without this a commit or a rebase moves the answer while path and
            // branch stay put, and nothing in the key changes.
            //
            // Free: the oid arrives in the same `git worktree list --porcelain`
            // output this function already runs. The base branch's own tip is
            // deliberately NOT probed here — that would need a `rev-parse` on
            // every build (this block runs before the cache check, so per build
            // and not per miss) to save a miss that costs less than the probe.
            // Base drift stays on the TTL floor.
            wt.head.hash(&mut h);
        }
        h.finish()
    };
    let key = WorktreesMetaKey {
        repo_root: repo_root.to_string(),
        base_branch: base_branch.to_string(),
        fingerprint,
    };
    (wts, key)
}

/// Per-worktree metadata for a worktree list, from repository-wide lookups.
///
/// Every worktree `git worktree list` reports belongs to this repository and
/// reads the same `.git/config` and the same refs, so the remote and the branch
/// lookups are all per-repo values: each is read once here and shared across the
/// rows, rather than asked once per row. A per-worktree loop over these is N
/// identical subprocesses for one answer, on the snapshot path with the App lock
/// held.
fn compute_worktrees_meta(
    github: &dyn er_engine::github::GitHub,
    git: &dyn er_engine::git::Git,
    wts: &[er_engine::git::Worktree],
    repo_root: &str,
    base_branch: &str,
) -> WorktreeMetaMap {
    let repo_remote = projects::resolve_repo_remote(github, git, repo_root);
    let merges = pr_branch_merges(git, repo_root);
    let merged = merged_branches(git, repo_root, base_branch, wts.len());
    wts.iter()
        .map(|wt| {
            let (is_pr, pr_number, is_merged) =
                detect_pr_meta(&merges, &merged, &wt.branch, base_branch);
            (
                wt.path.clone(),
                (is_pr, pr_number, is_merged, repo_remote.clone()),
            )
        })
        .collect()
}

pub fn build_worktrees(
    github: &dyn er_engine::github::GitHub,
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
    current_root: &str,
) -> Vec<WorktreeSnapshot> {
    let (wts, key) = worktrees_list_and_key(git, repo_root, base_branch);

    let meta = worktrees_meta_cached(key, || {
        compute_worktrees_meta(github, git, &wts, repo_root, base_branch)
    });

    wts.into_iter()
        .map(|wt| {
            let (is_pr, pr_number, is_merged, remote) = meta
                .get(&wt.path)
                .cloned()
                .unwrap_or((false, None, false, None));
            WorktreeSnapshot {
                is_current: wt.path == current_root,
                branch: wt.branch,
                path: wt.path,
                is_pr,
                pr_number,
                is_merged,
                remote,
            }
        })
        .collect()
}

/// Local branches merged into `base_branch`, from one `git branch --merged`.
///
/// Replaces a `merge-base --is-ancestor` per branch, which spawned a git
/// process for every local branch on every meta refresh — 100+ on a repo with
/// 100 branches — all asking the same question of the same base. Empty when the
/// answer cannot be established, which callers read as "nothing is merged",
/// matching the per-branch form's `.unwrap_or(false)`.
fn merged_into_base(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
) -> std::collections::HashSet<String> {
    if base_branch.is_empty() {
        return std::collections::HashSet::new();
    }
    // `--format` drops the `* `/`+ ` prefixes `git branch` adds for the current
    // and worktree branches, so the output is bare names.
    git.branch_merged(repo_root, base_branch)
        .map(|stdout| {
            stdout
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// How many worktrees a repo may have before the merged-branch check is skipped.
///
/// `git branch --merged` lists every local branch, so on a repo with many
/// worktrees the check costs more than the colouring it drives is worth. Above
/// this, callers see "nothing merged" and render every branch uncoloured.
const MAX_WORKTREES_FOR_MERGED_CHECK: usize = 10;

/// Branches merged into `base_branch`, for the callers that colour a branch
/// list.
///
/// Empty when the check is skipped (too many worktrees) or the answer cannot be
/// established (no base branch, git failed) — callers treat all three alike, so
/// they only need `contains`.
fn merged_branches(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
    worktree_count: usize,
) -> std::collections::HashSet<String> {
    if worktree_count > MAX_WORKTREES_FOR_MERGED_CHECK {
        return std::collections::HashSet::new();
    }
    merged_into_base(git, repo_root, base_branch)
}

fn build_tracked_branches(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
    current_branch: &str,
    tracked: &[String],
    dismissed: &[String],
    worktrees: &[er_engine::git::Worktree],
) -> Vec<BranchInfo> {
    let Ok(text) = git.for_each_ref(
        repo_root,
        "%(refname:short)|%(upstream:short)",
        None,
        "refs/heads/",
    ) else {
        return Vec::new();
    };

    let merged = merged_branches(git, repo_root, base_branch, worktrees.len());

    // Build the full list first, then filter to the curated set (tracked ∪ {current}).
    let all: Vec<BranchInfo> = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '|');
            let name = parts.next()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let upstream_raw = parts.next().unwrap_or("").trim().to_string();
            let upstream = if upstream_raw.is_empty() {
                None
            } else {
                Some(upstream_raw)
            };
            let is_current = name == current_branch;
            let is_merged = name != base_branch && merged.contains(&name);
            let worktree_path = worktrees
                .iter()
                .find(|w| w.branch == name)
                .map(|w| w.path.clone());
            Some(BranchInfo {
                name,
                upstream,
                is_current,
                is_merged,
                worktree_path,
                pr_number: None,
            })
        })
        .collect();

    let worktree_branches: std::collections::HashSet<&str> =
        worktrees.iter().map(|w| w.branch.as_str()).collect();
    let dismissed_set: std::collections::HashSet<&str> =
        dismissed.iter().map(|s| s.as_str()).collect();

    // Visibility set: {current} ∪ tracked ∪ {branches with an active worktree},
    // minus dismissed (current branch is always shown).
    let mut visible: Vec<BranchInfo> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();

    if let Some(cur) = all.iter().find(|b| b.is_current) {
        visible.push(cur.clone());
        seen.insert(cur.name.clone());
    }
    for b in &all {
        if seen.contains(&b.name) {
            continue;
        }
        if dismissed_set.contains(b.name.as_str()) {
            continue;
        }
        let in_tracked = tracked.iter().any(|t| t == &b.name);
        let has_worktree = worktree_branches.contains(b.name.as_str());
        if in_tracked || has_worktree {
            visible.push(b.clone());
            seen.insert(b.name.clone());
        }
    }
    visible
}

#[expect(
    clippy::too_many_arguments,
    reason = "one argument per branch-list input"
)]
fn build_auto_branches(
    git: &dyn er_engine::git::Git,
    repo_root: &str,
    base_branch: &str,
    current_branch: &str,
    tracked: &[String],
    dismissed: &[String],
    limit: usize,
    worktrees: &[er_engine::git::Worktree],
) -> Vec<BranchInfo> {
    let Ok(text) = git.for_each_ref(
        repo_root,
        "%(refname:short)|%(upstream:short)",
        Some("-committerdate"),
        "refs/heads/",
    ) else {
        return Vec::new();
    };

    let merged = merged_branches(git, repo_root, base_branch, worktrees.len());

    let tracked_set: std::collections::HashSet<&str> = tracked.iter().map(|s| s.as_str()).collect();
    let dismissed_set: std::collections::HashSet<&str> =
        dismissed.iter().map(|s| s.as_str()).collect();

    let mut result: Vec<BranchInfo> = Vec::new();
    for line in text.lines() {
        if result.len() >= limit {
            break;
        }
        let mut parts = line.splitn(2, '|');
        let Some(name) = parts.next().map(|s| s.trim().to_string()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if name == current_branch {
            continue;
        }
        if dismissed_set.contains(name.as_str()) {
            continue;
        }
        if tracked_set.contains(name.as_str()) {
            continue;
        }
        let upstream_raw = parts.next().unwrap_or("").trim().to_string();
        let upstream = if upstream_raw.is_empty() {
            None
        } else {
            Some(upstream_raw)
        };
        let is_merged = name != base_branch && merged.contains(&name);
        let worktree_path = worktrees
            .iter()
            .find(|w| w.branch == name)
            .map(|w| w.path.clone());
        result.push(BranchInfo {
            name,
            upstream,
            is_current: false,
            is_merged,
            worktree_path,
            pr_number: None,
        });
    }
    result
}

fn detect_current_branch(git: &dyn er_engine::git::Git, repo_root: &str) -> String {
    git.current_branch(repo_root).unwrap_or_default()
}

fn detect_base_branch(git: &dyn er_engine::git::Git, repo_root: &str) -> String {
    // Cheap fallback: try common defaults
    for candidate in ["main", "master", "develop", "dev"] {
        let spec = format!("refs/heads/{candidate}");
        if git
            .rev_parse_verify(repo_root, &spec)
            .is_ok_and(|oid| oid.is_some())
        {
            return candidate.to_string();
        }
    }
    String::new()
}

/// Every `branch.<name>.merge` value in the repository, in one git call.
///
/// Branch config is repository-wide, so this answers for every worktree at once
/// and belongs outside any per-worktree loop.
fn pr_branch_merges(git: &dyn er_engine::git::Git, repo_root: &str) -> HashMap<String, String> {
    let Ok(stdout) = git.config_get_regexp(repo_root, r"^branch\..*\.merge$") else {
        return HashMap::new();
    };
    stdout
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(' ')?;
            let name = key.strip_prefix("branch.")?.strip_suffix(".merge")?;
            Some((name.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Per-worktree PR metadata, from repository-wide lookups.
///
/// `merges` and `merged` are read once for the repository rather than per
/// worktree: refs and branch config both live in the shared `.git`, so every
/// worktree would get the same answer anyway. The per-worktree form spawned two
/// git processes per row — eighteen on a nine-worktree repo — on the snapshot
/// path.
fn detect_pr_meta(
    merges: &HashMap<String, String>,
    merged: &std::collections::HashSet<String>,
    branch: &str,
    base: &str,
) -> (bool, Option<u64>, bool) {
    let pr_number = merges
        .get(branch)
        .and_then(|val| val.strip_prefix("refs/pull/"))
        .and_then(|rest| rest.strip_suffix("/head"))
        .and_then(|n| n.parse::<u64>().ok());

    // `merged` is already empty when the check is skipped or the base is
    // unknown — see `merged_branches` — so only the self-compare needs a guard.
    let is_merged = !base.is_empty() && branch != base && merged.contains(branch);

    (pr_number.is_some(), pr_number, is_merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    // ── stale-pill gate for PR tabs ──

    // ── pr_cache_fingerprint folds in head_oid ──

    #[test]
    fn worktrees_meta_cache_recomputes_only_on_key_change() {
        use std::cell::Cell;

        let calls = Cell::new(0u32);
        let compute = || {
            calls.set(calls.get() + 1);
            let mut m = HashMap::new();
            m.insert("/wt".to_string(), (true, Some(7u64), false, None));
            m
        };
        let key = || WorktreesMetaKey {
            repo_root: "/repo".to_string(),
            base_branch: "main".to_string(),
            // Unique fingerprint so this test never collides with cached state
            // left by other suites touching the shared static.
            fingerprint: 0xDEAD_BEEF,
        };

        // First call: cache miss → compute runs once, value flows through.
        let v1 = worktrees_meta_cached(key(), compute);
        assert_eq!(calls.get(), 1);
        assert_eq!(v1.get("/wt").cloned(), Some((true, Some(7), false, None)));

        // Same key within TTL: cache hit → no recompute, same value.
        let v2 = worktrees_meta_cached(key(), compute);
        assert_eq!(
            calls.get(),
            1,
            "same worktree set within TTL must reuse cached meta, not respawn git subprocesses"
        );
        assert_eq!(v2.get("/wt").cloned(), Some((true, Some(7), false, None)));

        // Changed fingerprint (worktree added/removed/switched): miss → recompute.
        let changed = WorktreesMetaKey {
            repo_root: "/repo".to_string(),
            base_branch: "main".to_string(),
            fingerprint: 0xFEED_FACE,
        };
        let _ = worktrees_meta_cached(changed, compute);
        assert_eq!(
            calls.get(),
            2,
            "a changed worktree set must invalidate the cache and recompute"
        );
    }

    #[test]
    fn worktrees_meta_cache_holds_entries_for_different_base_branches() {
        use std::cell::Cell;

        // The workflow this cap exists for: a few PRs open at once, alternating
        // between tabs whose base branch differs. `base_branch` is part of the
        // key, so a single-slot cache evicted the other tab on every switch and
        // rebuilt it on the way back.
        let calls = Cell::new(0u32);
        let key_for = |base: &str| WorktreesMetaKey {
            repo_root: "/two-base-repo".to_string(),
            base_branch: base.to_string(),
            // Distinct fingerprints so this test never collides with cached
            // state other suites left in the shared static.
            fingerprint: 0x0BAD_F00D,
        };

        let _ = worktrees_meta_cached(key_for("main"), || {
            calls.set(calls.get() + 1);
            HashMap::from([("/wt".to_string(), (false, None, false, None))])
        });
        let _ = worktrees_meta_cached(key_for("release/v1"), || {
            calls.set(calls.get() + 1);
            HashMap::from([("/wt".to_string(), (false, None, false, None))])
        });
        assert_eq!(calls.get(), 2);

        // Both keys must still hit: neither evicted the other.
        let _ = worktrees_meta_cached(key_for("main"), || {
            calls.set(calls.get() + 1);
            HashMap::new()
        });
        let _ = worktrees_meta_cached(key_for("release/v1"), || {
            calls.set(calls.get() + 1);
            HashMap::new()
        });
        assert_eq!(
            calls.get(),
            2,
            "switching between two base branches must not rebuild either entry"
        );
    }

    /// A repo with one branch merged back into `main` and one left open —
    /// the two cases `merged_into_base` has to tell apart.
    fn init_repo_with_a_merged_and_an_open_branch() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "t@example.com"]);
        run_git(root, &["config", "user.name", "t"]);
        run_git(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("f.txt"), "one\n").unwrap();
        run_git(root, &["add", "f.txt"]);
        run_git(root, &["commit", "-qm", "base"]);

        // Merged back into main.
        run_git(root, &["checkout", "-q", "-b", "merged-branch"]);
        run_git(
            root,
            &["commit", "-q", "--allow-empty", "-m", "merged work"],
        );
        run_git(root, &["checkout", "-q", "main"]);
        run_git(
            root,
            &["merge", "-q", "--no-ff", "-m", "merge", "merged-branch"],
        );

        // Branched from main and left alone.
        run_git(root, &["checkout", "-q", "-b", "open-branch"]);
        run_git(root, &["commit", "-q", "--allow-empty", "-m", "open work"]);
        run_git(root, &["checkout", "-q", "main"]);
        tmp
    }

    #[test]
    fn merged_into_base_lists_only_merged_branches() {
        let dir = init_repo_with_a_merged_and_an_open_branch();
        let root = dir.path().to_string_lossy().to_string();

        let merged = merged_into_base(&*er_engine::ports::Ports::cli().git, &root, "main");
        assert!(merged.contains("merged-branch"), "got {merged:?}");
        assert!(merged.contains("main"), "the base is merged into itself");
        assert!(
            !merged.contains("open-branch"),
            "an unmerged branch must not be listed: {merged:?}"
        );

        // An unresolvable base yields nothing rather than a wrong answer —
        // the same outcome the per-branch form's `.unwrap_or(false)` gave.
        let git = &*er_engine::ports::Ports::cli().git;
        assert!(merged_into_base(git, &root, "no-such-base").is_empty());
        assert!(merged_into_base(git, &root, "").is_empty());
    }

    #[test]
    fn merged_branches_skips_the_check_above_the_worktree_threshold() {
        // Above the threshold every branch renders uncoloured, which is the
        // same outcome as "nothing is merged" — the guard belongs where it is
        // now, not duplicated at both call sites.
        let dir = init_repo_with_a_merged_and_an_open_branch();
        let root = dir.path().to_string_lossy().to_string();

        let git = &*er_engine::ports::Ports::cli().git;
        let at_limit = merged_branches(git, &root, "main", MAX_WORKTREES_FOR_MERGED_CHECK);
        assert!(
            at_limit.contains("merged-branch"),
            "at the threshold the check still runs: {at_limit:?}"
        );

        let over_limit = merged_branches(git, &root, "main", MAX_WORKTREES_FOR_MERGED_CHECK + 1);
        assert!(
            over_limit.is_empty(),
            "past the threshold the check is skipped: {over_limit:?}"
        );
        assert!(
            merged_branches(git, &root, "", 0).is_empty(),
            "an empty base has nothing to compare against"
        );
    }

    #[test]
    fn detect_current_branch_reads_head_through_the_handle() {
        let fake = std::sync::Arc::new(
            er_git::FakeGit::default().with_current_branch(Ok("feature".into())),
        );
        let ports = er_engine::ports::Ports::with_git(fake.clone());

        let branch = detect_current_branch(&*ports.git, "/nonexistent-repo");

        assert_eq!(branch, "feature");
        assert!(
            fake.calls()
                .iter()
                .any(|call| call.contains("current_branch")),
            "the helper must read HEAD through the handle: {:?}",
            fake.calls()
        );
    }
}
