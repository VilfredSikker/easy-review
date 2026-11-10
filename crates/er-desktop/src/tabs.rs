//! Tab persistence — stores the open tab list as the app's `tabs` record in
//! the artifact store so the multi-tab layout survives app restart.
//!
//! A tab is reconstructed from a [`TabDescriptor`]: enough to recreate the
//! `TabState` via one of the engine's constructors (`new`, `new_local_branch`,
//! `new_remote`). Heavy state (diff content, AI sidecar files) is not persisted
//! — it's rederived on launch.

use anyhow::{Context, Result};
use er_store::{Artifact, Loaded, Mode};
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
    const MODE: Mode = Mode::Enveloped;
    const FILE: &'static str = "tabs.json";
    type Slot = ();
}

/// Register this record's kind beside the engine's kinds.
#[cfg(test)]
pub(crate) fn register(registry: &mut er_store::Registry) -> Result<(), er_store::StoreError> {
    registry.register::<TabsFile>()
}

/// Persist tab descriptors as the app's tabs record.
pub fn save_tabs(tabs: &[TabDescriptor], active_idx: usize) -> Result<()> {
    let key = er_engine::store_registry::app_key::<TabsFile>()?;
    let file = TabsFile {
        tabs: tabs.to_vec(),
        active_idx,
    };
    er_engine::store_registry::store().write(&key, &file)?;
    Ok(())
}

/// Serialize the live tab list and active index to disk.
pub fn save_app_tabs(app: &er_engine::app::App) -> Result<()> {
    let (descriptors, active) = persisted_tabs(app);
    save_tabs(&descriptors, active)
}

/// The tabs worth restoring, and the active one's index among them. The
/// placeholder has no repo to reopen, so it is left out.
fn persisted_tabs(app: &er_engine::app::App) -> (Vec<TabDescriptor>, usize) {
    let kept: Vec<(usize, &er_engine::app::TabState)> = app
        .tabs
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.placeholder)
        .collect();
    let active = kept
        .iter()
        .position(|(i, _)| *i == app.active_tab)
        .unwrap_or(kept.len().saturating_sub(1));
    let descriptors = kept
        .into_iter()
        .map(|(_, t)| descriptor_from_tab(t))
        .collect();
    (descriptors, active)
}

/// Best-effort [`save_app_tabs`]; logs a warning on failure.
pub fn persist_app_tabs(app: &er_engine::app::App) {
    if let Err(e) = save_app_tabs(app) {
        log::warn!("er-desktop: failed to persist tabs: {e}");
    }
}

/// Load persisted tabs. Returns `None` if the record is missing or unreadable —
/// callers fall back to the default single-tab launch.
pub fn load_tabs() -> Option<TabsFile> {
    let key = er_engine::store_registry::app_key::<TabsFile>().ok()?;
    match er_engine::store_registry::store().read(&key).ok()? {
        Loaded::Found(stored) => Some(stored.value),
        Loaded::Absent | Loaded::Unreadable(_) => None,
    }
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
    // Remote-only PR (GitHub slug, no local-branch view / clone).
    if tab.is_remote() {
        let slug = tab.remote_repo.as_deref().unwrap_or("");
        let mut parts = slug.splitn(2, '/');
        let owner = parts.next().unwrap_or("").to_string();
        let repo = parts.next().unwrap_or("").to_string();
        let branch = if tab.current_branch.is_empty() {
            None
        } else {
            Some(tab.current_branch.clone())
        };
        let base_ref = if tab.base_branch.is_empty() {
            None
        } else {
            Some(tab.base_branch.clone())
        };
        return TabDescriptor {
            kind: TabKind::RemotePr,
            repo_root: tab.repo_root.clone(),
            branch,
            pr_owner: Some(owner),
            pr_repo: Some(repo),
            pr_number: tab.pr_number,
            pr_head_ref: None,
            base_ref,
            browser_url,
            browser_layout,
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
            browser_url,
            browser_layout,
        };
    }
    // Plain local branch view
    if let Some(branch) = tab.local_branch_view.clone() {
        let base_ref = if tab.base_branch.is_empty() {
            None
        } else {
            Some(tab.base_branch.clone())
        };
        return TabDescriptor {
            kind: TabKind::LocalBranch,
            repo_root: tab.repo_root.clone(),
            branch: Some(branch),
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
            pr_head_ref: None,
            base_ref,
            browser_url,
            browser_layout,
        };
    }
    let base_ref = if tab.base_branch.is_empty() {
        None
    } else {
        Some(tab.base_branch.clone())
    };
    TabDescriptor {
        kind: TabKind::Working,
        repo_root: tab.repo_root.clone(),
        branch: None,
        pr_owner: None,
        pr_repo: None,
        pr_number: None,
        pr_head_ref: None,
        base_ref,
        browser_url,
        browser_layout,
    }
}

fn rebuild_local_branch(
    d: &TabDescriptor,
    lazy: bool,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState> {
    let branch = d
        .branch
        .clone()
        .context("local_branch descriptor missing branch")?;
    // Prefer the persisted (user-chosen) base; fall back to auto-detection.
    let base = match d.base_ref.as_deref() {
        Some(base) if !base.is_empty() => base.to_string(),
        _ => ports.git.detect_base_branch(&d.repo_root)?,
    };
    let mut tab =
        er_engine::app::TabState::new_with_base_unloaded(d.repo_root.clone(), base, ports.clone())?;
    tab.local_branch_view = Some(branch);
    tab.mode = er_engine::app::DiffMode::Branch;
    // Reloads AI sidecars so Review/Notes/Context have this tab's data on
    // first select, without waiting for the deferred git diff.
    tab.sync_managed_storage();
    if lazy {
        tab.needs_initial_refresh = true;
    } else {
        tab.refresh_diff()?;
    }
    Ok(tab)
}

fn rebuild_local_pr(
    d: &TabDescriptor,
    lazy: bool,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState> {
    let number = d
        .pr_number
        .context("local_pr descriptor missing pr_number")?;
    if !lazy {
        return er_engine::app::TabState::new_local_pr(d.repo_root.clone(), number, ports.clone());
    }
    let base = match d.base_ref.clone() {
        Some(b) if !b.is_empty() => b,
        _ => ports.git.detect_base_branch(&d.repo_root)?,
    };
    let mut tab =
        er_engine::app::TabState::new_with_base_unloaded(d.repo_root.clone(), base, ports.clone())?;
    tab.local_branch_view = d.branch.clone().or_else(|| Some(format!("pr/{}", number)));
    tab.pr_number = Some(number);
    tab.pr_head_ref = d.pr_head_ref.clone();
    // Match `new_stub_pr_tab`. Restored PR tabs open on PR Diff; Branch is a
    // click away once the head checkout is attached.
    tab.mode = er_engine::app::DiffMode::PrDiff;
    // Reloads AI sidecars so Review/Notes/Context have this tab's data on
    // first select, without waiting for the deferred git diff.
    tab.sync_managed_storage();
    tab.needs_initial_refresh = true;
    Ok(tab)
}

fn rebuild_remote_pr(
    d: &TabDescriptor,
    lazy: bool,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState> {
    let owner = d.pr_owner.clone().context("remote_pr missing owner")?;
    let repo = d.pr_repo.clone().context("remote_pr missing repo")?;
    let number = d.pr_number.context("remote_pr missing number")?;
    let pr_ref = er_engine::github::PrRef {
        owner,
        repo,
        number,
    };
    if lazy {
        let mut tab = er_engine::app::TabState::new_remote_stub(&pr_ref, ports.clone())?;
        if let Some(branch) = d.branch.clone().filter(|s| !s.is_empty()) {
            tab.current_branch = branch;
        }
        if let Some(base) = d.base_ref.clone().filter(|s| !s.is_empty()) {
            tab.base_branch = base;
        }
        Ok(tab)
    } else {
        er_engine::app::TabState::new_remote(&pr_ref, ports.clone())
    }
}

/// Rebuild a `TabState` from a descriptor. Skips work that needs the network
/// (e.g. PR overview fetch) — that's done lazily when the tab is focused.
///
/// When `lazy` is true, tabs are built without their initial diff load and get
/// `needs_initial_refresh = true`. The first diff runs when the tab gains focus
/// (or when the background warmer reaches a same-project stub).
pub fn rebuild_tab_with(
    d: &TabDescriptor,
    lazy: bool,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState> {
    let mut tab = match d.kind {
        TabKind::Working => {
            if lazy {
                let base = match d.base_ref.as_deref() {
                    Some(base) if !base.is_empty() => base.to_string(),
                    _ => ports.git.detect_base_branch(&d.repo_root)?,
                };
                let mut tab = er_engine::app::TabState::new_with_base_unloaded(
                    d.repo_root.clone(),
                    base,
                    ports.clone(),
                )?;
                tab.needs_initial_refresh = true;
                tab
            } else {
                er_engine::app::TabState::new(d.repo_root.clone(), ports.clone())?
            }
        }
        TabKind::LocalBranch => rebuild_local_branch(d, lazy, ports)?,
        TabKind::RemotePr => rebuild_remote_pr(d, lazy, ports)?,
        TabKind::LocalPr => rebuild_local_pr(d, lazy, ports)?,
    };
    apply_descriptor_browser(&mut tab, d);
    Ok(tab)
}

/// Backwards-compatible eager rebuild (legacy callers).
pub fn rebuild_tab(
    d: &TabDescriptor,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState> {
    rebuild_tab_with(d, false, ports)
}

/// Lazy rebuild: returns a stub without running the initial `refresh_diff()`.
/// The tab's `needs_initial_refresh` flag is set so a focus handler (or
/// background warmer) can run the diff later.
pub fn rebuild_tab_stub(
    d: &TabDescriptor,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState> {
    rebuild_tab_with(d, true, ports)
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
        er_engine::storage::with_test_storage_root(dir.path(), || {
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
        er_engine::storage::with_test_storage_root(dir.path(), || {
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
        // Isolate from the developer's global config. With `commit.gpgsign`
        // set globally this commit goes through a signing agent, and the
        // `status()` below is ignored and stderr nulled -- so a signing
        // failure leaves HEAD unborn and the test fails with "Failed to
        // determine current branch", which says nothing about the cause.
        std::process::Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(path)
            .status()
            .expect("git config gpgsign");
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
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            let tmp = tempfile::tempdir().expect("tempdir");
            init_git_repo(tmp.path());
            let root = tmp.path().to_string_lossy().to_string();
            let mut tab = er_engine::app::TabState::new_with_base_unloaded(
                root,
                "main".to_string(),
                er_engine::ports::Ports::cli(),
            )
            .expect("tab");
            tab.pr_number = Some(42);
            tab.local_branch_view = Some("dependabot/cargo-abc".to_string());
            let d = descriptor_from_tab(&tab);
            assert_eq!(d.kind, TabKind::LocalPr);
            assert_eq!(d.pr_number, Some(42));
            assert!(d.pr_head_ref.is_none());
            assert_eq!(d.branch.as_deref(), Some("dependabot/cargo-abc"));
        });
    }

    #[test]
    fn descriptor_local_pr_with_remote_slug_is_not_remote() {
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            let tmp = tempfile::tempdir().expect("tempdir");
            init_git_repo(tmp.path());
            let root = tmp.path().to_string_lossy().to_string();
            let mut tab = er_engine::app::TabState::new_with_base_unloaded(
                root,
                "origin/main".to_string(),
                er_engine::ports::Ports::cli(),
            )
            .expect("tab");
            tab.pr_number = Some(1425);
            tab.local_branch_view = Some("feat/delete-plates".to_string());
            tab.remote_repo = Some("reshapebiotech/discovery".to_string());
            tab.base_branch = "origin/main".to_string();
            let d = descriptor_from_tab(&tab);
            assert_eq!(d.kind, TabKind::LocalPr);
            assert_eq!(d.pr_number, Some(1425));
            assert_eq!(d.branch.as_deref(), Some("feat/delete-plates"));
            assert_eq!(d.base_ref.as_deref(), Some("origin/main"));
            assert!(d.pr_owner.is_none());
        });
    }

    #[test]
    fn descriptor_remote_pr_persists_head_and_base() {
        let mut tab = er_engine::app::TabState::new_for_test(vec![]);
        tab.remote_repo = Some("reshapebiotech/discovery".to_string());
        tab.pr_number = Some(1425);
        tab.current_branch = "feat/from-fork".to_string();
        tab.base_branch = "main".to_string();
        let d = descriptor_from_tab(&tab);
        assert_eq!(d.kind, TabKind::RemotePr);
        assert_eq!(d.pr_number, Some(1425));
        assert_eq!(d.pr_owner.as_deref(), Some("reshapebiotech"));
        assert_eq!(d.pr_repo.as_deref(), Some("discovery"));
        assert_eq!(d.branch.as_deref(), Some("feat/from-fork"));
        assert_eq!(d.base_ref.as_deref(), Some("main"));
    }

    #[test]
    fn rebuild_remote_pr_stub_restores_head_and_base() {
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            let d = TabDescriptor {
                kind: TabKind::RemotePr,
                repo_root: String::new(),
                branch: Some("feat/from-fork".to_string()),
                pr_owner: Some("reshapebiotech".to_string()),
                pr_repo: Some("discovery".to_string()),
                pr_number: Some(1425),
                pr_head_ref: None,
                base_ref: Some("main".to_string()),
                browser_url: None,
                browser_layout: None,
            };
            let tab =
                rebuild_remote_pr(&d, true, &er_engine::ports::Ports::for_test()).expect("stub");
            assert_eq!(tab.current_branch, "feat/from-fork");
            assert_eq!(tab.base_branch, "main");
            assert_eq!(tab.pr_number, Some(1425));
            assert!(tab.local_branch_view.is_none());
            assert!(tab.is_remote());
        });
    }

    #[test]
    fn lazy_local_pr_stub_loads_ai_sidecars() {
        let tmp = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            init_git_repo(tmp.path());
            let root = tmp.path().to_string_lossy().to_string();

            let mut probe = er_engine::app::TabState::new_with_base_unloaded(
                root.clone(),
                "main".to_string(),
                er_engine::ports::Ports::cli(),
            )
            .expect("probe");
            probe.pr_number = Some(99);
            probe.local_branch_view = Some("feat-ai-stub".to_string());
            probe.sync_managed_storage_light();
            let er = probe.er_dir();
            std::fs::create_dir_all(&er).expect("er dir");
            std::fs::write(
                std::path::Path::new(&er).join("questions.json"),
                r#"{"version":1,"diff_hash":"","questions":[{"id":"q-1","file":"a.rs","hunk_index":0,"line_start":1,"line_content":"c","text":"why","resolved":false}]}"#,
            )
            .expect("write questions");

            let d = TabDescriptor {
                kind: TabKind::LocalPr,
                repo_root: root,
                branch: Some("feat-ai-stub".to_string()),
                pr_owner: None,
                pr_repo: None,
                pr_number: Some(99),
                pr_head_ref: None,
                base_ref: Some("main".to_string()),
                browser_url: None,
                browser_layout: None,
            };
            let tab = rebuild_local_pr(&d, true, &er_engine::ports::Ports::cli()).expect("stub");
            assert_eq!(
                tab.mode,
                er_engine::app::DiffMode::PrDiff,
                "restored PR stubs open on PR Diff, matching new_stub_pr_tab"
            );
            assert!(tab.needs_initial_refresh, "lazy stub still defers git diff");
            let qs = tab
                .ai
                .questions
                .as_ref()
                .expect("questions sidecar loaded without refresh_diff");
            assert_eq!(qs.questions.len(), 1);
            assert_eq!(qs.questions[0].id, "q-1");
        });
    }

    #[test]
    fn save_app_tabs_clamps_active_index() {
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            let tmp = tempfile::tempdir().expect("tempdir");
            init_git_repo(tmp.path());
            let root = tmp.path().to_string_lossy().to_string();
            let mut app = er_engine::app::App::new_unloaded(root, er_engine::ports::Ports::cli())
                .expect("app");
            app.active_tab = 99;
            let (descriptors, active) = persisted_tabs(&app);
            assert_eq!(active, 0);
            assert_eq!(descriptors.len(), 1);
        });
    }

    #[test]
    fn placeholder_tab_is_not_persisted() {
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            // Saving it would make the next launch try to restore a tab with no repo.
            let app = er_engine::app::App::new_empty(er_engine::ports::Ports::for_test());
            let (descriptors, active) = persisted_tabs(&app);
            assert!(descriptors.is_empty());
            assert_eq!(active, 0);
        });
    }
}
