//! The service path's snapshot source: the views the service publishes and
//! the desktop's own view state, read without the engine lock.
//!
//! The open tab's content (files, AI state, history, filters) has no
//! published owner yet: the actors write sidecars and publish a revision. So
//! this source answers no tab content, and `get_snapshot` keeps building from
//! the engine until an actor publishes it.
//!
//! Only compiled with `v1-service`.

use std::sync::Arc;

use er_engine::arena::{ArenaRunSummary, RunStatus};
use er_engine::config::ErConfig;
use er_engine::tabs_record::{TabDescriptor, TabKind};
use er_engine::TabKey;
use er_service::{SettingsView, TabsView, WorkspaceView};

use crate::snapshot::{
    AppSnapshot, ArenaRunSummaryWire, PrCache, ReviewFlowSnapshot, SessionRows, SnapshotRead,
    SnapshotSources, TabSummary, ViewRows,
};
use crate::view_state::{DesktopViewState, TabNavState};

/// One published view of each owner, plus the active tab's selection copied
/// out of the desktop view state, so a build holds no lock.
pub struct ViewRead {
    workspace: Arc<WorkspaceView>,
    settings: Arc<SettingsView>,
    tabs: Arc<TabsView>,
    nav: TabNavState,
    /// The active tab's review bucket, which picks its flow out of the
    /// workspace view. `None` shows no flow.
    bucket: Option<String>,
}

impl ViewRead {
    pub fn new(
        workspace: Arc<WorkspaceView>,
        settings: Arc<SettingsView>,
        tabs: Arc<TabsView>,
        view: &DesktopViewState,
        bucket: Option<String>,
    ) -> Self {
        let nav = tabs.active.map(|key| view.nav(key)).unwrap_or_default();
        Self {
            workspace,
            settings,
            tabs,
            nav,
            bucket,
        }
    }
}

impl SnapshotRead for ViewRead {
    fn tab_key(&self) -> Option<TabKey> {
        self.tabs.active
    }

    fn tab_content(&self, _: &SnapshotSources<'_>, _: bool) -> Option<AppSnapshot> {
        None
    }

    /// With no published file list there is no filter to translate through,
    /// so the selected index is its own visible position.
    fn view_rows(&self) -> ViewRows {
        ViewRows {
            selected_file: self.nav.selected_file,
            current_hunk: Some(self.nav.current_hunk),
        }
    }

    /// The registry's list. A tab's staleness and change token come from its
    /// diff, which is not published, so they stay unset.
    fn tab_strip(&self, _: Option<&PrCache>) -> (Vec<TabSummary>, usize) {
        let active = self.tabs.active;
        let summaries = self
            .tabs
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, entry)| tab_summary(idx, &entry.descriptor, Some(entry.key) == active))
            .collect();
        let position = self
            .tabs
            .tabs
            .iter()
            .position(|entry| Some(entry.key) == active)
            .unwrap_or(0);
        (summaries, position)
    }

    fn config(&self) -> &ErConfig {
        &self.settings.config
    }

    fn config_load_error(&self) -> Option<String> {
        self.settings.load_error.clone()
    }

    /// The service path has no input mode or AI pick of its own yet, and its
    /// notices are dropped until it renders from the service.
    fn session(&self) -> SessionRows {
        SessionRows {
            input_mode: "normal",
            notification: None,
            watching: false,
            active_ai_label: String::new(),
            active_ai_provider_label: None,
            active_ai_model_label: None,
            active_ai_effort: None,
            background_tasks: Vec::new(),
        }
    }

    fn review_flow(&self) -> Option<ReviewFlowSnapshot> {
        let live = self.workspace.review_flow(self.bucket.as_deref()?)?;
        Some(ReviewFlowSnapshot::from_flow(&live.flow, live.draining))
    }

    /// The actor's live runs. Title, start time and counts are in the run
    /// record on disk, which this source does not read.
    fn background_arena_runs(&self) -> Vec<ArenaRunSummaryWire> {
        self.workspace
            .arena_runs
            .iter()
            .rev()
            .filter(|run| matches!(run.status, RunStatus::Queued | RunStatus::Running { .. }))
            .map(|run| ArenaRunSummary {
                id: run.run_id.clone(),
                title: None,
                branch_ref: String::new(),
                status: run.status.clone(),
                created_at: String::new(),
                completed_at: None,
                reviewer_count: 0,
                finding_count: 0,
            })
            .collect()
    }
}

fn tab_summary(idx: usize, tab: &TabDescriptor, is_active: bool) -> TabSummary {
    let remote = match (&tab.pr_owner, &tab.pr_repo) {
        (Some(owner), Some(repo)) => Some(format!("{owner}/{repo}")),
        _ => None,
    };
    let branch = match tab.kind {
        TabKind::LocalBranch => tab.branch.clone(),
        _ => None,
    };
    let kind = match tab.kind {
        TabKind::RemotePr => "remote_pr",
        TabKind::LocalBranch => "local_branch",
        TabKind::Working | TabKind::LocalPr => "working",
    };
    TabSummary {
        idx,
        label: tab_label(tab, branch.as_deref()),
        kind: kind.to_string(),
        branch,
        pr_number: tab.pr_number,
        remote,
        repo_root: tab.repo_root.clone(),
        is_active,
        change_token: String::new(),
        stale: false,
    }
}

/// The engine's tab name, from what the descriptor holds: the viewed branch,
/// then `repo#N`, then the checkout's directory name.
fn tab_label(tab: &TabDescriptor, branch: Option<&str>) -> String {
    if let Some(branch) = branch {
        return branch.to_string();
    }
    if let Some(repo) = &tab.pr_repo {
        return match tab.pr_number {
            Some(number) => format!("{repo}#{number}"),
            None => repo.clone(),
        };
    }
    std::path::Path::new(&tab.repo_root)
        .file_name()
        .map_or_else(|| tab.repo_root.clone(), |name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use er_engine::ai::review_flow::ReviewFlow;
    use er_engine::arena::{ArenaProgressState, RunStatus};
    use er_engine::ports::Ports;
    use er_engine::tabs_record::{TabDescriptor, TabKind};
    use er_engine::TabKey;
    use er_service::{LiveArenaRun, LiveReviewFlow, TabEntry, TabsView, WorkspaceView};

    use super::ViewRead;
    use crate::snapshot::{build_snapshot_from, SnapshotSources};
    use crate::view_state::DesktopViewState;

    /// The production half of a source file, without its test module.
    fn production(src: &str) -> &str {
        src.split("\n#[cfg(test)]\nmod tests").next().unwrap_or(src)
    }

    /// Whole-identifier match, so `App` does not match `AppSnapshot`.
    fn has_ident(src: &str, ident: &str) -> bool {
        let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
        src.match_indices(ident).any(|(at, _)| {
            let before = src.get(..at).and_then(|s| s.chars().next_back());
            let after = src.get(at + ident.len()..).and_then(|s| s.chars().next());
            !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
        })
    }

    /// The service path's source reads published views only: never the
    /// engine `App`, its tabs, or the desktop state that holds them.
    #[test]
    fn the_view_source_reads_no_app_or_tab_state() {
        let src = production(include_str!("snapshot_view.rs"));
        for ident in [
            "App",
            "AppState",
            "AppRead",
            "TabState",
            "app",
            "er_engine::app",
        ] {
            assert!(
                !has_ident(src, ident),
                "snapshot_view.rs reads engine state through `{ident}`"
            );
        }
        for call in [".tab(", ".tab_mut(", ".tabs()", ".lock("] {
            assert!(
                !src.contains(call),
                "snapshot_view.rs reaches engine or locked state through `{call}`"
            );
        }
        // The engine tab's view fields, read off a tab value.
        for field in [
            "selected_file",
            "current_hunk",
            "current_line",
            "diff_scroll",
            "h_scroll",
        ] {
            for owner in ["tab.", "t."] {
                assert!(
                    !src.contains(&format!("{owner}{field}")),
                    "snapshot_view.rs reads `{owner}{field}` off a tab"
                );
            }
        }
    }

    fn working(root: &str) -> TabDescriptor {
        TabDescriptor {
            kind: TabKind::Working,
            repo_root: root.to_string(),
            branch: None,
            pr_owner: None,
            pr_repo: None,
            pr_number: None,
            pr_head_ref: None,
            base_ref: None,
            extra: serde_json::Map::new(),
        }
    }

    fn remote_pr(owner: &str, repo: &str, number: u64) -> TabDescriptor {
        TabDescriptor {
            kind: TabKind::RemotePr,
            pr_owner: Some(owner.to_string()),
            pr_repo: Some(repo.to_string()),
            pr_number: Some(number),
            ..working("")
        }
    }

    /// Every section the service publishes reaches the snapshot through the
    /// one builder: the tab strip from the registry, the selection from the
    /// desktop view state, the theme from the settings actor and the live
    /// arena runs from the workspace.
    #[test]
    fn the_published_sections_build_the_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, "[display]\ntheme = \"light\"\n").expect("seed config");
        let settings = er_engine::storage::with_process_test_storage_root(dir.path(), || {
            crate::service::ServiceState::with_settings_path(Ports::for_test(), config_path)
                .settings()
                .view()
        });
        assert_eq!(settings.config.display.theme, "light", "the config loaded");

        let (first, second) = (TabKey::next(), TabKey::next());
        let tabs = TabsView {
            revision: 1,
            tabs: vec![
                TabEntry {
                    key: first,
                    descriptor: working("/src/widgets"),
                },
                TabEntry {
                    key: second,
                    descriptor: remote_pr("acme", "gadgets", 7),
                },
            ],
            active: Some(second),
        };
        let workspace = WorkspaceView {
            arena_runs: vec![
                LiveArenaRun {
                    run_id: "run-1".to_string(),
                    er_dir: "/bucket".to_string(),
                    status: RunStatus::Complete,
                    progress: ArenaProgressState::default(),
                },
                LiveArenaRun {
                    run_id: "run-2".to_string(),
                    er_dir: "/bucket".to_string(),
                    status: RunStatus::Running { round: 1 },
                    progress: ArenaProgressState::default(),
                },
            ],
            review_flows: ["/other", "/bucket"]
                .into_iter()
                .map(|er_dir| LiveReviewFlow {
                    er_dir: er_dir.to_string(),
                    flow: ReviewFlow::new(format!("hash{er_dir}")),
                    draining: er_dir == "/bucket",
                    run_targets: Vec::new(),
                    rerun_stages: Vec::new(),
                })
                .collect(),
            ..WorkspaceView::default()
        };
        let mut view = DesktopViewState::default();
        view.nav_mut(second).selected_file = 4;
        view.nav_mut(second).current_hunk = 3;
        view.nav_mut(first).current_hunk = 9;

        let source = ViewRead::new(
            Arc::new(workspace),
            settings,
            Arc::new(tabs),
            &view,
            Some("/bucket".to_string()),
        );
        let snap = build_snapshot_from(&source, &SnapshotSources::default(), false);

        assert_eq!(
            (snap.selected_file, snap.current_hunk),
            (4, Some(3)),
            "the selection is the active tab's own"
        );
        assert_eq!(snap.active_tab, 1);
        let strip: Vec<_> = snap
            .tabs
            .iter()
            .map(|tab| (tab.label.as_str(), tab.kind.as_str(), tab.is_active))
            .collect();
        assert_eq!(
            strip,
            [("widgets", "working", false), ("gadgets#7", "remote_pr", true)]
        );
        assert_eq!(snap.tabs[1].remote.as_deref(), Some("acme/gadgets"));
        assert_eq!(snap.theme, "light");
        let live: Vec<_> = snap
            .background_arena_runs
            .iter()
            .map(|run| run.id.as_str())
            .collect();
        assert_eq!(live, ["run-2"], "only a run still going is live");
        let flow = snap.review_flow.expect("the active bucket's flow");
        assert_eq!(
            (flow.diff_hash.as_str(), flow.draining),
            ("hash/bucket", true),
            "the flow is the active bucket's"
        );
        assert_eq!(snap.input_mode, "normal");
        assert!(snap.files.is_empty(), "no tab content is published");
    }
}
