//! The app owner: the one writer of the sidebar's resources.
//!
//! It publishes `projects`, each project's `branches`, each remote's `prs`,
//! each open root's `worktrees` and the `inbox` from the projects record, the
//! metadata cache, the PR cache, the tick's worktree listing and the inbox
//! state, and
//! stamps a key's version only when the value it builds differs from the one
//! it last published, so any row field that moves moves the version. It is
//! also the one writer of the metadata cache and of the PR cache: every
//! refresh, sweep and patch runs through it and publishes after, and so does
//! every inbox command and every write of the projects record. Changes it
//! does not make (another process writing the projects record, the GitHub
//! login arriving after launch) reach the resources at the next publish,
//! which the backend tick runs.
//!
//! The owner never names the App. The desktop's resource host reads it through
//! [`SidebarOwner`], which is why the move into the service changed neither the
//! host's reads nor the frontend.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use er_engine::config::InboxConfig;
use er_protocol::{
    Branch, Branches, CheckRun, GithubStatus, Inbox, InboxItem, InboxTarget, PrRow, PrStatus,
    Project, ProjectPrs, Projects, Prs, RepoMerge, ResourceChanged, ResourceKey, ResourceVersion,
    StatusComment, StatusReview, StatusReviewer, Worktree, Worktrees,
};

use crate::app_records::inbox::{InboxHandle, InboxState};
use crate::app_records::pr_cache::{self, PrCacheFetchedAtMap, PrCacheMap};
use crate::app_records::projects::{
    self, ProjectRecord, ProjectReviewSettingsPatch, ProjectsFile, TabProjectRef,
};
use crate::app_records::sidebar::{
    self, BranchInfo, GhStatusCache, GhUser, GithubStatusSnapshot, MetaCache, PrInfo, ProjectMeta,
    WorktreeSnapshot,
};
use er_engine::git::Git;
use er_engine::github::GitHub;

#[cfg(test)]
mod tests;

/// What the resource host reads from the sidebar's owner.
pub trait SidebarOwner: Send + Sync {
    /// The projects at the version last stamped; empty at version 0 before
    /// the first publish.
    fn projects(&self) -> (u64, Projects);
    /// One project's branches at their version. A project the owner has not
    /// published reads empty at version 0, so a stale key in a batched fetch
    /// does not fail the keys beside it.
    fn branches(&self, project: &str) -> (u64, Branches);
    /// One remote's pull requests at their version; a remote the owner has
    /// not published reads empty at version 0.
    fn prs(&self, remote: &str) -> (u64, Prs);
    /// The worktrees of one open tab's root at their version; a root the
    /// tick has not listed reads empty at version 0.
    fn worktrees(&self, root: &str) -> (u64, Worktrees);
    /// The inbox at its version; empty at version 0 until the owner reads
    /// the inbox prefs.
    fn inbox(&self) -> (u64, Inbox);
    /// One PR's GitHub status at its version, by `owner/repo` and number; a
    /// PR the status cache holds nothing for reads none at version 0.
    fn pr_status(&self, remote: &str, number: u64) -> (u64, PrStatus);
    /// Every key the owner has published, with its version.
    fn versions(&self) -> Vec<(ResourceKey, u64)>;
}

pub struct AppOwner {
    meta: MetaCache,
    pr_cache: PrCacheMap,
    pr_fetched_at: PrCacheFetchedAtMap,
    gh_user: GhUser,
    /// The worktree list of each open tab's root, as the tick last listed it.
    worktrees: Mutex<BTreeMap<String, Worktrees>>,
    inbox: InboxHandle,
    /// Where the inbox's kind filter comes from. Set on the flag path only:
    /// the default build's snapshot reads the App's prefs, and no reader of
    /// `inbox` exists there.
    inbox_prefs: OnceLock<InboxPrefs>,
    /// Where `pr_status` comes from. Set on the flag path only, as
    /// `inbox_prefs` is.
    gh_status: OnceLock<GhStatusCache>,
    published: Mutex<Published>,
    /// Wakes the resource relay after a publish that moved a version.
    wake: Mutex<Option<Sender<()>>>,
    /// Hears each PR-list fetch and sweep, with its kind and fields.
    change_hook: OnceLock<Box<ChangeHook>>,
}

/// What [`AppOwner::set_change_hook`] takes: a kind and its fields.
pub type ChangeHook = dyn Fn(&str, &[(&str, String)]) + Send + Sync;

#[derive(Default)]
struct Published {
    /// The last version stamped on any key. Every key draws from this one
    /// counter, so a project removed and added again under its id never
    /// repeats a version a reader already holds.
    stamp: u64,
    projects: Option<Stamped<Projects>>,
    branches: BTreeMap<String, Stamped<Branches>>,
    prs: BTreeMap<String, Stamped<Prs>>,
    worktrees: BTreeMap<String, Stamped<Worktrees>>,
    inbox: Option<Stamped<Inbox>>,
    pr_status: BTreeMap<(String, u64), Stamped<PrStatus>>,
}

type InboxPrefs = Box<dyn Fn() -> Option<InboxConfig> + Send + Sync>;

struct Stamped<T> {
    version: u64,
    value: T,
}

impl AppOwner {
    /// The owner over the desktop's metadata cache, PR cache, GitHub login
    /// and inbox.
    pub fn new(
        meta: MetaCache,
        pr_cache: PrCacheMap,
        pr_fetched_at: PrCacheFetchedAtMap,
        gh_user: GhUser,
        inbox: InboxHandle,
    ) -> Self {
        Self {
            meta,
            pr_cache,
            pr_fetched_at,
            gh_user,
            worktrees: Mutex::default(),
            inbox,
            inbox_prefs: OnceLock::new(),
            gh_status: OnceLock::new(),
            published: Mutex::default(),
            wake: Mutex::default(),
            change_hook: OnceLock::new(),
        }
    }

    pub fn set_wake(&self, wake: Sender<()>) {
        *lock(&self.wake) = Some(wake);
    }

    /// Has `hook` hear each PR-list fetch and sweep the owner runs, with its
    /// kind and fields; the desktop logs them. The first call wins.
    pub fn set_change_hook(&self, hook: impl Fn(&str, &[(&str, String)]) + Send + Sync + 'static) {
        let _ = self.change_hook.set(Box::new(hook));
    }

    fn change_hook(&self) -> &ChangeHook {
        fn unheard(_: &str, _: &[(&str, String)]) {}
        self.change_hook
            .get()
            .map_or(&unheard, |hook| hook.as_ref())
    }

    /// Builds `inbox` from here on, with the kind filter `prefs` reads at
    /// each publish; while it answers none the held inbox stays as it is.
    /// The first call wins.
    pub fn read_inbox_prefs_from(
        &self,
        prefs: impl Fn() -> Option<InboxConfig> + Send + Sync + 'static,
    ) {
        let _ = self.inbox_prefs.set(Box::new(prefs));
    }

    /// Builds `pr_status` from `cache` from here on. A fetch that stores a
    /// status bumps the desktop revision, which wakes the tick that
    /// publishes. The first call wins.
    pub fn read_status_from(&self, cache: GhStatusCache) {
        let _ = self.gh_status.set(cache);
    }

    /// Applies one write to the inbox and publishes, then saves the inbox.
    pub fn write_inbox(&self, write: impl FnOnce(&mut InboxState)) -> Result<(), String> {
        {
            let mut inbox = self.inbox.lock().map_err(|e| e.to_string())?;
            write(&mut inbox);
        }
        self.publish();
        crate::app_records::inbox::save_inbox_state(&self.inbox);
        Ok(())
    }

    /// Saves the inbox as a sweep left it. The tick publishes.
    pub fn save_inbox(&self) {
        crate::app_records::inbox::save_inbox_state(&self.inbox);
    }

    /// Runs `refresh` over the metadata cache, then publishes. Answers what
    /// `refresh` answers: whether the cache's fingerprint moved.
    pub fn refresh_meta_with(&self, refresh: impl FnOnce(&MetaCache) -> bool) -> bool {
        let moved = refresh(&self.meta);
        self.publish();
        moved
    }

    /// Sets one cached PR's head commit, as the head probe found it, then
    /// publishes. Answers whether the commit changed.
    pub fn patch_pr_head_oid(&self, remote: &str, number: u64, head_oid: &str) -> bool {
        let changed = pr_cache::patch_pr_head_oid(&self.pr_cache, remote, number, head_oid);
        if changed {
            self.publish();
        }
        changed
    }

    /// Fetches the PRs of every project remote with a checkout, then
    /// publishes. Answers the remotes whose fetch failed.
    pub fn refresh_prs(&self, github: Arc<dyn er_engine::github::GitHub>) -> Vec<String> {
        let hook = self.change_hook();
        let failed = pr_cache::refresh_pr_cache(github, &self.pr_cache, &self.pr_fetched_at, hook);
        self.publish();
        failed
    }

    /// Fetches one remote's PRs, then publishes. Answers whether it fetched.
    pub fn refresh_remote_prs(
        &self,
        github: Arc<dyn er_engine::github::GitHub>,
        remote: &str,
    ) -> bool {
        let fetched = pr_cache::refresh_pr_cache_for_remote(
            github,
            remote,
            &self.pr_cache,
            &self.pr_fetched_at,
            self.change_hook(),
        );
        self.publish();
        fetched
    }

    /// Caches one PR fetched on its own, then publishes.
    pub fn cache_pr(&self, remote: &str, pr: PrInfo) {
        pr_cache::cache_pr(&self.pr_cache, &self.pr_fetched_at, remote, pr);
        self.publish();
    }

    /// Drops every cached remote with `remote`'s slug, then publishes.
    pub fn forget_remote_prs(&self, remote: &str) {
        pr_cache::forget_remote(&self.pr_cache, &self.pr_fetched_at, remote);
        self.publish();
    }

    /// Replaces the PR cache with the persisted one, then publishes.
    pub fn restore_prs(&self, restored: pr_cache::PersistedPrCache) {
        let (prs, fetched_at) = restored;
        *lock(&self.pr_cache) = prs;
        *lock(&self.pr_fetched_at) = fetched_at;
        self.publish();
    }

    /// Holds `root`'s worktree list, as the tick listed it for the active
    /// tab, and drops the lists of roots no open tab shows. The tick
    /// publishes next.
    pub fn hold_worktrees(&self, root: &str, worktrees: Worktrees, open_roots: &BTreeSet<String>) {
        let mut held = lock(&self.worktrees);
        held.retain(|held_root, _| held_root == root || open_roots.contains(held_root));
        held.insert(root.to_string(), worktrees);
    }

    /// Builds every sidebar resource from its sources and stamps a new
    /// version on each one whose value changed. Answers the keys it moved.
    pub fn publish(&self) -> Vec<ResourceChanged> {
        let changed = {
            // Held while building, so two publishes cannot stamp out of order.
            let mut published = lock(&self.published);
            let file = projects::load();
            let metas = lock(&self.meta).clone();
            let cache = lock(&self.pr_cache).clone();
            let fetched_at = lock(&self.pr_fetched_at).clone();
            let me = lock(&self.gh_user).clone();
            let branches = file
                .projects
                .iter()
                .map(|project| {
                    let prs = project.remote.as_ref().and_then(|remote| cache.get(remote));
                    let value = build_branches(
                        project,
                        metas.get(&project.id),
                        prs.map(Vec::as_slice),
                        me.as_deref(),
                    );
                    (project.id.clone(), value)
                })
                .collect();
            let prs = build_prs(&file, &cache, &fetched_at, me.as_deref());
            let worktrees = lock(&self.worktrees).clone();
            let inbox = self
                .inbox_prefs
                .get()
                .and_then(|prefs| prefs())
                .map(|prefs| build_inbox(&lock(&self.inbox), &prefs));
            let statuses = self
                .gh_status
                .get()
                .map(|cache| build_statuses(&lock(cache), me.as_deref()));
            let mut changed =
                published.replace(build_projects(&file), branches, prs, worktrees, inbox);
            if let Some(statuses) = statuses {
                published.replace_statuses(statuses, &mut changed);
            }
            changed
        };
        if !changed.is_empty() {
            if let Some(wake) = lock(&self.wake).as_ref() {
                let _ = wake.send(());
            }
        }
        changed
    }
}

/// The projects record's writes. Each runs through `projects.rs`, under the
/// store's scope lock, then publishes, so the owner is the record's one
/// writer in its process.
impl AppOwner {
    /// Registers the repository holding `path`; see [`projects::auto_register`].
    pub fn auto_register(
        &self,
        github: &dyn GitHub,
        git: &dyn Git,
        path: &str,
    ) -> Option<ProjectRecord> {
        self.wrote(projects::auto_register(github, git, path))
    }

    /// Registers every root and remote `refs` names.
    pub fn sync_projects_from_refs(
        &self,
        github: &dyn GitHub,
        git: &dyn Git,
        refs: &[TabProjectRef],
    ) {
        projects::sync_projects_from_refs(github, git, refs);
        self.publish();
    }

    pub fn ensure_remote_project(&self, remote: &str) -> anyhow::Result<String> {
        self.wrote(projects::ensure_remote_project(remote))
    }

    pub fn delete_project(&self, project: &str) -> anyhow::Result<()> {
        self.wrote(projects::delete_project(project))
    }

    pub fn reorder_projects(&self, ordered: &[String]) -> anyhow::Result<()> {
        self.wrote(projects::reorder_projects(ordered))
    }

    pub fn set_active_project(&self, project: &str) {
        projects::set_active(project);
        self.publish();
    }

    pub fn patch_project_review_settings(
        &self,
        project: &str,
        patch: ProjectReviewSettingsPatch,
    ) -> anyhow::Result<()> {
        self.wrote(projects::patch_project_review_settings(project, patch))
    }

    pub fn add_tracked_branch(&self, project: &str, name: &str) -> anyhow::Result<()> {
        self.wrote(projects::add_tracked_branch(project, name))
    }

    pub fn remove_tracked_branch(&self, project: &str, name: &str) -> anyhow::Result<()> {
        self.wrote(projects::remove_tracked_branch(project, name))
    }

    pub fn dismiss_pr(&self, project: &str, number: u64) {
        projects::dismiss_pr(project, number);
        self.publish();
    }

    pub fn undismiss_pr(&self, project: &str, number: u64) -> anyhow::Result<()> {
        self.wrote(projects::undismiss_pr(project, number))
    }

    pub fn track_pr(&self, project: &str, number: u64) -> anyhow::Result<()> {
        self.wrote(projects::track_pr(project, number))
    }

    pub fn untrack_pr(&self, project: &str, number: u64) -> anyhow::Result<()> {
        self.wrote(projects::untrack_pr(project, number))
    }

    pub fn save_pr(&self, project: &str, number: u64, title: &str) -> anyhow::Result<()> {
        self.wrote(projects::save_pr(project, number, title))
    }

    pub fn unsave_pr(&self, project: &str, number: u64) -> anyhow::Result<()> {
        self.wrote(projects::unsave_pr(project, number))
    }

    /// Publishes after a write and answers what the write answered.
    fn wrote<T>(&self, answer: T) -> T {
        self.publish();
        answer
    }
}

impl SidebarOwner for AppOwner {
    fn projects(&self) -> (u64, Projects) {
        lock(&self.published)
            .projects
            .as_ref()
            .map(|stamped| (stamped.version, stamped.value.clone()))
            .unwrap_or_default()
    }

    fn branches(&self, project: &str) -> (u64, Branches) {
        lock(&self.published)
            .branches
            .get(project)
            .map(|stamped| (stamped.version, stamped.value.clone()))
            .unwrap_or_default()
    }

    fn prs(&self, remote: &str) -> (u64, Prs) {
        lock(&self.published)
            .prs
            .get(remote)
            .map(|stamped| (stamped.version, stamped.value.clone()))
            .unwrap_or_default()
    }

    fn worktrees(&self, root: &str) -> (u64, Worktrees) {
        lock(&self.published)
            .worktrees
            .get(root)
            .map(|stamped| (stamped.version, stamped.value.clone()))
            .unwrap_or_default()
    }

    fn inbox(&self) -> (u64, Inbox) {
        lock(&self.published)
            .inbox
            .as_ref()
            .map(|stamped| (stamped.version, stamped.value.clone()))
            .unwrap_or_default()
    }

    fn pr_status(&self, remote: &str, number: u64) -> (u64, PrStatus) {
        lock(&self.published)
            .pr_status
            .get(&(remote.to_string(), number))
            .map(|stamped| (stamped.version, stamped.value.clone()))
            .unwrap_or_default()
    }

    fn versions(&self) -> Vec<(ResourceKey, u64)> {
        let published = lock(&self.published);
        let statuses = published
            .pr_status
            .iter()
            .map(|((remote, number), stamped)| {
                let key = ResourceKey::PrStatus {
                    remote: remote.clone(),
                    number: *number,
                };
                (key, stamped.version)
            });
        let projects = published
            .projects
            .as_ref()
            .map(|stamped| (ResourceKey::Projects, stamped.version));
        let branches = published.branches.iter().map(|(project, stamped)| {
            let key = ResourceKey::Branches {
                project: project.clone(),
            };
            (key, stamped.version)
        });
        let prs = published.prs.iter().map(|(remote, stamped)| {
            let key = ResourceKey::Prs {
                remote: remote.clone(),
            };
            (key, stamped.version)
        });
        let worktrees = published.worktrees.iter().map(|(root, stamped)| {
            let key = ResourceKey::Worktrees { root: root.clone() };
            (key, stamped.version)
        });
        let inbox = published
            .inbox
            .as_ref()
            .map(|stamped| (ResourceKey::Inbox, stamped.version));
        projects
            .into_iter()
            .chain(branches)
            .chain(prs)
            .chain(worktrees)
            .chain(inbox)
            .chain(statuses)
            .collect()
    }
}

impl Published {
    /// Replaces each value that differs from the one held, under a new
    /// version, and drops the keys of projects and remotes that left. No
    /// inbox leaves the held one as it is.
    fn replace(
        &mut self,
        projects: Projects,
        branches: BTreeMap<String, Branches>,
        prs: BTreeMap<String, Prs>,
        worktrees: BTreeMap<String, Worktrees>,
        inbox: Option<Inbox>,
    ) -> Vec<ResourceChanged> {
        let mut changed = Vec::new();
        if self.projects.as_ref().map(|held| &held.value) != Some(&projects) {
            self.stamp += 1;
            self.projects = Some(Stamped {
                version: self.stamp,
                value: projects,
            });
            changed.push(moved(ResourceKey::Projects, self.stamp));
        }
        let branches_key = |project| ResourceKey::Branches { project };
        replace_keyed(
            &mut self.stamp,
            &mut self.branches,
            branches,
            branches_key,
            &mut changed,
        );
        let prs_key = |remote| ResourceKey::Prs { remote };
        replace_keyed(&mut self.stamp, &mut self.prs, prs, prs_key, &mut changed);
        let worktrees_key = |root| ResourceKey::Worktrees { root };
        replace_keyed(
            &mut self.stamp,
            &mut self.worktrees,
            worktrees,
            worktrees_key,
            &mut changed,
        );
        if let Some(inbox) = inbox {
            if self.inbox.as_ref().map(|held| &held.value) != Some(&inbox) {
                self.stamp += 1;
                self.inbox = Some(Stamped {
                    version: self.stamp,
                    value: inbox,
                });
                changed.push(moved(ResourceKey::Inbox, self.stamp));
            }
        }
        changed
    }
}

impl Published {
    /// Replaces each status that differs from the one held, under a new
    /// version, and drops the PRs the cache no longer holds.
    fn replace_statuses(
        &mut self,
        built: BTreeMap<(String, u64), PrStatus>,
        changed: &mut Vec<ResourceChanged>,
    ) {
        self.pr_status.retain(|key, _| built.contains_key(key));
        for ((remote, number), value) in built {
            let key = (remote, number);
            if self.pr_status.get(&key).map(|held| &held.value) == Some(&value) {
                continue;
            }
            self.stamp += 1;
            let version = self.stamp;
            let (remote, number) = key.clone();
            self.pr_status.insert(key, Stamped { version, value });
            changed.push(moved(ResourceKey::PrStatus { remote, number }, version));
        }
    }
}

/// Each cached status under its `owner/repo` and number. `me` decides
/// `is_authored_by_me` as the snapshot does: while no login is known the
/// stored flag stands.
fn build_statuses(
    cache: &HashMap<(String, String, u64), GithubStatusSnapshot>,
    me: Option<&str>,
) -> BTreeMap<(String, u64), PrStatus> {
    cache
        .iter()
        .map(|((owner, repo, number), status)| {
            let status = PrStatus {
                status: Some(github_status(status, me)),
                fetching: false,
            };
            ((format!("{owner}/{repo}"), *number), status)
        })
        .collect()
}

/// The wire twin of a cached status.
pub fn github_status(status: &GithubStatusSnapshot, me: Option<&str>) -> GithubStatus {
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    GithubStatus {
        owner: status.owner.clone(),
        repo: status.repo.clone(),
        number: status.number,
        url: status.url.clone(),
        state: status.state.clone(),
        is_draft: status.is_draft,
        title: status.title.clone(),
        body: status.body.clone(),
        author: status.author.clone(),
        head_ref: status.head_ref.clone(),
        base_ref: status.base_ref.clone(),
        review_decision: status.review_decision.clone(),
        mergeable: status.mergeable.clone(),
        labels: status.labels.clone(),
        checks: status
            .checks
            .iter()
            .map(|check| CheckRun {
                name: check.name.clone(),
                status: check.status.clone(),
                conclusion: check.conclusion.clone(),
                url: check.url.clone(),
                started_at: check.started_at.clone(),
                completed_at: check.completed_at.clone(),
                duration_secs: check.duration_secs,
                description: check.description.clone(),
            })
            .collect(),
        comments_count: count(status.comments_count),
        reviews_count: count(status.reviews_count),
        recent_comments: status
            .recent_comments
            .iter()
            .map(|c| StatusComment {
                author: c.author.clone(),
                body: c.body.clone(),
                created_at: c.created_at.clone(),
                url: c.url.clone(),
            })
            .collect(),
        recent_reviews: status
            .recent_reviews
            .iter()
            .map(|r| StatusReview {
                author: r.author.clone(),
                state: r.state.clone(),
                body: r.body.clone(),
                submitted_at: r.submitted_at.clone(),
            })
            .collect(),
        last_updated: status.last_updated.clone(),
        is_authored_by_me: me.map_or(status.is_authored_by_me, |login| {
            status.author.eq_ignore_ascii_case(login)
        }),
        merge_state_status: status.merge_state_status.clone(),
        head_oid: status.head_oid.clone(),
        is_cross_repository: status.is_cross_repository,
        auto_merge_method: status.auto_merge_method.clone(),
        head_branch_exists: status.head_branch_exists,
        repo_merge: status.repo_merge.as_ref().map(|m| RepoMerge {
            merge_commit_allowed: m.merge_commit_allowed,
            squash_merge_allowed: m.squash_merge_allowed,
            rebase_merge_allowed: m.rebase_merge_allowed,
            delete_branch_on_merge: m.delete_branch_on_merge,
            auto_merge_allowed: m.auto_merge_allowed,
            viewer_permission: m.viewer_permission.clone(),
        }),
        in_merge_queue: status.in_merge_queue,
        base_has_merge_queue: status.base_has_merge_queue,
        can_bypass_rules: status.can_bypass_rules,
        created_at: status.created_at.clone(),
        commit_count: count(status.commit_count),
        reviewers: status
            .reviewers
            .iter()
            .map(|r| StatusReviewer {
                reviewer: r.reviewer.clone(),
                is_team: r.is_team,
                requested: r.requested,
                state: r.state.clone(),
            })
            .collect(),
    }
}

/// `replace` for one keyed family: every built value that differs from the
/// one held takes the next version.
fn replace_keyed<T: PartialEq>(
    stamp: &mut u64,
    held: &mut BTreeMap<String, Stamped<T>>,
    built: BTreeMap<String, T>,
    key: impl Fn(String) -> ResourceKey,
    changed: &mut Vec<ResourceChanged>,
) {
    held.retain(|id, _| built.contains_key(id));
    for (id, value) in built {
        if held.get(&id).map(|stamped| &stamped.value) == Some(&value) {
            continue;
        }
        *stamp += 1;
        let version = *stamp;
        held.insert(id.clone(), Stamped { version, value });
        changed.push(moved(key(id), version));
    }
}

fn moved(key: ResourceKey, version: u64) -> ResourceChanged {
    ResourceChanged {
        key,
        version: ResourceVersion(version),
    }
}

/// The project rows, in the record's order.
pub fn build_projects(file: &ProjectsFile) -> Projects {
    Projects {
        projects: file
            .projects
            .iter()
            .map(|project| Project {
                id: project.id.clone(),
                name: project.name.clone(),
                root_path: project.root_path.clone(),
                remote: project.remote.clone(),
                remote_only: remote_only(project),
                auto_triage_max_diff_kb: project.auto_triage_max_diff_kb,
                review_ignore_globs: project.review_ignore_globs.clone(),
            })
            .collect(),
    }
}

/// One project's branches from its metadata, each named with the open pull
/// request whose head it is. A project known only by its remote has none.
pub fn build_branches(
    project: &ProjectRecord,
    meta: Option<&ProjectMeta>,
    prs: Option<&[PrInfo]>,
    me: Option<&str>,
) -> Branches {
    let Some(meta) = meta.filter(|_| !remote_only(project)) else {
        return Branches::default();
    };
    let (mine, to_review) = listed_open_prs(prs.unwrap_or_default(), &project.dismissed_prs, me);
    let open: Vec<&PrInfo> = mine.into_iter().chain(to_review).collect();
    let wire = |branches: &[BranchInfo]| -> Vec<Branch> {
        branches
            .iter()
            .map(|branch| Branch {
                name: branch.name.clone(),
                upstream: branch.upstream.clone(),
                is_merged: branch.is_merged,
                worktree_path: branch.worktree_path.clone(),
                pr_number: open
                    .iter()
                    .find(|pr| pr.head_ref == branch.name)
                    .map(|pr| pr.number),
            })
            .collect()
    };
    Branches {
        current_branch: meta.current_branch.clone(),
        local_branches: wire(&meta.local_branches),
        auto_branches: wire(&meta.auto_branches),
    }
}

/// The open pull requests the sidebar lists as mine and as to review: the
/// ones a branch row takes its number from, mine first.
fn listed_open_prs<'a>(
    prs: &'a [PrInfo],
    dismissed: &[u64],
    me: Option<&str>,
) -> (Vec<&'a PrInfo>, Vec<&'a PrInfo>) {
    let open: Vec<&PrInfo> = prs
        .iter()
        .filter(|pr| pr.state == "OPEN" && !dismissed.contains(&pr.number))
        .collect();
    let reviewed_by_me = |pr: &PrInfo, states: &[&str]| {
        me.is_some_and(|login| {
            pr.latest_reviewer_states
                .iter()
                .any(|(reviewer, state)| reviewer == login && states.contains(&state.as_str()))
        })
    };
    // With no login the cached flag stands, as the snapshot leaves it.
    let approved_by_me = |pr: &PrInfo| match me {
        Some(_) => reviewed_by_me(pr, &["APPROVED"]),
        None => pr.approved_by_me,
    };
    let mine = open
        .iter()
        .filter(|pr| me.is_some_and(|login| pr.author == login));
    let to_review = open.iter().filter(|pr| {
        !pr.is_draft
            && me.is_none_or(|login| pr.author != login)
            && !approved_by_me(pr)
            && !reviewed_by_me(pr, &["APPROVED", "CHANGES_REQUESTED"])
    });
    (mine.copied().collect(), to_review.copied().collect())
}

/// Each remote's pull requests: every remote the cache holds or a project
/// names, keyed by the exact string the record and the cache use.
pub fn build_prs(
    file: &ProjectsFile,
    cache: &HashMap<String, Vec<PrInfo>>,
    fetched_at: &HashMap<String, u64>,
    me: Option<&str>,
) -> BTreeMap<String, Prs> {
    let named = file.projects.iter().filter_map(|p| p.remote.as_ref());
    let remotes: BTreeSet<&String> = cache.keys().chain(named).collect();
    remotes
        .into_iter()
        .map(|remote| {
            let cached = cache.get(remote).map(Vec::as_slice).unwrap_or_default();
            let mut rows: Vec<PrRow> = cached.iter().map(|pr| pr_row(pr, me)).collect();
            let projects = file
                .projects
                .iter()
                .filter(|project| project.remote.as_ref() == Some(remote))
                .map(|project| project_prs(project, cached, me, &mut rows))
                .collect();
            let prs = Prs {
                rows,
                fetched_at_ms: fetched_at.get(remote).copied(),
                projects,
            };
            (remote.clone(), prs)
        })
        .collect()
}

/// One project's lists over its remote's cached rows, with its dismissed
/// numbers left out of mine, to review and recently merged. A project known
/// only by its remote lists only what it saved and opened.
fn project_prs(
    project: &ProjectRecord,
    cached: &[PrInfo],
    me: Option<&str>,
    rows: &mut Vec<PrRow>,
) -> ProjectPrs {
    let numbers = |prs: Vec<&PrInfo>| prs.iter().map(|pr| pr.number).collect();
    let dismissed = &project.dismissed_prs;
    let (mine, to_review, recently_merged) = if remote_only(project) {
        Default::default()
    } else {
        let (mine, to_review) = listed_open_prs(cached, dismissed, me);
        let shown = cached.iter().filter(|pr| !dismissed.contains(&pr.number));
        let merged = sidebar::recently_merged_prs(shown.cloned().collect());
        (
            numbers(mine),
            numbers(to_review),
            merged.iter().map(|pr| pr.number).collect(),
        )
    };
    let mut saved: Vec<_> = project.saved_prs.iter().collect();
    saved.sort_by_key(|entry| Reverse(entry.saved_at_ms));
    let saved = saved
        .into_iter()
        .filter_map(|entry| entry_row(entry.number, &entry.title, cached, rows))
        .collect();
    let mut recent: Vec<_> = project.recent_prs.iter().collect();
    recent.sort_by_key(|entry| Reverse(entry.viewed_at_ms));
    let recent = recent
        .into_iter()
        .filter_map(|entry| entry_row(entry.number, &entry.title, cached, rows))
        .collect();
    ProjectPrs {
        project: project.id.clone(),
        mine,
        to_review,
        recently_merged,
        saved,
        recent,
        dismissed: dismissed.clone(),
    }
}

/// A saved or recent entry's number, once a row holds it: the cached row, or
/// a title-only one added for an entry the cache lacks. An entry with
/// neither a row nor a title is left out, as the snapshot leaves it out.
fn entry_row(number: u64, title: &str, cached: &[PrInfo], rows: &mut Vec<PrRow>) -> Option<u64> {
    if cached.iter().any(|pr| pr.number == number) {
        return Some(number);
    }
    if title.is_empty() {
        return None;
    }
    if !rows.iter().any(|row| row.number == number) {
        rows.push(pr_row(&sidebar::minimal_pr_info(number, title), None));
    }
    Some(number)
}

/// The wire row for a cached PR. With a login, whether it approved is read
/// from the latest reviews, as the snapshot reads it; without one the
/// cached flag stands.
pub fn pr_row(pr: &PrInfo, me: Option<&str>) -> PrRow {
    let approved_by_me = match me {
        Some(login) => pr
            .latest_reviewer_states
            .iter()
            .any(|(reviewer, state)| reviewer == login && state == "APPROVED"),
        None => pr.approved_by_me,
    };
    PrRow {
        number: pr.number,
        title: pr.title.clone(),
        head_ref: pr.head_ref.clone(),
        base_ref: pr.base_ref.clone(),
        head_oid: pr.head_oid.clone(),
        state: pr.state.clone(),
        is_draft: pr.is_draft,
        author: pr.author.clone(),
        assignees: pr.assignees.clone(),
        reviewers: pr.reviewers.clone(),
        checks_state: pr.checks_state.clone(),
        review_decision: pr.review_decision.clone(),
        merged_at: pr.merged_at.clone(),
        updated_at: pr.updated_at.clone(),
        approved_by_me,
    }
}

/// The inbox as the snapshot shows it: the kinds `prefs` hides are left out
/// of the items and of the unread count.
pub fn build_inbox(state: &InboxState, prefs: &InboxConfig) -> Inbox {
    let items: Vec<InboxItem> = state
        .items
        .iter()
        .filter(|item| prefs.shows(&item.kind))
        .map(|item| InboxItem {
            id: item.id.clone(),
            kind: item.kind.clone(),
            category: crate::app_records::inbox::category_for_kind(&item.kind)
                .as_str()
                .to_string(),
            severity: item.severity.clone(),
            title: item.title.clone(),
            body: item.body.clone(),
            source: item.source.clone(),
            target: InboxTarget {
                project_id: item.target.project_id.clone(),
                repo_root: item.target.repo_root.clone(),
                remote: item.target.remote.clone(),
                pr_number: item.target.pr_number,
                branch: item.target.branch.clone(),
                url: item.target.url.clone(),
            },
            created_at_ms: item.created_at_ms,
            read_at_ms: item.read_at_ms,
            dedupe_key: item.dedupe_key.clone(),
        })
        .collect();
    let unread = items
        .iter()
        .filter(|item| item.read_at_ms.is_none())
        .count();
    Inbox {
        items,
        unread_count: u64::try_from(unread).unwrap_or(u64::MAX),
        last_refresh_ms: state.last_refresh_ms,
    }
}

/// The wire form of the snapshot's worktree rows, without which one is
/// current.
pub fn worktree_rows(rows: Vec<WorktreeSnapshot>) -> Worktrees {
    Worktrees {
        worktrees: rows
            .into_iter()
            .map(|row| Worktree {
                path: row.path,
                branch: row.branch,
                is_pr: row.is_pr,
                pr_number: row.pr_number,
                is_merged: row.is_merged,
                remote: row.remote,
            })
            .collect(),
    }
}

fn remote_only(project: &ProjectRecord) -> bool {
    project.root_path.is_empty() && project.remote.is_some()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
