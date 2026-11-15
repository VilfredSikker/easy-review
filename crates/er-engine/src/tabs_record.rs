//! Tab persistence — stores the open tab list as the app's `tabs` record in
//! the artifact store so the multi-tab layout survives app restart.
//!
//! A tab is reconstructed from a [`TabDescriptor`]: enough to recreate the
//! `TabState` via one of the engine's constructors (`new`, `new_local_branch`,
//! `new_remote`). Heavy state (diff content, AI sidecar files) is not persisted
//! — it's rederived on launch.
//!
//! The record lives in the engine's always-compiled half so the desktop's
//! flag-off path and the service's app registry save the same file.

use anyhow::Result;
use er_store::{Artifact, Loaded};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    Working,
    LocalBranch,
    RemotePr,
    /// Local clone + fetched PR ref; never runs `gh pr checkout`.
    LocalPr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
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
    /// For `LocalPr` tabs: the fetched local ref (`refs/er/pr/<n>/head`).
    #[serde(default)]
    pub pr_head_ref: Option<String>,
    /// For `LocalPr` tabs: the resolved base ref used for the diff.
    #[serde(default)]
    pub base_ref: Option<String>,
    /// Last browser URL for this tab (desktop).
    #[serde(default)]
    pub browser_url: Option<String>,
    /// Last browser layout: hidden | split | fullscreen (desktop).
    #[serde(default)]
    pub browser_layout: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct TabsFile {
    pub tabs: Vec<TabDescriptor>,
    #[serde(default)]
    pub active_idx: usize,
}

impl Artifact for TabsFile {
    const KIND: &'static str = "tabs";
    const VERSION: u32 = 1;
    const FILE: &'static str = "tabs.json";
    type Slot = ();
}

/// Register this record's kind beside the engine's kinds.
pub(crate) fn register(registry: &mut er_store::Registry) -> Result<(), er_store::StoreError> {
    registry.register::<TabsFile>()
}

/// Persist tab descriptors as the app's tabs record.
pub fn save_tabs(tabs: &[TabDescriptor], active_idx: usize) -> Result<()> {
    let key = crate::store_registry::app_key::<TabsFile>()?;
    let file = TabsFile {
        tabs: tabs.to_vec(),
        active_idx,
    };
    crate::store_registry::store().write(&key, &file)?;
    Ok(())
}

/// Load persisted tabs. Returns `None` if the record is missing or unreadable —
/// callers fall back to the default single-tab launch.
pub fn load_tabs() -> Option<TabsFile> {
    let key = crate::store_registry::app_key::<TabsFile>().ok()?;
    match crate::store_registry::store().read(&key).ok()? {
        Loaded::Found(stored) => Some(stored.value),
        Loaded::Absent | Loaded::Unreadable(_) => None,
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
                pr_head_ref: None,
                base_ref: None,
                browser_url: None,
                browser_layout: None,
            },
            TabDescriptor {
                kind: TabKind::LocalBranch,
                repo_root: "/tmp/repo-a".to_string(),
                branch: Some("feat/x".to_string()),
                pr_owner: None,
                pr_repo: None,
                pr_number: None,
                pr_head_ref: None,
                base_ref: None,
                browser_url: None,
                browser_layout: None,
            },
            TabDescriptor {
                kind: TabKind::RemotePr,
                repo_root: String::new(),
                branch: None,
                pr_owner: Some("octo".to_string()),
                pr_repo: Some("cat".to_string()),
                pr_number: Some(42),
                pr_head_ref: None,
                base_ref: None,
                browser_url: None,
                browser_layout: None,
            },
            TabDescriptor {
                kind: TabKind::LocalPr,
                repo_root: "/tmp/repo-b".to_string(),
                branch: Some("feat/no-checkout".to_string()),
                pr_owner: None,
                pr_repo: None,
                pr_number: Some(1110),
                pr_head_ref: Some("refs/er/pr/1110/head".to_string()),
                base_ref: Some("origin/main".to_string()),
                browser_url: None,
                browser_layout: None,
            },
        ];

        let dir = tempfile::tempdir().expect("tempdir");
        crate::storage::with_test_storage_root(dir.path(), || {
            save_tabs(&tabs, 1).expect("save");

            let loaded = load_tabs().expect("a saved record");

            assert_eq!(loaded.tabs.len(), 4);
            assert_eq!(loaded.tabs, tabs);
            assert_eq!(loaded.active_idx, 1);
        });
    }

    #[test]
    fn tabs_round_trip_through_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::storage::with_test_storage_root(dir.path(), || {
            let tabs = vec![TabDescriptor {
                kind: TabKind::RemotePr,
                repo_root: String::new(),
                branch: None,
                pr_owner: Some("octo".to_string()),
                pr_repo: Some("cat".to_string()),
                pr_number: Some(42),
                pr_head_ref: None,
                base_ref: None,
                browser_url: None,
                browser_layout: None,
            }];
            save_tabs(&tabs, 0).expect("save");

            let loaded = load_tabs().expect("a saved record");
            assert_eq!(loaded.tabs, tabs);
            assert_eq!(loaded.active_idx, 0);
        });
    }

    /// P46 keeps each tab's browser URL and layout in `tabs.json`. A record
    /// written before the move loads and saves back with the same payload, and
    /// descriptors carrying both fields come back equal.
    #[test]
    fn the_tabs_record_round_trips_a_pre_p46_file() {
        let payload = serde_json::json!({
            "tabs": [{
                "kind": "remote_pr",
                "repo_root": "",
                "branch": "feat/browser",
                "pr_owner": "octo",
                "pr_repo": "cat",
                "pr_number": 42,
                "pr_head_ref": null,
                "base_ref": "main",
                "browser_url": "http://localhost:5173/review",
                "browser_layout": "split"
            }],
            "active_idx": 0
        });
        let dir = tempfile::tempdir().expect("tempdir");
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = crate::store_registry::app_path::<TabsFile>().expect("tabs path");
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            let record = serde_json::json!({
                "kind": "tabs", "version": 1, "diff_hash": null, "producer": "host",
                "written_at": "2026-11-14T20:28:49Z", "payload": payload,
            });
            std::fs::write(&path, record.to_string()).expect("write record");

            let file = load_tabs().expect("the pre-P46 record loads");
            save_tabs(&file.tabs, file.active_idx).expect("save");
            let saved: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
            assert_eq!(saved["payload"], payload, "the saved payload changed");
            assert_eq!(load_tabs().expect("reload").tabs, file.tabs);
        });
    }
}
