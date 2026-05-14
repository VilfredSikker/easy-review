//! Tab persistence — serializes the open tab list to `~/.config/er/tabs.json`
//! so the multi-tab layout survives app restart.
//!
//! A tab is reconstructed from a [`TabDescriptor`]: enough to recreate the
//! `TabState` via one of the engine's constructors (`new`, `new_local_branch`,
//! `new_remote`). Heavy state (diff content, AI sidecar files) is not persisted
//! — it's rederived on launch.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    Working,
    LocalBranch,
    RemotePr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TabDescriptor {
    pub kind: TabKind,
    pub repo_root: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pr_owner: Option<String>,
    #[serde(default)]
    pub pr_repo: Option<String>,
    #[serde(default)]
    pub pr_number: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TabsFile {
    pub tabs: Vec<TabDescriptor>,
    #[serde(default)]
    pub active_idx: usize,
}

/// Location of the persisted tab file. `~/.config/er/tabs.json` on Linux/mac,
/// platform equivalent elsewhere via the `dirs` crate.
fn tabs_path() -> Option<PathBuf> {
    let dir = dirs::config_dir()?.join("er");
    Some(dir.join("tabs.json"))
}

/// Persist tab descriptors to `~/.config/er/tabs.json`. Writes atomically via
/// tmp file + rename so a crash mid-save never produces a truncated file.
pub fn save_tabs(tabs: &[TabDescriptor], active_idx: usize) -> Result<()> {
    let path = tabs_path().context("no config dir")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let file = TabsFile {
        tabs: tabs.to_vec(),
        active_idx,
    };
    let json = serde_json::to_string_pretty(&file)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}

/// Load persisted tabs. Returns `None` if the file is missing or unreadable —
/// callers fall back to the default single-tab launch.
pub fn load_tabs() -> Option<TabsFile> {
    let path = tabs_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Convert a live `TabState` into a persistable descriptor.
pub fn descriptor_from_tab(tab: &er_engine::app::TabState) -> TabDescriptor {
    if let (Some(slug), Some(num)) = (tab.remote_repo.as_ref(), tab.pr_number) {
        let mut parts = slug.splitn(2, '/');
        let owner = parts.next().unwrap_or("").to_string();
        let repo = parts.next().unwrap_or("").to_string();
        return TabDescriptor {
            kind: TabKind::RemotePr,
            repo_root: tab.repo_root.clone(),
            branch: None,
            pr_owner: Some(owner),
            pr_repo: Some(repo),
            pr_number: Some(num),
        };
    }
    if let Some(branch) = tab.local_branch_view.clone() {
        return TabDescriptor {
            kind: TabKind::LocalBranch,
            repo_root: tab.repo_root.clone(),
            branch: Some(branch),
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
        };
    }
    TabDescriptor {
        kind: TabKind::Working,
        repo_root: tab.repo_root.clone(),
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
    }
}

/// Rebuild a `TabState` from a descriptor. Skips work that needs the network
/// (e.g. PR overview fetch) — that's done lazily when the tab is focused.
pub fn rebuild_tab(d: &TabDescriptor) -> Result<er_engine::app::TabState> {
    match d.kind {
        TabKind::Working => er_engine::app::TabState::new(d.repo_root.clone()),
        TabKind::LocalBranch => {
            let branch = d
                .branch
                .clone()
                .context("local_branch descriptor missing branch")?;
            er_engine::app::TabState::new_local_branch(d.repo_root.clone(), branch)
        }
        TabKind::RemotePr => {
            let owner = d.pr_owner.clone().context("remote_pr missing owner")?;
            let repo = d.pr_repo.clone().context("remote_pr missing repo")?;
            let number = d.pr_number.context("remote_pr missing number")?;
            let pr_ref = er_engine::github::PrRef { owner, repo, number };
            er_engine::app::TabState::new_remote(&pr_ref)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a few descriptors through `save_tabs` / `load_tabs`.
    ///
    /// We can't easily redirect `tabs_path()` for tests without adding a
    /// `dirs` shim, so we exercise the serde plumbing directly via a temp
    /// file — proves the wire format is stable.
    #[test]
    fn round_trip_serializes_and_deserializes() {
        let tabs = vec![
            TabDescriptor {
                kind: TabKind::Working,
                repo_root: "/tmp/repo-a".to_string(),
                branch: None,
                pr_owner: None,
                pr_repo: None,
                pr_number: None,
            },
            TabDescriptor {
                kind: TabKind::LocalBranch,
                repo_root: "/tmp/repo-a".to_string(),
                branch: Some("feat/x".to_string()),
                pr_owner: None,
                pr_repo: None,
                pr_number: None,
            },
            TabDescriptor {
                kind: TabKind::RemotePr,
                repo_root: String::new(),
                branch: None,
                pr_owner: Some("octo".to_string()),
                pr_repo: Some("cat".to_string()),
                pr_number: Some(42),
            },
        ];
        let file = TabsFile {
            tabs: tabs.clone(),
            active_idx: 1,
        };

        let path = std::env::temp_dir().join(format!(
            "er-tabs-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let json = serde_json::to_string_pretty(&file).expect("serialize");
        std::fs::write(&path, &json).expect("write");

        let loaded_raw = std::fs::read_to_string(&path).expect("read");
        let loaded: TabsFile = serde_json::from_str(&loaded_raw).expect("deserialize");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.tabs.len(), 3);
        assert_eq!(loaded.tabs, tabs);
        assert_eq!(loaded.active_idx, 1);
    }
}
