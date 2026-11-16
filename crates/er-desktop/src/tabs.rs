//! Tab persistence — stores the open tab list as the app's `tabs` record in
//! the artifact store so the multi-tab layout survives app restart.
//!
//! A tab is reconstructed from a [`TabDescriptor`]: enough to recreate the
//! `TabState` via one of the engine's constructors (`new`, `new_local_branch`,
//! `new_remote`). Heavy state (diff content, AI sidecar files) is not persisted
//! — it's rederived on launch.

use anyhow::{Context, Result};
pub use er_engine::tabs_record::{load_tabs, save_tabs, TabDescriptor, TabKind, TabsFile};

use crate::view_state::{BrowserLayout, DesktopViewState, TabBrowserState, ViewStateHandle};

#[cfg(feature = "v1-service")]
mod follow;
#[cfg(feature = "v1-service")]
pub use follow::{follow, follow_registry, restore};

/// Serialize the live tab list and active index to disk. Each tab's browser URL
/// and layout come from the desktop view state.
pub fn save_app_tabs(app: &er_engine::app::App, view: &DesktopViewState) -> Result<()> {
    let (descriptors, active) = persisted_tabs(app, view);
    save_tabs(&descriptors, active)
}

/// The tabs worth restoring, and the active one's index among them. The
/// placeholder has no repo to reopen, so it is left out.
fn persisted_tabs(
    app: &er_engine::app::App,
    view: &DesktopViewState,
) -> (Vec<TabDescriptor>, usize) {
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
        .map(|(_, t)| descriptor_from_tab(t, view))
        .collect();
    (descriptors, active)
}

/// Best-effort [`save_app_tabs`]; logs a warning on failure. Called with the
/// engine lock held after a tab-list change, so it also drops the browser
/// state of tabs that are gone.
///
/// With `v1-service` the service's app registry is the record's one writer,
/// so this only prunes the view state.
pub fn persist_app_tabs(app: &er_engine::app::App, view: &ViewStateHandle) {
    let Ok(mut view) = view.lock() else {
        log::warn!("er-desktop: view state lock poisoned; tabs not persisted");
        return;
    };
    let live: Vec<er_engine::TabKey> = app.tabs.iter().map(|t| t.key()).collect();
    view.retain_tabs(&live);
    #[cfg(not(feature = "v1-service"))]
    if let Err(e) = save_app_tabs(app, &view) {
        log::warn!("er-desktop: failed to persist tabs: {e}");
    }
}

/// The keys a tab's browser state is saved under. The engine keeps them in the
/// descriptor's open map without reading them (ADR 0068).
const BROWSER_URL_KEY: &str = "browser_url";
const BROWSER_LAYOUT_KEY: &str = "browser_layout";

/// A tab's browser URL and layout as descriptor keys. An empty URL and the
/// hidden layout are left out, as they were when the record named them.
fn browser_descriptor_fields(
    browser: &TabBrowserState,
) -> serde_json::Map<String, serde_json::Value> {
    let mut fields = serde_json::Map::new();
    if !browser.url.trim().is_empty() {
        fields.insert(BROWSER_URL_KEY.to_string(), browser.url.clone().into());
    }
    if browser.layout != BrowserLayout::Hidden {
        fields.insert(
            BROWSER_LAYOUT_KEY.to_string(),
            browser.layout.as_str().to_string().into(),
        );
    }
    fields
}

/// Apply a saved descriptor's browser URL and layout to the restored tab's
/// entry in the view state.
pub fn apply_descriptor_browser(
    view: &mut DesktopViewState,
    tab: &er_engine::app::TabState,
    d: &TabDescriptor,
) {
    let browser = view.browser_mut(tab.key());
    let saved = |key: &str| d.extra.get(key).and_then(serde_json::Value::as_str);
    if let Some(url) = saved(BROWSER_URL_KEY) {
        browser.url = url.to_string();
    }
    if let Some(layout) = saved(BROWSER_LAYOUT_KEY) {
        browser.layout = BrowserLayout::from_label(layout);
    }
}

/// Convert a live `TabState` into a persistable descriptor.
pub fn descriptor_from_tab(
    tab: &er_engine::app::TabState,
    view: &DesktopViewState,
) -> TabDescriptor {
    let extra = browser_descriptor_fields(&view.browser(tab.key()));
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
            extra,
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
            extra,
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
            extra,
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
        extra,
    }
}

/// Rebuild a `TabState` from a descriptor. Skips work that needs the network
/// (e.g. PR overview fetch) — that's done lazily when the tab is focused.
///
/// When `lazy` is true, tabs are built without their initial diff load and get
/// `needs_initial_refresh = true`. The first diff runs when the tab gains focus
/// (or when the background warmer reaches a same-project stub). An eager
/// plain, local PR or remote PR tab loads through its network constructor.
pub fn rebuild_tab_with(
    d: &TabDescriptor,
    lazy: bool,
    ports: &er_engine::ports::Ports,
) -> Result<er_engine::app::TabState> {
    if !lazy {
        match d.kind {
            TabKind::Working => {
                return er_engine::app::TabState::new(d.repo_root.clone(), ports.clone());
            }
            TabKind::LocalPr => {
                let number = d
                    .pr_number
                    .context("local_pr descriptor missing pr_number")?;
                return er_engine::app::TabState::new_local_pr(
                    d.repo_root.clone(),
                    number,
                    ports.clone(),
                );
            }
            TabKind::RemotePr => {
                let pr_ref = er_engine::github::PrRef {
                    owner: d.pr_owner.clone().context("remote_pr missing owner")?,
                    repo: d.pr_repo.clone().context("remote_pr missing repo")?,
                    number: d.pr_number.context("remote_pr missing number")?,
                };
                return er_engine::app::TabState::new_remote(&pr_ref, ports.clone());
            }
            TabKind::LocalBranch => {}
        }
    }
    let resolved = er_engine::tab_identity::resolve_descriptor(&*ports.git, d)?;
    let mut tab = er_engine::app::TabState::from_resolved(resolved, ports.clone())?;
    if matches!(d.kind, TabKind::LocalBranch | TabKind::LocalPr) {
        // Reloads AI sidecars so Review/Notes/Context have this tab's data on
        // first select, without waiting for the deferred git diff.
        tab.sync_managed_storage();
    }
    if lazy {
        tab.needs_initial_refresh = true;
    } else {
        tab.refresh_diff()?;
    }
    Ok(tab)
}

/// Rebuild a saved tab (a stub without its first diff when `lazy`) and hand its
/// browser URL and layout to the view state under the new tab's key.
pub fn restore_tab(
    d: &TabDescriptor,
    lazy: bool,
    ports: &er_engine::ports::Ports,
    view: &mut DesktopViewState,
) -> Result<er_engine::app::TabState> {
    let tab = rebuild_tab_with(d, lazy, ports)?;
    apply_descriptor_browser(view, &tab, d);
    Ok(tab)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let d = descriptor_from_tab(&tab, &DesktopViewState::default());
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
            let d = descriptor_from_tab(&tab, &DesktopViewState::default());
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
        let d = descriptor_from_tab(&tab, &DesktopViewState::default());
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
                extra: serde_json::Map::new(),
            };
            let tab =
                rebuild_tab_with(&d, true, &er_engine::ports::Ports::for_test()).expect("stub");
            assert_eq!(tab.current_branch, "feat/from-fork");
            assert_eq!(tab.base_branch, "main");
            assert_eq!(tab.pr_number, Some(1425));
            assert!(tab.local_branch_view.is_none());
            assert!(tab.is_remote());
        });
    }

    /// The browser state restores from and saves to the keys an older record
    /// used, so a saved URL and layout survive a restart in both directions.
    #[test]
    fn the_browser_keys_restore_and_save_under_their_saved_names() {
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            let saved: TabDescriptor = serde_json::from_value(serde_json::json!({
                "kind": "remote_pr",
                "repo_root": "",
                "pr_owner": "octo",
                "pr_repo": "cat",
                "pr_number": 42,
                "browser_url": "http://localhost:5173/review",
                "browser_layout": "split"
            }))
            .expect("descriptor");
            let tab =
                rebuild_tab_with(&saved, true, &er_engine::ports::Ports::for_test()).expect("stub");
            let mut view = DesktopViewState::default();
            apply_descriptor_browser(&mut view, &tab, &saved);
            let browser = view.browser(tab.key());
            assert_eq!(browser.url, "http://localhost:5173/review");
            assert_eq!(browser.layout, BrowserLayout::Split);
            assert_eq!(descriptor_from_tab(&tab, &view).extra, saved.extra);
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
            er_engine::test_support::write_sidecar_json::<er_engine::ai::ErQuestions>(
                std::path::Path::new(&er),
                serde_json::json!({
                    "version": 1, "diff_hash": "", "questions": [
                        {"id":"q-1","file":"a.rs","hunk_index":0,"line_start":1,"line_content":"c","text":"why","resolved":false}
                    ]
                }),
            );

            let d = TabDescriptor {
                kind: TabKind::LocalPr,
                repo_root: root,
                branch: Some("feat-ai-stub".to_string()),
                pr_owner: None,
                pr_repo: None,
                pr_number: Some(99),
                pr_head_ref: None,
                base_ref: Some("main".to_string()),
                extra: serde_json::Map::new(),
            };
            let tab = rebuild_tab_with(&d, true, &er_engine::ports::Ports::cli()).expect("stub");
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
            let (descriptors, active) = persisted_tabs(&app, &DesktopViewState::default());
            assert_eq!(active, 0);
            assert_eq!(descriptors.len(), 1);
        });
    }

    fn remote_pr_tab() -> er_engine::app::TabState {
        let mut tab = er_engine::app::TabState::new_for_test(vec![]);
        tab.remote_repo = Some("octo/cat".to_string());
        tab.pr_number = Some(42);
        tab
    }

    #[test]
    fn tab_descriptors_round_trip_browser_state() {
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            let tab = remote_pr_tab();
            let mut view = DesktopViewState::default();
            view.browser_mut(tab.key())
                .apply(crate::view_state::BrowserUpdate {
                    url: Some("http://localhost:5173/review".to_string()),
                    layout: Some("fullscreen".to_string()),
                    annotate: Some(true),
                    ..Default::default()
                });
            save_tabs(&[descriptor_from_tab(&tab, &view)], 0).expect("save");

            let loaded = load_tabs().expect("a saved record");
            let mut restored_view = DesktopViewState::default();
            let restored = restore_tab(
                &loaded.tabs[0],
                true,
                &er_engine::ports::Ports::for_test(),
                &mut restored_view,
            )
            .expect("restore");

            assert_ne!(restored.key(), tab.key(), "a restored tab gets a new key");
            let browser = restored_view.browser(restored.key());
            assert_eq!(browser.url, "http://localhost:5173/review");
            assert_eq!(browser.layout, BrowserLayout::Fullscreen);
            assert!(!browser.annotate_mode, "only url and layout are saved");
        });
    }

    /// A `tabs.json` written by the build before the view state moved out of
    /// `TabState`, generated by running `save_tabs` on that build.
    #[test]
    fn a_tabs_file_from_before_the_view_state_restores_url_and_layout() {
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            let path = er_engine::store_registry::app_path::<TabsFile>().expect("tabs path");
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(
                &path,
                include_str!("../tests/fixtures/tabs-before-view-state.json"),
            )
            .expect("write fixture");

            let file = load_tabs().expect("the old file still loads");
            let mut view = DesktopViewState::default();
            let tab = restore_tab(
                &file.tabs[0],
                true,
                &er_engine::ports::Ports::for_test(),
                &mut view,
            )
            .expect("restore");

            assert_eq!(tab.pr_number, Some(42));
            let browser = view.browser(tab.key());
            assert_eq!(browser.url, "http://localhost:5173/review");
            assert_eq!(browser.layout, BrowserLayout::Split);
        });
    }

    #[test]
    fn placeholder_tab_is_not_persisted() {
        let storage = tempfile::tempdir().expect("tempdir");
        er_engine::storage::with_test_storage_root(storage.path(), || {
            // Saving it would make the next launch try to restore a tab with no repo.
            let app = er_engine::app::App::new_empty(er_engine::ports::Ports::for_test());
            let (descriptors, active) = persisted_tabs(&app, &DesktopViewState::default());
            assert!(descriptors.is_empty());
            assert_eq!(active, 0);
        });
    }
}
