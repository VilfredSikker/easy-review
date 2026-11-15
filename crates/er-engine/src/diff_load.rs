//! Loading one tab's diff: fetch the raw text, parse it, hash it, and parse a
//! lazy file on demand.
//!
//! The steps read only the tab's identity and the ports, so they live in the
//! half of the engine that always compiles. `TabState`'s refresh runs them and
//! keeps the view work (selection restore, mtime sort, memory budget); the
//! headless service runs the same steps for the tabs it loads.

use crate::ai;
use crate::git::{self, CompactionConfig, DiffFile, DiffFileHeader};
use crate::ports::Ports;
use crate::tab_identity::{DiffMode, TabIdentity};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// Above this many bytes a diff is parsed lazily: headers only, with small
/// files parsed eagerly. A byte count is O(1) where counting newlines is O(n);
/// 200_000 bytes is about 5000 lines at ~40 bytes a line.
pub const LAZY_PARSE_BYTES: usize = 200_000;

/// A raw diff split into files.
pub struct ParsedDiff {
    pub files: Vec<DiffFile>,
    pub headers: Vec<DiffFileHeader>,
    /// Whether some files are unparsed stubs that [`parse_lazy_file`] fills.
    pub lazy: bool,
}

/// Parse `raw`, lazily when it is large. Low-value files are compacted;
/// `is_expanded` names the files the user opened, which a lazy parse keeps
/// whole.
pub fn parse_raw(
    raw: &str,
    compaction: &CompactionConfig,
    is_expanded: impl Fn(&str) -> bool,
) -> ParsedDiff {
    if raw.len() > LAZY_PARSE_BYTES {
        // Lazy mode: header-only parse; large/pattern files become compacted stubs,
        // small files are parsed eagerly so they render without a lazy round-trip.
        let headers = git::parse_diff_headers(raw);
        let files = git::lazy_files_with_compaction(raw, &headers, compaction, is_expanded);
        ParsedDiff {
            files,
            headers,
            lazy: true,
        }
    } else {
        // Eager mode: full parse (fast enough for smaller diffs)
        let mut files = git::parse_diff(raw);
        let headers = git::parse_diff_headers(raw);
        // Apply auto-compaction to low-value files
        git::compact_files(&mut files, compaction);
        ParsedDiff {
            files,
            headers,
            lazy: false,
        }
    }
}

/// The diff hash for `raw`. A full refresh needs the SHA-256, which the AI
/// sidecars compare against; a quick refresh (a watch event) uses the fast
/// hash and skips the expensive one.
pub fn diff_hash(raw: &str, full: bool) -> String {
    if full {
        ai::compute_diff_hash(raw)
    } else {
        format!("{:016x}", ai::compute_diff_hash_fast(raw))
    }
}

/// Per-file diff hashes of `raw` for the `reviewed` paths only — the ones the
/// auto-unmark pass consults without a user action. Every other path is
/// resolved on demand, so a refresh does not pay a SHA-256 per file.
pub fn reviewed_file_hashes(
    raw: &str,
    reviewed: &HashMap<String, String>,
) -> HashMap<String, String> {
    let wanted: HashSet<String> = reviewed.keys().cloned().collect();
    ai::compute_per_file_hashes_for(raw, &wanted)
}

/// The reviewed paths whose stored hash no longer matches `hashes`, which
/// must have been built for exactly the reviewed paths in this pass: a miss
/// there means the file left the diff. An empty stored hash is a legacy mark
/// with no hash to compare, and stays.
pub fn changed_reviewed(
    reviewed: &HashMap<String, String>,
    hashes: &HashMap<String, String>,
) -> Vec<String> {
    reviewed
        .iter()
        .filter_map(|(path, stored_hash)| {
            if stored_hash.is_empty() {
                return None;
            }
            match hashes.get(path) {
                Some(h) if h == stored_hash => None,
                _ => Some(path.clone()),
            }
        })
        .collect()
}

/// The reviewed paths absent from `active`: orphans from another branch's
/// marks, or legacy lines. A full refresh drops them.
pub fn orphaned_reviewed(reviewed: &HashMap<String, String>, active: &[DiffFile]) -> Vec<String> {
    let active: HashSet<&str> = active.iter().map(|f| f.path.as_str()).collect();
    reviewed
        .keys()
        .filter(|p| !active.contains(p.as_str()))
        .cloned()
        .collect()
}

/// GitHub PR diff for AI review when this tab is PR-associated. Skips local
/// working-tree scopes on a checked-out branch so Branch/Unstaged/Staged
/// use the working tree (live edits) instead of `gh pr diff`. PrDiff and
/// Tour still use `gh pr diff` — PrDiff is the head-vs-base parity view by
/// design, and Tour walks the branch/PR diff (its cached raw_diff is the
/// parity diff).
pub fn fetch_pr_diff(ports: &Ports, id: &TabIdentity<'_>) -> Option<Result<String>> {
    let pr_number = id.pr_number?;
    // Checked-out PR tab: Branch/Unstaged/Staged review the working tree,
    // not the PR parity diff. PrDiff/Tour keep the parity diff.
    if id.checkout_root.is_some()
        && matches!(
            id.mode,
            DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged
        )
    {
        return None;
    }
    // Remote-only tabs (`gh pr diff --repo`). Local PR tabs also store a
    // GitHub slug for status keys; they still have a clone, so they use
    // local `gh pr diff` below. Using the remote path here made Branch
    // and PR Diff load the same network diff.
    if id.is_remote() {
        if let Some(repo_slug) = id.remote_repo {
            let parts: Vec<&str> = repo_slug.split('/').collect();
            if parts.len() == 2 {
                return Some(
                    ports
                        .github
                        .pr_diff_remote(&*ports.git, parts[0], parts[1], pr_number)
                        .map_err(anyhow::Error::from),
                );
            }
        }
        return Some(Err(anyhow::anyhow!(
            "Remote tab missing owner/repo for PR diff"
        )));
    }
    if id.repo_root.is_empty() {
        return None;
    }
    Some(
        ports
            .github
            .pr_diff(&*ports.git, pr_number, id.repo_root)
            .map_err(anyhow::Error::from),
    )
}

/// Fetch raw unified-diff text using the same subprocess rules as a refresh.
///
/// Extracts only the git/gh calls — no parsing, hashing, or selection restore.
/// `committed_unpushed` is the Staged view's "just committed" state, which
/// shows the last commit while nothing new is staged.
pub fn fetch_raw(
    ports: &Ports,
    id: &TabIdentity<'_>,
    scope: &str,
    committed_unpushed: bool,
) -> Result<String> {
    if id.mode == DiffMode::History || id.mode == DiffMode::Conflicts {
        anyhow::bail!("Cannot fetch diff in {:?} mode", id.mode);
    }
    if id.mode == DiffMode::Hidden {
        return Ok(String::new());
    }

    if let Some(result) = fetch_pr_diff(ports, id) {
        return result;
    }

    if let Some(branch) = id.local_branch_view {
        let inputs = crate::diff_fetch::BranchScopeFetchInputs {
            ports: ports.clone(),
            repo_root: id.repo_root.to_string(),
            base_branch: id.base_branch.to_string(),
            local_branch_view: Some(branch.to_string()),
            pr_head_ref: id.pr_head_ref.map(str::to_string),
            pr_number: id.pr_number,
            checkout_root: id.checkout_root.map(str::to_string),
            remote_repo: id.remote_repo.map(str::to_string),
        };
        return crate::diff_fetch::fetch_branch_scope_raw(scope, &inputs);
    }

    if let Some(repo_slug) = id.remote_repo {
        let inputs = crate::diff_fetch::BranchScopeFetchInputs {
            ports: ports.clone(),
            repo_root: id.repo_root.to_string(),
            base_branch: id.base_branch.to_string(),
            local_branch_view: None,
            pr_head_ref: id.pr_head_ref.map(str::to_string),
            pr_number: id.pr_number,
            checkout_root: None,
            remote_repo: Some(repo_slug.to_string()),
        };
        return crate::diff_fetch::fetch_branch_scope_raw(scope, &inputs);
    }

    if scope == "staged" && committed_unpushed {
        let staged_raw =
            ports
                .git
                .diff_raw("staged", id.base_branch, id.repo_root, id.pr_head_ref)?;
        if !staged_raw.is_empty() {
            return Ok(staged_raw);
        }
        match ports.git.diff_raw_range("HEAD~1", "HEAD", id.repo_root) {
            Ok(raw) => return Ok(raw),
            Err(_) => return Ok(String::new()),
        }
    }

    ports
        .git
        .diff_raw(scope, id.base_branch, id.repo_root, id.pr_head_ref)
        .map_err(anyhow::Error::from)
}

/// The PR head commit a preview reads files at, for the views that show the
/// PR head rather than the working tree; `None` for the others.
pub fn fetch_preview_head(ports: &Ports, id: &TabIdentity<'_>) -> Option<String> {
    if id.pr_number.is_some()
        && (id.is_remote()
            || id.mode == DiffMode::PrDiff
            || (id.mode == DiffMode::Tour && id.tour_is_pr)
            || (id.local_branch_view.is_some() && id.checkout_root.is_none()))
    {
        id.remote_repo
            .and_then(|slug| slug.split_once('/'))
            .and_then(|(owner, repo)| {
                ports
                    .github
                    .pr_head_oid_remote(owner, repo, id.pr_number.unwrap_or(0))
                    .ok()
            })
    } else {
        None
    }
}

/// [`fetch_raw`] with the preview head read on both sides of it. The head is
/// kept only when it did not move during the fetch, so a preview never reads
/// files at a commit the diff was not taken from.
pub fn fetch_raw_with_preview_head(
    ports: &Ports,
    id: &TabIdentity<'_>,
    scope: &str,
    committed_unpushed: bool,
) -> Result<(String, Option<String>)> {
    let head = fetch_preview_head(ports, id);
    let raw = fetch_raw(ports, id, scope, committed_unpushed)?;
    let confirmed_head = fetch_preview_head(ports, id);
    Ok((raw, if head == confirmed_head { head } else { None }))
}

/// Parse the lazy stub at `index` from the raw diff, without changing any
/// selection. Returns `false` when there was nothing to do (the file is
/// already parsed or compacted), and `true` once a parse was attempted, after
/// which the caller rebuilds what depends on the file's hunks.
pub fn parse_lazy_file(
    ports: &Ports,
    id: &TabIdentity<'_>,
    files: &mut [DiffFile],
    headers: &[DiffFileHeader],
    raw: Option<&str>,
    index: usize,
) -> bool {
    if let Some(file) = files.get(index) {
        // Already parsed (has hunks) or is compacted — skip
        if !file.hunks.is_empty() || file.compacted {
            return false;
        }
    }
    // Parse on demand from raw diff — look up header by path (not index) to handle mtime sort
    let path = files.get(index).map(|f| f.path.clone());
    let header_idx = path
        .as_ref()
        .and_then(|p| headers.iter().position(|h| h.path == *p));
    if let (Some(raw), Some(idx)) = (raw, header_idx) {
        let header = &headers[idx];
        let parsed = git::parse_file_at_offset(raw, header);
        if !parsed.hunks.is_empty() {
            if let Some(file) = files.get_mut(index) {
                file.hunks = parsed.hunks;
                file.adds = parsed.adds;
                file.dels = parsed.dels;
            }
            return true;
        }
    }
    // Fallback: offset parse returned no hunks but file has changes — fetch from git directly
    // Skip git fallback in remote mode — raw_diff is our only source
    let fallback_path = if id.is_remote() {
        None
    } else {
        files
            .get(index)
            .filter(|file| file.adds + file.dels > 0)
            .map(|file| file.path.clone())
    };
    if let Some(path) = fallback_path {
        if let Ok(raw) = ports.git.diff_raw_file(
            id.mode.git_mode(),
            id.base_branch,
            id.repo_root,
            &path,
            None,
            id.pr_head_ref,
        ) {
            let parsed = git::parse_diff(&raw);
            if let Some(p) = parsed.into_iter().next() {
                if let Some(file) = files.get_mut(index) {
                    file.hunks = p.hunks;
                    file.adds = p.adds;
                    file.dels = p.dels;
                }
            }
        }
    }
    true
}

/// Plain inputs a PR refresh needs.
///
/// Owned, so [`fetch_pr_refresh_with`] can run with no App lock held and
/// no tab to borrow — which is the whole point of splitting the fetch out.
#[derive(Debug, Clone)]
pub struct PrRefreshInputs {
    pub repo_root: String,
    pub pr_number: u64,
}

/// What [`fetch_pr_refresh_with`] brought back, ready for a tab to adopt.
#[derive(Debug)]
pub struct PrRefreshResult {
    pub head_ref: String,
    /// The resolved remote-tracking base (e.g. `origin/main`).
    pub resolved_base: String,
    pub last_diff_head_oid: Option<String>,
    pub commits: Vec<crate::git::CommitInfo>,
}

/// Every network leg a local PR tab's refresh needs: fetch the PR head ref,
/// look up the base and the commits, fetch the base ref. The head fetch and
/// the `gh pr view` are independent, so they overlap; the base fetch needs the
/// base name the lookup returns, so it follows them.
pub fn fetch_pr_refresh_with(
    github: &dyn er_github::GitHub,
    git: &dyn er_git::Git,
    inputs: &PrRefreshInputs,
) -> Result<PrRefreshResult> {
    let pr_number = inputs.pr_number;
    let repo_root = inputs.repo_root.as_str();

    let (head_result, lookup_result) = std::thread::scope(|s| {
        let head_handle = s.spawn(move || crate::github::fetch_pr_head(git, pr_number, repo_root));
        let lookup = github.pr_base_branch_and_commits(repo_root, pr_number, 250);
        let head_result = head_handle
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("pr head fetch thread panicked")));
        (head_result, lookup)
    });
    let head_ref = head_result?;

    // Base branch and commit list are two fields of one PR, so one
    // `gh pr view` answers both. Two invocations cost two round trips
    // (~0.6s each) for the same object.
    let (base_branch, commits) = match lookup_result {
        Ok(pair) => pair,
        // A failed union cannot say which field was at fault, so fall back
        // to the two-call form and keep its error split: a base lookup
        // failure is fatal, a commit lookup failure is not (an empty list
        // keeps the existing one — see `apply_pr_commit_refresh`).
        Err(_) => (
            github.pr_base_branch(pr_number, repo_root)?,
            github
                .pr_commits(repo_root, pr_number, 250)
                .map_err(|e| {
                    eprintln!("sync: pr commits re-fetch failed for #{pr_number}: {e}");
                })
                .unwrap_or_default(),
        ),
    };

    let resolved_base = github.fetch_pr_base_ref(git, repo_root, pr_number, &base_branch)?;
    // The oid the diff is about to be computed against, so the desktop
    // freshness check can compare it to the latest PR head_oid.
    let last_diff_head_oid = crate::github::rev_parse_oid(git, repo_root, &head_ref);

    Ok(PrRefreshResult {
        head_ref,
        resolved_base,
        last_diff_head_oid,
        commits,
    })
}
