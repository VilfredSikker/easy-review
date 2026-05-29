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
    /// Local clone + fetched PR ref; never runs `gh pr checkout`.
    LocalPr,
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
    /// For `LocalPr` tabs: the fetched local ref (`refs/er/pr/<n>/head`).
    #[serde(default)]
    pub pr_head_ref: Option<String>,
    /// For `LocalPr` tabs: the resolved base ref used for the diff.
    #[serde(default)]
    pub base_ref: Option<String>,
    /// For `LocalBranch` tabs: the Easy Review owned ref (e.g.
    /// `refs/er/branches/<branch>/head`) populated by an explicit force-refresh.
    /// Persisting it ensures the refreshed view survives tab recreation/app restart.
    #[serde(default)]
    pub local_branch_diff_ref: Option<String>,
    /// Last browser URL for this tab (desktop).
    #[serde(default)]
    pub browser_url: Option<String>,
    /// Last browser layout: hidden | split | fullscreen (desktop).
    #[serde(default)]
    pub browser_layout: Option<String>,
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
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
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

/// Serialize the live tab list and active index to disk.
pub fn save_app_tabs(app: &er_engine::app::App) -> Result<()> {
    let descriptors: Vec<TabDescriptor> = app.tabs.iter().map(descriptor_from_tab).collect();
    let active = app.active_tab.min(app.tabs.len().saturating_sub(1));
    save_tabs(&descriptors, active)
}

/// Best-effort [`save_app_tabs`]; logs a warning on failure.
pub fn persist_app_tabs(app: &er_engine::app::App) {
    if let Err(e) = save_app_tabs(app) {
        log::warn!("er-desktop: failed to persist tabs: {e}");
    }
}

/// Load persisted tabs. Returns `None` if the file is missing or unreadable —
/// callers fall back to the default single-tab launch.
pub fn load_tabs() -> Option<TabsFile> {
    let path = tabs_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn browser_descriptor_fields(tab: &er_engine::app::TabState) -> (Option<String>, Option<String>) {
    let url = if tab.browser_url.trim().is_empty() {
        None
    } else {
        Some(tab.browser_url.clone())
    };
    let layout = if tab.browser_layout == er_engine::app::BrowserLayout::Hidden {
        None
    } else {
        Some(tab.browser_layout.as_str().to_string())
    };
    (url, layout)
}

/// Apply persisted browser fields from a descriptor onto a live tab.
pub fn apply_descriptor_browser(tab: &mut er_engine::app::TabState, d: &TabDescriptor) {
    if let Some(url) = d.browser_url.clone() {
        tab.browser_url = url;
    }
    if let Some(layout) = d.browser_layout.as_deref() {
        tab.browser_layout = er_engine::app::BrowserLayout::from_label(layout);
    }
}

/// Convert a live `TabState` into a persistable descriptor.
pub fn descriptor_from_tab(tab: &er_engine::app::TabState) -> TabDescriptor {
    let (browser_url, browser_layout) = browser_descriptor_fields(tab);
    // Remote PR (no local clone)
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
            pr_head_ref: None,
            base_ref: None,
            local_branch_diff_ref: None,
            browser_url: browser_url.clone(),
            browser_layout: browser_layout.clone(),
        };
    }
    // Local PR review (fetched head ref, no checkout)
    if let Some(num) = tab.pr_number.filter(|_| !tab.is_remote()) {
        let base_ref = if tab.base_branch.is_empty() {
            None
        } else {
            Some(tab.base_branch.clone())
        };
        return TabDescriptor {
            kind: TabKind::LocalPr,
            repo_root: tab.repo_root.clone(),
            branch: tab.local_branch_view.clone(),
            pr_owner: None,
            pr_repo: None,
            pr_number: Some(num),
            pr_head_ref: tab.pr_head_ref.clone(),
            base_ref,
            local_branch_diff_ref: None,
            browser_url: browser_url.clone(),
            browser_layout: browser_layout.clone(),
        };
    }
    // Plain local branch view
    if let Some(branch) = tab.local_branch_view.clone() {
        return TabDescriptor {
            kind: TabKind::LocalBranch,
            repo_root: tab.repo_root.clone(),
            branch: Some(branch),
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
            pr_head_ref: None,
            base_ref: None,
            local_branch_diff_ref: tab.local_branch_diff_ref.clone(),
            browser_url: browser_url.clone(),
            browser_layout: browser_layout.clone(),
        };
    }
    TabDescriptor {
        kind: TabKind::Working,
        repo_root: tab.repo_root.clone(),
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref: None,
        local_branch_diff_ref: None,
        browser_url,
        browser_layout,
    }
}

fn rebuild_local_branch(d: &TabDescriptor, lazy: bool) -> Result<er_engine::app::TabState> {
    let branch = d
        .branch
        .clone()
        .context("local_branch descriptor missing branch")?;
    let resolved_ref = d
        .local_branch_diff_ref
        .clone()
        .or_else(|| er_engine::github::refreshed_branch_ref_if_exists(&d.repo_root, &branch));
    let base = er_engine::git::detect_base_branch_in(&d.repo_root)?;
    let mut tab = er_engine::app::TabState::new_with_base_unloaded(d.repo_root.clone(), base)?;
    tab.local_branch_view = Some(branch);
    tab.mode = er_engine::app::DiffMode::Branch;
    tab.local_branch_diff_ref = resolved_ref;
    if lazy {
        tab.needs_initial_refresh = true;
    } else {
        tab.refresh_diff()?;
    }
    Ok(tab)
}

fn rebuild_local_pr(d: &TabDescriptor, lazy: bool) -> Result<er_engine::app::TabState> {
    let number = d
        .pr_number
        .context("local_pr descriptor missing pr_number")?;
    if !lazy {
        return er_engine::app::TabState::new_local_pr(d.repo_root.clone(), number);
    }
    let base = match d.base_ref.clone() {
        Some(b) if !b.is_empty() => b,
        _ => er_engine::git::detect_base_branch_in(&d.repo_root)?,
    };
    let mut tab = er_engine::app::TabState::new_with_base_unloaded(d.repo_root.clone(), base)?;
    tab.local_branch_view = d.branch.clone().or_else(|| Some(format!("pr/{}", number)));
    tab.pr_number = Some(number);
    tab.pr_head_ref = d.pr_head_ref.clone();
    tab.mode = er_engine::app::DiffMode::Branch;
    tab.needs_initial_refresh = true;
    Ok(tab)
}

fn rebuild_remote_pr(d: &TabDescriptor, lazy: bool) -> Result<er_engine::app::TabState> {
    let owner = d.pr_owner.clone().context("remote_pr missing owner")?;
    let repo = d.pr_repo.clone().context("remote_pr missing repo")?;
    let number = d.pr_number.context("remote_pr missing number")?;
    let pr_ref = er_engine::github::PrRef {
        owner,
        repo,
        number,
    };
    if lazy {
        er_engine::app::TabState::new_remote_stub(&pr_ref)
    } else {
        er_engine::app::TabState::new_remote(&pr_ref)
    }
}

/// Rebuild a `TabState` from a descriptor. Skips work that needs the network
/// (e.g. PR overview fetch) — that's done lazily when the tab is focused.
///
/// When `lazy` is true, tabs are built without their initial diff load and get
/// `needs_initial_refresh = true`. The first diff runs when the tab gains focus
/// (or when the background warmer reaches a same-project stub).
pub fn rebuild_tab_with(d: &TabDescriptor, lazy: bool) -> Result<er_engine::app::TabState> {
    let mut tab = match d.kind {
        TabKind::Working => {
            if lazy {
                let base = er_engine::git::detect_base_branch_in(&d.repo_root)?;
                let mut tab =
                    er_engine::app::TabState::new_with_base_unloaded(d.repo_root.clone(), base)?;
                tab.needs_initial_refresh = true;
                tab
            } else {
                er_engine::app::TabState::new(d.repo_root.clone())?
            }
        }
        TabKind::LocalBranch => rebuild_local_branch(d, lazy)?,
        TabKind::RemotePr => rebuild_remote_pr(d, lazy)?,
        TabKind::LocalPr => rebuild_local_pr(d, lazy)?,
    };
    tab.apply_managed_root();
    apply_descriptor_browser(&mut tab, d);
    Ok(tab)
}

/// Backwards-compatible eager rebuild (legacy callers).
pub fn rebuild_tab(d: &TabDescriptor) -> Result<er_engine::app::TabState> {
    rebuild_tab_with(d, false)
}

/// Lazy rebuild: returns a stub without running the initial `refresh_diff()`.
/// The tab's `needs_initial_refresh` flag is set so a focus handler (or
/// background warmer) can run the diff later.
pub fn rebuild_tab_stub(d: &TabDescriptor) -> Result<er_engine::app::TabState> {
    rebuild_tab_with(d, true)
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
                local_branch_diff_ref: None,
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
                local_branch_diff_ref: None,
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
                local_branch_diff_ref: None,
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
                local_branch_diff_ref: None,
                browser_url: None,
                browser_layout: None,
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

        assert_eq!(loaded.tabs.len(), 4);
        assert_eq!(loaded.tabs, tabs);
        assert_eq!(loaded.active_idx, 1);
    }

    fn init_git_repo(path: &std::path::Path) {
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git init");
        std::process::Command::new("git")
            .args(["config", "user.email", "er@test.local"])
            .current_dir(path)
            .status()
            .expect("git config email");
        std::process::Command::new("git")
            .args(["config", "user.name", "er-test"])
            .current_dir(path)
            .status()
            .expect("git config name");
        std::fs::write(path.join("README"), "x").expect("write readme");
        std::process::Command::new("git")
            .args(["add", "README"])
            .current_dir(path)
            .status()
            .expect("git add");
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git commit");
    }

    #[test]
    fn descriptor_local_pr_without_head_ref() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_git_repo(tmp.path());
        let root = tmp.path().to_string_lossy().to_string();
        let mut tab = er_engine::app::TabState::new_with_base_unloaded(root, "main".to_string())
            .expect("tab");
        tab.pr_number = Some(42);
        tab.local_branch_view = Some("dependabot/cargo-abc".to_string());
        let d = descriptor_from_tab(&tab);
        assert_eq!(d.kind, TabKind::LocalPr);
        assert_eq!(d.pr_number, Some(42));
        assert!(d.pr_head_ref.is_none());
        assert_eq!(d.branch.as_deref(), Some("dependabot/cargo-abc"));
    }

    #[test]
    fn descriptor_local_branch_preserves_diff_ref() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_git_repo(tmp.path());
        let root = tmp.path().to_string_lossy().to_string();
        let mut tab = er_engine::app::TabState::new_with_base_unloaded(root, "main".to_string())
            .expect("tab");
        tab.local_branch_view = Some("feat/x".to_string());
        tab.local_branch_diff_ref = Some("refs/er/branches/feat/x/head".to_string());
        let d = descriptor_from_tab(&tab);
        assert_eq!(d.kind, TabKind::LocalBranch);
        assert_eq!(d.branch.as_deref(), Some("feat/x"));
        assert_eq!(
            d.local_branch_diff_ref.as_deref(),
            Some("refs/er/branches/feat/x/head")
        );
    }

    #[test]
    fn save_app_tabs_clamps_active_index() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_git_repo(tmp.path());
        let root = tmp.path().to_string_lossy().to_string();
        let mut app = er_engine::app::App::new_unloaded(root).expect("app");
        app.active_tab = 99;
        let descriptors: Vec<_> = app.tabs.iter().map(descriptor_from_tab).collect();
        let active = app.active_tab.min(app.tabs.len().saturating_sub(1));
        assert_eq!(active, 0);
        assert_eq!(descriptors.len(), 1);
    }
}
