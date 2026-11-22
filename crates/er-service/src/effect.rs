//! Off-actor effects: the code that may block.
//!
//! This module runs on an effect thread, never on the actor loop, so its
//! `deps.ports()` call passes the guard. It resolves open targets and loads
//! each tab's content; the actor applies what a load returns.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use er_engine::ai::tab_ai::{assemble_tab_ai, TabAiLoad};
use er_engine::ai::{self, tab_refresh};
use er_engine::diff_load::{self, PrRefreshInputs};
use er_engine::git::{CompactionConfig, DiffFile, DiffFileHeader};
use er_engine::paths::ErRoot;
use er_engine::preview_key::{capture_stamps, StampSource};
use er_engine::session_store;
use er_engine::store_registry;
use er_engine::tab_identity::{resolve_descriptor, DiffMode, ResolvedTab};
use er_engine::tabs_record::TabDescriptor;

use crate::actor::WorkspaceDeps;
use crate::ai_extras::read_extras;
use crate::annotations::failed;
use crate::content::{
    read_stamped, AiSection, DiffMeta, DiffSection, LoadedDiff, LoadedTab, PrSection, ReloadedAi,
    ScopeStats, SidecarStamp, TabContent,
};
use crate::freshness::RemoteTips;
use crate::handle::WorkspaceError;
use crate::protocol::{OpenTarget, TabContext};
use crate::tab_pr::{load_pr, PrLoad};
use crate::workspace::WorkspaceTarget;

/// Resolves what to open: parse the target, ask GitHub for the PR state, and
/// return the typed target the actor applies.
pub(crate) fn resolve_target(
    deps: &WorkspaceDeps,
    target: &OpenTarget,
) -> Result<WorkspaceTarget, WorkspaceError> {
    let parts = pr_parts(target)?;
    let remote = if parts.owner.is_empty() || parts.repo.is_empty() {
        String::new()
    } else {
        format!("{}/{}", parts.owner, parts.repo)
    };
    let pr_state = if remote.is_empty() {
        None
    } else {
        Some(
            deps.ports()
                .github
                .pr_state(&parts.owner, &parts.repo, parts.number)
                .map_err(|error| WorkspaceError::Failed(error.to_string()))?,
        )
    };
    Ok(WorkspaceTarget {
        repo_root: target.repo_root().to_string(),
        remote,
        pr_number: Some(parts.number),
        branch: None,
        label: label(&parts),
        pr_state,
    })
}

struct PrParts {
    owner: String,
    repo: String,
    number: u64,
}

fn pr_parts(target: &OpenTarget) -> Result<PrParts, WorkspaceError> {
    match target {
        OpenTarget::PrUrl { url, .. } => {
            let pr = er_github::parse_github_pr_url(url)
                .ok_or_else(|| WorkspaceError::Failed(format!("not a GitHub PR URL: {url}")))?;
            Ok(PrParts {
                owner: pr.owner,
                repo: pr.repo,
                number: pr.number,
            })
        }
        OpenTarget::RemotePr {
            owner,
            repo,
            number,
            ..
        } => Ok(PrParts {
            owner: owner.clone(),
            repo: repo.clone(),
            number: *number,
        }),
        OpenTarget::PrReview {
            remote, pr_number, ..
        } => {
            let (owner, repo) = remote.split_once('/').unwrap_or_default();
            Ok(PrParts {
                owner: owner.to_string(),
                repo: repo.to_string(),
                number: *pr_number,
            })
        }
    }
}

fn label(parts: &PrParts) -> String {
    if parts.owner.is_empty() || parts.repo.is_empty() {
        format!("pr-{}", parts.number)
    } else {
        format!("{}/{}#{}", parts.owner, parts.repo, parts.number)
    }
}

/// The input of a tab's first load: its descriptor resolved as a restore
/// does, shown in `mode` when one is given.
pub(crate) fn first_load(
    deps: &WorkspaceDeps,
    tab: &TabDescriptor,
    mode: Option<DiffMode>,
) -> Result<LoadInput, WorkspaceError> {
    let mut resolved = resolve_descriptor(&*deps.ports().git, tab)
        .map_err(|error| failed("resolve the tab", error))?;
    if let Some(mode) = mode {
        resolved.mode = mode;
    }
    Ok(LoadInput {
        tab: resolved,
        auto_unmark: true,
        recheck_branch: false,
        fetch_pr_refs: false,
        pr_refs_fetched: false,
        prev_ai: None,
        prev_branch_diff_hash: String::new(),
        expanded: BTreeSet::new(),
        prev_pr: None,
        refresh_pr: false,
        adopt_head_oid: None,
        tips: RemoteTips::default(),
    })
}

/// What a tab load starts from.
pub(crate) struct LoadInput {
    pub tab: ResolvedTab,
    /// Clear reviewed marks whose file moved. A mode switch keeps them: the
    /// views show different diffs, so a different hash is not a change.
    pub auto_unmark: bool,
    /// Re-read the checkout's branch first, as a refresh of a plain tab does.
    pub recheck_branch: bool,
    /// Fetch the refs the diff compares first, as a sync or a first entry into
    /// the PR diff does: a local PR's head and base, or a branch view's
    /// `origin/<base>`.
    pub fetch_pr_refs: bool,
    pub pr_refs_fetched: bool,
    /// The AI state the tab held, `None` for a tab not loaded yet. A plain
    /// tab's non-branch view fetches the branch diff only when it held data,
    /// and a reload keeps its staleness.
    pub prev_ai: Option<Arc<AiSection>>,
    pub prev_branch_diff_hash: String,
    /// The compacted files the user expanded, which the load parses in full.
    pub expanded: BTreeSet<String>,
    /// The PR section the tab held, carried unless `refresh_pr` is set.
    pub prev_pr: Option<Arc<PrSection>>,
    pub refresh_pr: bool,
    /// A PR head the caller confirmed, adopted as the diff's head.
    pub adopt_head_oid: Option<String>,
    /// What the desktop's probes last posted, measured again against the
    /// loaded diff.
    pub tips: RemoteTips,
}

/// Fetches a branch view's `origin/<base>` again, so a sync compares against
/// current origin. Best effort, as on the App: offline, the diff is still
/// recomputed against the base already fetched. A base pinned under
/// `refs/er/` is a deleted branch's last commit, with nothing on origin.
fn fetch_branch_base(git: &dyn er_git::Git, tab: &mut ResolvedTab) {
    let base = tab.base_branch.clone();
    let base_short = base.strip_prefix("origin/").unwrap_or(&base);
    if er_engine::github::is_pinned_pr_base(base_short) {
        return;
    }
    if let Ok(resolved) = er_engine::github::fetch_base_branch_ref(git, &tab.repo_root, base_short)
    {
        tab.base_branch = resolved;
    }
}

/// `base` as a ref the diff can use, by the App's rule: a read-only branch
/// view takes a ref already present, `origin/<base>` first; every other tab
/// takes the local branch, `origin/<base>`, or fetches it from origin.
pub(crate) fn resolve_base(
    deps: &WorkspaceDeps,
    tab: &ResolvedTab,
    base: &str,
) -> Result<String, WorkspaceError> {
    let base = base.trim();
    if base.is_empty() {
        return Err(WorkspaceError::Failed("Base branch cannot be empty".into()));
    }
    if tab.identity().is_remote() {
        return Err(WorkspaceError::Failed(
            "Cannot change the compare base of a remote-only PR — the diff comes from GitHub's PR API"
                .into(),
        ));
    }
    let git = &*deps.ports().git;
    if tab.local_branch_view.is_some() && tab.checkout_root.is_none() {
        let short = base.strip_prefix("origin/").unwrap_or(base);
        return [format!("origin/{short}"), short.to_string()]
            .into_iter()
            .find(|candidate| er_engine::github::ref_exists_locally(git, &tab.repo_root, candidate))
            .ok_or_else(|| {
                WorkspaceError::Failed(format!(
                    "No local ref for '{base}' — fetch the branch first"
                ))
            });
    }
    er_engine::github::ensure_base_ref_available(git, &tab.repo_root, base)
        .map_err(|error| failed("resolve the base", error))
}

/// Loads one tab's content in its mode, by the steps a full diff refresh
/// runs on a `TabState`: fetch and parse the diff, hash it, load and assemble
/// the AI sidecars, follow moved comments, mark stale findings, and clear the
/// reviewed marks that no longer hold.
pub(crate) fn load_tab(
    deps: &WorkspaceDeps,
    input: LoadInput,
) -> Result<LoadedTab, WorkspaceError> {
    let ports = deps.ports();
    let git = &*ports.git;
    let mut tab = input.tab;
    if input.recheck_branch && tab.remote_repo.is_none() && tab.local_branch_view.is_none() {
        tab.current_branch = git
            .current_branch(&tab.repo_root)
            .map_err(|error| failed("read the checkout's branch", error))?;
    }
    // Resolved on every load by the desktop watcher's rule, so a branch
    // checked out, or moved to another worktree, since the last load is read
    // from where it is now.
    let watched = er_engine::checkout::watched_branch(
        tab.identity().is_remote(),
        tab.local_branch_view.as_deref(),
        &tab.current_branch,
    );
    tab.checkout_root = watched.and_then(|branch| {
        er_engine::checkout::resolve_head_checkout(git, &tab.repo_root, &branch)
    });
    let mut pr_refs_fetched = input.pr_refs_fetched;
    let (mut fetched_commits, mut fetched_head) = (None, None);
    if input.fetch_pr_refs && !tab.identity().is_remote() {
        if let Some(pr_number) = tab.pr_number {
            let fetched = diff_load::fetch_pr_refresh_with(
                &*ports.github,
                git,
                &PrRefreshInputs {
                    repo_root: tab.repo_root.clone(),
                    pr_number,
                },
            )
            .map_err(|error| failed("fetch the PR refs", error))?;
            tab.pr_head_ref = Some(fetched.head_ref);
            tab.base_branch = fetched.resolved_base;
            pr_refs_fetched = true;
            fetched_commits = Some(fetched.commits);
            fetched_head = fetched.last_diff_head_oid;
        } else if tab.mode == DiffMode::Branch {
            fetch_branch_base(git, &mut tab);
        }
    }
    if matches!(
        tab.mode,
        DiffMode::History | DiffMode::Conflicts | DiffMode::Hidden
    ) {
        return Err(WorkspaceError::Failed(format!(
            "the workspace does not load the {:?} view",
            tab.mode
        )));
    }
    if tab.mode == DiffMode::PrDiff && tab.pr_number.is_none() {
        return Err(WorkspaceError::Failed(
            "No PR number set for this tab".into(),
        ));
    }

    let id = tab.identity();
    let er_dir = id
        .managed_root(git)
        .unwrap_or_else(|| ErRoot::unresolved(&tab.repo_root, git))
        .er_dir();
    let comments_dir = id.pr_bucket_er_dir(git).unwrap_or_else(|| er_dir.clone());
    let remote_only = id
        .is_remote()
        .then_some(tab.remote_repo.as_deref())
        .flatten();
    let rules_key = er_engine::storage::rules_key(git, &tab.repo_root, remote_only);
    let risk_artifact = er_engine::config::load_risk_rules_table(&rules_key);
    let store = store_registry::store();
    let scope = crate::annotations::bucket_scope(&er_dir)?;
    let mut marks = session_store::load_reviewed(&store, &scope).unwrap_or_default();
    let (prev_tour_stale, prev_stale_files) = input
        .prev_ai
        .as_ref()
        .map(|ai| (ai.state.tour_stale, ai.state.stale_files.clone()))
        .unwrap_or_default();
    let held_ai = input
        .prev_ai
        .as_ref()
        .is_some_and(|ai| ai.state.has_data() || ai.state.has_questions() || ai.state.has_tour());
    let reload = |stamp: &SidecarStamp, diff_hash: &str, branch_diff_hash: &str| {
        let mut state = ai::load_ai_state_with_arbiter(
            &er_dir,
            branch_diff_hash,
            id.storage_branch_scope(),
            stamp.arbiter.as_deref(),
        );
        let load = TabAiLoad {
            identity: id,
            git,
            er_dir: &er_dir,
            diff_hash,
            branch_diff_hash,
        };
        assemble_tab_ai(&mut state, &load, prev_tour_stale, prev_stale_files.clone());
        state
    };

    let is_remote_pr = tab.pr_number.is_some()
        && tab
            .remote_repo
            .as_deref()
            .is_some_and(|slug| slug.split('/').count() == 2);
    let fetch = |scope: &str| {
        diff_load::fetch_raw_with_preview_head(ports, &id, scope, false)
            .map_err(|error| failed("fetch the diff", error))
    };
    let branch_view = tab.local_branch_view.is_some();
    // A branch view takes its working tree's diffs only when the branch is
    // checked out; otherwise every view reads the branch diff, as a remote PR
    // does.
    let diff_scope = if branch_view && tab.checkout_root.is_some() {
        tab.mode.git_mode()
    } else if branch_view || is_remote_pr {
        "branch"
    } else {
        tab.mode.fetch_scope()
    };
    let (raw, preview_head_oid) = fetch(diff_scope)?;
    let expanded = input.expanded;
    let mut parsed = diff_load::parse_raw(&raw, &CompactionConfig::default(), |path| {
        expanded.contains(path)
    });
    // An eager parse compacts by pattern alone; the files the user expanded
    // are parsed in full again, as the App's refresh does.
    if !parsed.lazy {
        for file in &mut parsed.files {
            if file.compacted && expanded.contains(&file.path) {
                expand_file(deps, &tab, &raw, &parsed.headers, file)
                    .map_err(|error| failed("expand a file", error))?;
            }
        }
    }
    let stamps = capture_stamps(
        git,
        &parsed.files,
        &StampSource {
            repo_root: &tab.repo_root,
            checkout_root: tab.checkout_root.as_deref(),
            is_remote: id.is_remote(),
            mode: tab.mode,
            committed_unpushed: false,
            revision: tab
                .local_branch_view
                .as_deref()
                .or(tab.pr_head_ref.as_deref()),
        },
    );
    // Only a live checkout has a working tree and an index to count: not a
    // remote PR, and not a branch view whose branch is checked out nowhere.
    let scope_stats = if !id.is_remote() && (!branch_view || tab.checkout_root.is_some()) {
        ScopeStats {
            unstaged: git.diff_shortstat(&tab.repo_root, false),
            staged: git.diff_shortstat(&tab.repo_root, true),
        }
    } else {
        ScopeStats::default()
    };
    let diff_hash = ai::compute_diff_hash(&raw);
    // A tour entered from the branch view walks the branch diff itself.
    let raw_is_branch_diff = is_remote_pr
        || tab.mode == DiffMode::Branch
        || (tab.mode == DiffMode::Tour && !tab.tour_is_pr);
    // A plain tab's other views need a second git call for the branch diff,
    // which only AI data, questions or a tour consume.
    let fetched_branch = if !branch_view && !raw_is_branch_diff && held_ai {
        Some(
            git.diff_raw(
                "branch",
                &tab.base_branch,
                &tab.repo_root,
                tab.pr_head_ref.as_deref(),
            )
            .map_err(|error| failed("fetch the branch diff", error))?,
        )
    } else {
        None
    };
    let branch_raw = if branch_view {
        None
    } else if raw_is_branch_diff {
        Some(raw.as_str())
    } else {
        fetched_branch.as_deref()
    };
    let branch_diff_hash = match (branch_view, branch_raw) {
        (true, _) => diff_hash.clone(),
        (false, Some(branch)) if fetched_branch.is_some() => ai::compute_diff_hash(branch),
        (false, Some(_)) => diff_hash.clone(),
        (false, None) => input.prev_branch_diff_hash,
    };
    let dirs = stamped_dirs(&er_dir, &comments_dir, &id, git);
    let (ai_stamp, (mut state, ai_extras)) = read_stamped(dirs, |stamp| {
        let state = reload(stamp, &diff_hash, &branch_diff_hash);
        (state, read_extras(&tab, git, &er_dir, &comments_dir))
    });
    // A branch view's refresh follows neither comments nor findings.
    if !branch_view {
        tab_refresh::relocate_all_comments(
            &mut state,
            &parsed.files,
            parsed.lazy,
            &diff_hash,
            &er_dir,
            || comments_dir.clone(),
        );
        tab_refresh::refresh_finding_staleness(&mut state, &parsed.files, parsed.lazy);
        if state.is_stale {
            if let Some(branch_raw) = branch_raw {
                tab_refresh::compute_stale_files(&mut state, branch_raw);
            }
        }
    }

    let file_hashes = diff_load::reviewed_file_hashes(&raw, &marks);
    let cleared = if input.auto_unmark {
        clear_moved_marks(
            &mut marks,
            &file_hashes,
            &parsed.files,
            id.persists_reviewed(),
            &store,
            &scope,
        )
    } else {
        0
    };

    let pr = load_pr(
        deps,
        &tab,
        PrLoad {
            prev: input.prev_pr.as_deref(),
            force: input.refresh_pr,
            fetched_commits,
            fetched_head,
            preview_head_oid: preview_head_oid.as_deref(),
            adopt_head_oid: input.adopt_head_oid,
        },
    );
    let freshness = crate::freshness::measure(git, &tab, input.tips, scope_stats.unstaged.0);
    let context = TabContext {
        er_dir,
        comments_dir,
        is_remote: id.is_remote(),
        repo_root: tab.repo_root.clone(),
        base_branch: tab.base_branch.clone(),
        scope: match tab.mode {
            DiffMode::Unstaged => "unstaged",
            DiffMode::Staged => "staged",
            _ => "branch",
        }
        .to_string(),
        remote_repo: tab.remote_repo.clone(),
        pr_number: tab.pr_number,
        pr_head_ref: tab.pr_head_ref.clone(),
        local_branch_view: tab.local_branch_view.clone(),
        checkout_root: tab.checkout_root.clone(),
        branch_diff_hash: branch_diff_hash.clone(),
        diff_hash: diff_hash.clone(),
    };
    Ok(LoadedTab {
        tab,
        context,
        pr_refs_fetched,
        diff: LoadedDiff {
            files: parsed.files,
            meta: DiffMeta {
                headers: parsed.headers,
                raw,
                lazy: parsed.lazy,
                diff_hash,
                branch_diff_hash,
                preview_head_oid,
                stamps,
                scope_stats,
                rules_key,
                risk_artifact,
            },
            expanded,
        },
        marks,
        file_hashes,
        ai: state,
        ai_stamp,
        ai_extras,
        pr,
        freshness,
        cleared,
    })
}

/// The AI state of `content` read again from its sidecars, as the App's
/// reload does: per-file staleness is kept, since only a diff refresh
/// recomputes it. `None` when the sidecars have not moved since the content
/// read them, unless `always`.
pub(crate) fn reload_ai(
    deps: &WorkspaceDeps,
    content: &TabContent,
    always: bool,
) -> Option<ReloadedAi> {
    let er_dir = content.context.er_dir.as_str();
    let dirs = content.ai.stamp.dirs.clone();
    let (stamp, state) = read_stamped(dirs, |stamp| {
        if !always && !content.ai.stamp.moved_to(stamp) {
            return None;
        }
        let id = content.tab.identity();
        let mut state = ai::load_ai_state_with_arbiter(
            er_dir,
            &content.context.branch_diff_hash,
            id.storage_branch_scope(),
            stamp.arbiter.as_deref(),
        );
        let load = TabAiLoad {
            identity: id,
            git: &*deps.ports().git,
            er_dir,
            diff_hash: &content.context.diff_hash,
            branch_diff_hash: &content.context.branch_diff_hash,
        };
        let prev = &content.ai.state;
        assemble_tab_ai(&mut state, &load, prev.tour_stale, prev.stale_files.clone());
        let comments_dir = &content.context.comments_dir;
        let extras = read_extras(&content.tab, load.git, er_dir, comments_dir);
        Some((state, extras))
    });
    let (state, extras) = state?;
    Some(ReloadedAi {
        state,
        stamp,
        extras,
    })
}

/// The buckets a tab's AI state reads, the view's own first: the
/// GitHub-comment bucket, then both tour buckets, each once.
fn stamped_dirs(
    er_dir: &str,
    comments_dir: &str,
    id: &er_engine::tab_identity::TabIdentity<'_>,
    git: &dyn er_git::Git,
) -> Vec<String> {
    let tours = [id.tour_bucket_er_dir(git), id.cross_tour_bucket_er_dir(git)];
    let mut dirs = vec![er_dir.to_string()];
    for dir in [Some(comments_dir.to_string())]
        .into_iter()
        .chain(tours)
        .flatten()
    {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// Drops the marks whose file left the diff or changed since it was marked,
/// and saves what is left when any were dropped. Answers how many.
fn clear_moved_marks(
    marks: &mut HashMap<String, String>,
    file_hashes: &HashMap<String, String>,
    files: &[DiffFile],
    persists: bool,
    store: &er_store::ArtifactStore,
    scope: &er_store::Scope,
) -> usize {
    let save = |marks: &HashMap<String, String>| {
        if persists {
            let _ = session_store::save_reviewed(store, scope, marks);
        }
    };
    let orphans = if files.is_empty() {
        Vec::new()
    } else {
        diff_load::orphaned_reviewed(marks, files)
    };
    for path in &orphans {
        marks.remove(path);
    }
    if !orphans.is_empty() {
        save(marks);
    }
    let moved = diff_load::changed_reviewed(marks, file_hashes);
    for path in &moved {
        marks.remove(path);
    }
    if !moved.is_empty() {
        save(marks);
    }
    orphans.len() + moved.len()
}

/// Parses the named lazy files of a loaded diff, each on its own copy, and
/// answers the ones a parse filled with their index in the section. With
/// `expand`, a compacted file is parsed in full too, and its path is answered
/// beside the files.
pub(crate) fn parse_files(
    deps: &WorkspaceDeps,
    tab: &ResolvedTab,
    diff: &DiffSection,
    paths: &[String],
    expand: bool,
) -> (Vec<(usize, DiffFile)>, Vec<String>) {
    let ports = deps.ports();
    let id = tab.identity();
    let mut expanded = Vec::new();
    let files = paths
        .iter()
        .filter_map(|path| {
            let index = diff.files.iter().position(|file| &file.path == path)?;
            let mut one = [(*diff.files[index]).clone()];
            if expand && one[0].compacted {
                let [mut file] = one;
                expand_file(deps, tab, &diff.meta.raw, &diff.meta.headers, &mut file).ok()?;
                expanded.push(file.path.clone());
                return Some((index, file));
            }
            let parsed = diff_load::parse_lazy_file(
                ports,
                &id,
                &mut one,
                &diff.meta.headers,
                Some(&diff.meta.raw),
                0,
            );
            let [file] = one;
            parsed.then_some((index, file))
        })
        .collect();
    (files, expanded)
}

/// Parses a compacted file in full, as the App's expand does: from the raw
/// diff when the tab has no checkout to ask (a remote or local PR), from git
/// otherwise.
fn expand_file(
    deps: &WorkspaceDeps,
    tab: &ResolvedTab,
    raw: &str,
    headers: &[DiffFileHeader],
    file: &mut DiffFile,
) -> anyhow::Result<()> {
    if tab.identity().is_remote() || tab.pr_number.is_some() {
        let header = headers
            .iter()
            .find(|header| header.path == file.path)
            .ok_or_else(|| anyhow::anyhow!("{} is not in the diff", file.path))?;
        let parsed = er_engine::git::parse_file_at_offset(raw, header);
        file.hunks = parsed.hunks;
        file.adds = parsed.adds;
        file.dels = parsed.dels;
    } else {
        er_engine::git::expand_compacted_file(
            file,
            &*deps.ports().git,
            &tab.repo_root,
            tab.mode.fetch_scope(),
            &tab.base_branch,
            tab.pr_head_ref.as_deref(),
        )?;
    }
    file.compacted = false;
    Ok(())
}
