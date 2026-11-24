//! Opening a GitHub PR from the Open hub (P73): a stack row or the remote
//! URL input. The tab is listed in the registry and then loaded on its
//! actor, as the start opens `--remote`; a PR open already, in any tab kind
//! (`same_pr`), is selected instead. Nothing here waits on GitHub: the load
//! is posted, and the tab draws once it lands.

use er_engine::github;
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
    let registry = host.registry();
    let tabs = registry.tabs_view();
    let open = tabs
        .tabs
        .iter()
        .find(|entry| er_service::same_pr(&entry.descriptor, &start.tab));
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
    Ok(Opened::Listed(key, start))
}
