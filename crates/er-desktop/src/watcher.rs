//! The flag path's active-branch watcher and branch-base probe: what they
//! watch and what they ask for, read from the app registry's active tab and
//! that tab's content, so neither takes the App lock to choose.

use std::collections::HashMap;

use er_engine::checkout::{resolve_head_checkout, watched_branch};
use er_engine::git::Git;
use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_service::{Command, RemoteTips};

use crate::page_view::PrInfo;
use crate::service::{actor_key, ServiceState};

/// The registry's active tab, as its content was loaded and as its checkout
/// is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    pub key: TabKey,
    pub tab: TabDescriptor,
    /// The branch the tab shows, when it has a checkout.
    pub branch: Option<String>,
    /// Where that branch is checked out now.
    pub root: Option<String>,
    /// The checkout root the content was loaded with.
    pub loaded_root: Option<String>,
}

impl Watched {
    /// The branch and checkout root to watch, when the branch is checked out.
    pub fn target(&self) -> Option<(String, String)> {
        Some((self.branch.clone()?, self.root.clone()?))
    }

    /// Whether the checkout moved since the content loaded, so a reload
    /// would read another working tree.
    pub fn root_moved(&self) -> bool {
        self.root != self.loaded_root
    }
}

/// The registry's active tab once its content has loaded, with the checkout
/// root the load's own rule finds now.
pub fn watched(service: &ServiceState, git: &dyn Git) -> Option<Watched> {
    let view = service.app_registry().tabs_view();
    let entry = view.active_entry()?;
    let content = service.content(entry.key, &entry.descriptor)?;
    let tab = &content.tab;
    let branch = watched_branch(
        tab.identity().is_remote(),
        tab.local_branch_view.as_deref(),
        &tab.current_branch,
    );
    let root = branch
        .as_deref()
        .and_then(|branch| resolve_head_checkout(git, &tab.repo_root, branch));
    Some(Watched {
        key: entry.key,
        tab: entry.descriptor.clone(),
        branch,
        root,
        loaded_root: tab.checkout_root.clone(),
    })
}

/// What the watcher follows, turn by turn. A checkout that moved since the
/// load asks for one reload per move, so a branch checked out after the load
/// is followed, and a reload still on its way is not asked for again.
#[derive(Debug, Default)]
pub struct Follower {
    watched: Option<Watched>,
    reloaded_for: Option<(TabKey, Option<String>)>,
}

impl Follower {
    /// This turn's watched tab, with a reload posted when its checkout moved.
    pub fn next(&mut self, service: &ServiceState, git: &dyn Git) -> Option<Watched> {
        self.watched = watched(service, git);
        if let Some(moved) = self.watched.as_ref().filter(|w| w.root_moved()) {
            let asked = (moved.key, moved.root.clone());
            if self.reloaded_for.as_ref() != Some(&asked) {
                refresh(service, moved.key, &moved.tab);
                self.reloaded_for = Some(asked);
            }
        }
        self.watched.clone()
    }

    /// A file changed in the watched checkout: refreshes the tab this turn
    /// watched, whichever tab is active by now. Answers whether it posted.
    pub fn on_change(&self, service: &ServiceState) -> bool {
        self.watched
            .as_ref()
            .is_some_and(|w| refresh(service, w.key, &w.tab))
    }
}

/// Posts a refresh of `key`'s content that clears the reviewed marks of files
/// whose diff changed, as the App's watcher refresh does. Never waits: the
/// push brings the new diff and marks. Answers whether it posted.
pub fn refresh(service: &ServiceState, key: TabKey, tab: &TabDescriptor) -> bool {
    let refresh = Command::RefreshDiff {
        key,
        force: false,
        adopt_head_oid: None,
    };
    service.post_if_started(&actor_key(tab), refresh)
}

/// What the branch-base probe asks about for the registry's active tab:
/// origin's tip of the base its branch-vs-base view diffs against, and the
/// PR list's head for the PR its checkout has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTarget {
    pub key: TabKey,
    pub tab: TabDescriptor,
    pub repo_root: String,
    /// The base's short name, for a branch-vs-base view.
    pub base: Option<String>,
    /// The `owner/repo` whose PR list names the tab's PR; none for a remote
    /// tab, whose divergence nothing shows.
    pub remote: Option<String>,
    pub pr_number: Option<u64>,
    /// The branch the checkout follows, which finds a PR by its head when
    /// the tab has no number.
    pub branch: Option<String>,
    /// The tips the content holds now.
    pub tips: RemoteTips,
}

/// The registry's active tab once its content has loaded, as the probe sees
/// it. Never takes the App lock.
pub fn probe_target(service: &ServiceState) -> Option<ProbeTarget> {
    let view = service.app_registry().tabs_view();
    let entry = view.active_entry()?;
    let content = service.content(entry.key, &entry.descriptor)?;
    let tab = &content.tab;
    let is_remote = tab.identity().is_remote();
    let base = (!is_remote && tab.pr_number.is_none() && tab.mode == DiffMode::Branch)
        .then(|| tab.base_branch.trim_start_matches("origin/").to_string())
        .filter(|base| !base.is_empty());
    let slug = entry
        .descriptor
        .pr_owner
        .as_ref()
        .zip(entry.descriptor.pr_repo.as_ref());
    let remote = slug
        .map(|(owner, repo)| format!("{owner}/{repo}"))
        .or_else(|| {
            er_service::app_records::projects::load()
                .projects
                .into_iter()
                .find(|project| project.root_path == tab.repo_root)
                .and_then(|project| project.remote)
        });
    Some(ProbeTarget {
        key: entry.key,
        tab: entry.descriptor.clone(),
        repo_root: tab.repo_root.clone(),
        base,
        remote: remote.filter(|_| !is_remote),
        pr_number: tab.pr_number,
        branch: watched_branch(
            is_remote,
            tab.local_branch_view.as_deref(),
            &tab.current_branch,
        ),
        tips: content.freshness.value.tips.clone(),
    })
}

/// The PR list's head for `target`'s PR: by its number, else the PR whose
/// head is the checkout's branch.
pub fn pr_head(lists: &HashMap<String, Vec<PrInfo>>, target: &ProbeTarget) -> Option<String> {
    let list = lists.get(target.remote.as_deref()?)?;
    let pr = match (target.pr_number, target.branch.as_deref()) {
        (Some(number), _) => list.iter().find(|pr| pr.number == number),
        (None, Some(branch)) => list.iter().find(|pr| pr.head_ref == branch),
        (None, None) => None,
    };
    pr.map(|pr| pr.head_oid.clone())
        .filter(|oid| !oid.is_empty())
}

/// Origin's tip of `base`, read with `git ls-remote`. None when the read
/// fails or names nothing.
pub fn remote_tip(git: &dyn Git, repo_root: &str, base: &str) -> Option<String> {
    let stdout = git.ls_remote(repo_root, "origin", base).ok()?;
    let line = stdout.lines().next()?;
    line.split_whitespace()
        .next()
        .map(str::to_string)
        .filter(|oid| !oid.is_empty())
}

/// Posts `tips` to the target's actor, which measures the tab against them
/// again even when they held still: a local commit or fetch moves the counts
/// too. Never waits: the push brings the stale mark and the divergence.
/// Answers whether it posted.
pub fn post_tips(service: &ServiceState, target: &ProbeTarget, tips: RemoteTips) -> bool {
    let command = Command::SetRemoteTips {
        key: target.key,
        tips,
    };
    service.post_if_started(&actor_key(&target.tab), command)
}
