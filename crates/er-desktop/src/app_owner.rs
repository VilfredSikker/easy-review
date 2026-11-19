//! The desktop app owner: the one writer of the sidebar's resources.
//!
//! It publishes `projects` and each project's `branches` from the projects
//! record, the metadata cache and the PR cache, and stamps a key's version
//! only when the value it builds differs from the one it last published. It
//! is also the metadata cache's one writer: every refresh runs through it and
//! publishes after. Changes it does not make (a PR-cache write, another
//! process writing the projects record, the GitHub login arriving after
//! launch) reach the resources at the next publish, which the backend tick
//! runs.
//!
//! The owner never names the App. The resource host reads it through
//! [`SidebarOwner`], so moving the owner into the service changes neither the
//! host nor the frontend.

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard, PoisonError};

use er_protocol::{
    Branch, Branches, Project, Projects, ResourceChanged, ResourceKey, ResourceVersion,
};

use crate::pr_cache::PrCacheMap;
use crate::projects::{self, ProjectRecord, ProjectsFile};
use crate::snapshot::{self, BranchInfo, GhUser, MetaCache, PrInfo, ProjectMeta};

pub mod startup;

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
    /// Every key the owner has published, with its version.
    fn versions(&self) -> Vec<(ResourceKey, u64)>;
}

pub struct AppOwner {
    meta: MetaCache,
    pr_cache: PrCacheMap,
    gh_user: GhUser,
    published: Mutex<Published>,
    /// Wakes the resource relay after a publish that moved a version.
    wake: Mutex<Option<Sender<()>>>,
}

#[derive(Default)]
struct Published {
    /// The last version stamped on any key. Every key draws from this one
    /// counter, so a project removed and added again under its id never
    /// repeats a version a reader already holds.
    stamp: u64,
    projects: Option<Stamped<Projects>>,
    branches: BTreeMap<String, Stamped<Branches>>,
}

struct Stamped<T> {
    version: u64,
    value: T,
}

impl AppOwner {
    /// The owner over the desktop's metadata cache, PR cache and GitHub login.
    pub fn new(meta: MetaCache, pr_cache: PrCacheMap, gh_user: GhUser) -> Self {
        Self {
            meta,
            pr_cache,
            gh_user,
            published: Mutex::default(),
            wake: Mutex::default(),
        }
    }

    pub fn set_wake(&self, wake: Sender<()>) {
        *lock(&self.wake) = Some(wake);
    }

    /// Refreshes every project's metadata, then publishes. Answers whether
    /// the cache's fingerprint moved, which the snapshot's revision follows.
    pub fn refresh_meta(&self, git: &dyn er_engine::git::Git, active_root: &str) -> bool {
        let moved = snapshot::refresh_meta_cache(git, active_root, &self.meta);
        self.publish();
        moved
    }

    /// Refreshes one project's metadata, then publishes.
    pub fn refresh_project_meta(&self, git: &dyn er_engine::git::Git, project_id: &str) -> bool {
        let moved = snapshot::refresh_meta_cache_for_project(git, project_id, &self.meta);
        self.publish();
        moved
    }

    /// Builds every sidebar resource from its sources and stamps a new
    /// version on each one whose value changed. Answers the keys it moved.
    pub fn publish(&self) -> Vec<ResourceChanged> {
        let changed = {
            // Held while building, so two publishes cannot stamp out of order.
            let mut published = lock(&self.published);
            let file = projects::load();
            let metas = lock(&self.meta).clone();
            let prs = remote_prs(&file, &lock(&self.pr_cache));
            let me = lock(&self.gh_user).clone();
            let branches = file
                .projects
                .iter()
                .map(|project| {
                    let prs = project.remote.as_ref().and_then(|remote| prs.get(remote));
                    let value = build_branches(
                        project,
                        metas.get(&project.id),
                        prs.map(Vec::as_slice),
                        me.as_deref(),
                    );
                    (project.id.clone(), value)
                })
                .collect();
            published.replace(build_projects(&file), branches)
        };
        if !changed.is_empty() {
            if let Some(wake) = lock(&self.wake).as_ref() {
                let _ = wake.send(());
            }
        }
        changed
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

    fn versions(&self) -> Vec<(ResourceKey, u64)> {
        let published = lock(&self.published);
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
        projects.into_iter().chain(branches).collect()
    }
}

impl Published {
    /// Replaces each value that differs from the one held, under a new
    /// version, and drops the branches of projects that left.
    fn replace(
        &mut self,
        projects: Projects,
        branches: BTreeMap<String, Branches>,
    ) -> Vec<ResourceChanged> {
        let mut changed = Vec::new();
        if self.projects.as_ref().map(|held| &held.value) != Some(&projects) {
            let version = self.next();
            self.projects = Some(Stamped {
                version,
                value: projects,
            });
            changed.push(moved(ResourceKey::Projects, version));
        }
        self.branches
            .retain(|project, _| branches.contains_key(project));
        for (project, value) in branches {
            if self.branches.get(&project).map(|held| &held.value) == Some(&value) {
                continue;
            }
            let version = self.next();
            self.branches
                .insert(project.clone(), Stamped { version, value });
            changed.push(moved(ResourceKey::Branches { project }, version));
        }
        changed
    }

    fn next(&mut self) -> u64 {
        self.stamp += 1;
        self.stamp
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
    let open = listed_open_prs(prs.unwrap_or_default(), &project.dismissed_prs, me);
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

/// The open pull requests the sidebar lists as mine or to review, mine first:
/// the ones a branch row takes its number from.
fn listed_open_prs<'a>(prs: &'a [PrInfo], dismissed: &[u64], me: Option<&str>) -> Vec<&'a PrInfo> {
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
    mine.chain(to_review).copied().collect()
}

fn remote_only(project: &ProjectRecord) -> bool {
    project.root_path.is_empty() && project.remote.is_some()
}

/// The cached pull requests of each project's remote.
fn remote_prs(
    file: &ProjectsFile,
    cache: &HashMap<String, Vec<PrInfo>>,
) -> HashMap<String, Vec<PrInfo>> {
    file.projects
        .iter()
        .filter_map(|project| project.remote.as_ref())
        .filter_map(|remote| Some((remote.clone(), cache.get(remote)?.clone())))
        .collect()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
