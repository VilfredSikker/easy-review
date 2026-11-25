//! Opening a tab from the Open hub: a GitHub PR from a stack row or the
//! remote URL input (P73), and a folder or worktree (P75.2). The tab is
//! listed in the registry and then loaded on its actor, as the start opens
//! one; a tab open already is selected instead: for a PR, in any tab kind
//! (`same_pr`); for a folder, any tab on the same repo root, as the App's
//! `open_in_new_tab`. Nothing here waits: the load is posted, and the tab
//! draws once it lands.

use er_engine::github;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_service::{AppCommand, Reply, ServiceHost};

use super::startup::{self, StartTab};

pub enum Opened {
    /// The PR was open already; its tab is now the active one.
    Selected(TabKey),
    /// A new remote PR tab, now active, still to load.
    Listed(TabKey, StartTab),
}

/// Lists `url`'s PR as the active tab, or selects the tab that holds it.
/// A URL that is not a PR's answers the App's message.
pub fn open(host: &ServiceHost, url: &str) -> Result<Opened, String> {
    let pr = github::parse_github_pr_url(url).ok_or("Invalid GitHub PR URL")?;
    let start = startup::remote_pr(pr);
    let same = |tab: &TabDescriptor| er_service::same_pr(tab, &start.tab);
    list_or_select(host, &start, same)
}

/// Lists a working tab on `repo_root` as the active tab, or selects a tab
/// already open on that root.
pub fn open_folder(host: &ServiceHost, repo_root: &str) -> Result<Opened, String> {
    let start = startup::working(repo_root.to_string());
    list_or_select(host, &start, |tab| tab.repo_root == repo_root)
}

fn list_or_select(
    host: &ServiceHost,
    start: &StartTab,
    same: impl Fn(&TabDescriptor) -> bool,
) -> Result<Opened, String> {
    let registry = host.registry();
    let tabs = registry.tabs_view();
    let open = tabs.tabs.iter().find(|entry| same(&entry.descriptor));
    if let Some(entry) = open {
        let key = entry.key;
        registry
            .send(AppCommand::SelectTab { key })
            .map_err(|error| error.to_string())?;
        return Ok(Opened::Selected(key));
    }
    let tab = start.tab.clone();
    let reply = registry
        .send(AppCommand::NewTab { tab })
        .map_err(|error| error.to_string())?;
    let Reply::Tabs(view) = reply else {
        return Err(format!("a new tab answered {reply:?}"));
    };
    let key = view.active.ok_or("The new tab did not become active")?;
    Ok(Opened::Listed(key, start.clone()))
}
