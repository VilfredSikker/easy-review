//! The flag path's active-branch watcher: what it watches and what it asks
//! for, read from the app registry's active tab and that tab's content, so
//! the watcher never takes the App lock to choose.

use er_engine::checkout::{resolve_head_checkout, watched_branch};
use er_engine::git::Git;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_service::Command;

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
