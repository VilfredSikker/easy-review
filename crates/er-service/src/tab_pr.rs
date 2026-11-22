//! A PR tab's overview, commits and head, as its content publishes them.
//!
//! A tab fills its PR section once, from the app registry's open cache when
//! a prefetch warmed it and from GitHub otherwise, and carries it across later
//! loads; a sync (`force`) reads it again. Every read is best effort: a PR
//! GitHub cannot describe still shows its diff.

use er_engine::gh_stack::StackSource;
use er_engine::git::CommitInfo;
use er_engine::github::PrOverviewData;
use er_engine::pr_open_entry::PR_COMMIT_CACHE_LIMIT;
use er_engine::tab_identity::ResolvedTab;

use crate::actor::WorkspaceDeps;
use crate::app_registry::CachedPr;
use crate::content::{LoadedPr, PrSection};

/// What a load knows about the PR beside the tab.
pub(crate) struct PrLoad<'a> {
    /// The section the tab held before this load.
    pub prev: Option<&'a PrSection>,
    /// Read the overview and commits again, as a sync does.
    pub force: bool,
    /// The commits and head a PR ref fetch in this load brought back.
    pub fetched_commits: Option<Vec<CommitInfo>>,
    pub fetched_head: Option<String>,
    /// The PR head the diff fetch was taken at.
    pub preview_head_oid: Option<&'a str>,
    /// A head the caller has confirmed, which wins over every other source.
    pub adopt_head_oid: Option<String>,
}

/// The PR section of a load of `tab`. The stack the tab held is kept while
/// the tab still reads it from the same place.
pub(crate) fn load_pr(deps: &WorkspaceDeps, tab: &ResolvedTab, load: PrLoad<'_>) -> LoadedPr {
    let held = load.prev.and_then(|prev| prev.stack.clone());
    let mut loaded = load_pr_fields(deps, tab, load);
    let source = stack_source(tab, loaded.overview.as_ref());
    loaded.stack = held.filter(|read| source.as_ref() == Some(&read.source));
    loaded
}

/// Where `tab`'s stack is read from, as `TabState::stack_source` finds it:
/// `gh stack view` in the checkout that holds the viewed branch, else the
/// PR chain of a PR's head; none when the tab has neither.
pub(crate) fn stack_source(
    tab: &ResolvedTab,
    overview: Option<&PrOverviewData>,
) -> Option<StackSource> {
    if !tab.identity().is_remote() {
        let checkout = match tab.local_branch_view {
            Some(_) => tab.checkout_root.as_deref(),
            None => Some(tab.repo_root.as_str()),
        };
        if let Some(root) = checkout {
            return Some(StackSource::Checkout(root.to_string()));
        }
    }
    tab.pr_number?;
    let repo = tab.remote_repo.clone().filter(|r| !r.is_empty())?;
    let head = overview
        .map(|pr| pr.head_branch.clone())
        .filter(|head| !head.is_empty())
        .or_else(|| {
            tab.local_branch_view
                .clone()
                .filter(|b| !er_engine::storage::is_pr_placeholder_branch(b))
        })?;
    Some(StackSource::PrChain { repo, head })
}

/// One stack lookup from `source`, through the GitHub port. Blocking.
pub(crate) fn read_stack(
    deps: &WorkspaceDeps,
    source: &StackSource,
) -> er_engine::gh_stack::StackInfo {
    er_engine::gh_stack::load_from_with(&*deps.ports().github, source)
}

fn load_pr_fields(deps: &WorkspaceDeps, tab: &ResolvedTab, load: PrLoad<'_>) -> LoadedPr {
    let Some(number) = tab.pr_number else {
        return LoadedPr::default();
    };
    let remote = remote_parts(tab);
    let prev = load.prev.filter(|prev| prev.pr_number == Some(number));
    let cached = || {
        deps.app_registry()
            .and_then(|registry| registry.cached_pr(remote, &tab.repo_root, number))
            .unwrap_or_default()
    };
    let (overview, commits, cached_head) = match prev {
        Some(prev) if !load.force => (prev.overview.clone(), prev.commits.clone(), None),
        _ => {
            let CachedPr {
                overview,
                commits,
                head_oid,
            } = if load.force {
                CachedPr::default()
            } else {
                cached()
            };
            let overview = overview.or_else(|| fetch_overview(deps, tab, remote, number));
            let commits = commits.unwrap_or_else(|| fetch_commits(deps, tab, remote, number));
            (overview, commits, head_oid)
        }
    };
    // An empty fetch keeps the list the tab had, as a sync does.
    let commits = match load.fetched_commits {
        Some(fetched) if !fetched.is_empty() => fetched,
        _ => commits,
    };
    // The head this load's diff was taken at comes first. The cache's head
    // describes the cached diff, which this load did not read, so it is the
    // last resort.
    let head_oid = load
        .adopt_head_oid
        .or(load.fetched_head)
        .or_else(|| load.preview_head_oid.map(str::to_string))
        .or_else(|| prev.and_then(|prev| prev.head_oid.clone()))
        .or_else(|| local_head(deps, tab, number))
        .or(cached_head);
    LoadedPr {
        overview,
        commits,
        head_oid,
        stack: None,
    }
}

/// `(owner, repo)` of a tab with no clone, which GitHub is asked by slug.
fn remote_parts(tab: &ResolvedTab) -> Option<(&str, &str)> {
    if !tab.identity().is_remote() {
        return None;
    }
    tab.remote_repo.as_deref()?.split_once('/')
}

fn fetch_overview(
    deps: &WorkspaceDeps,
    tab: &ResolvedTab,
    remote: Option<(&str, &str)>,
    number: u64,
) -> Option<PrOverviewData> {
    let github = &deps.ports().github;
    match remote {
        Some((owner, repo)) => github.overview_remote(owner, repo, number),
        None => github.overview(&tab.repo_root, Some(number)),
    }
    .ok()
    .flatten()
}

fn fetch_commits(
    deps: &WorkspaceDeps,
    tab: &ResolvedTab,
    remote: Option<(&str, &str)>,
    number: u64,
) -> Vec<CommitInfo> {
    let github = &deps.ports().github;
    match remote {
        Some((owner, repo)) => github.commits_remote(owner, repo, number, PR_COMMIT_CACHE_LIMIT),
        None => github.pr_commits(&tab.repo_root, number, PR_COMMIT_CACHE_LIMIT),
    }
    .unwrap_or_default()
}

/// The head of a local PR's fetched ref, when it was fetched before.
fn local_head(deps: &WorkspaceDeps, tab: &ResolvedTab, number: u64) -> Option<String> {
    if tab.identity().is_remote() {
        return None;
    }
    let ref_name = format!("refs/er/pr/{number}/head");
    er_engine::github::rev_parse_oid(&*deps.ports().git, &tab.repo_root, &ref_name)
}
