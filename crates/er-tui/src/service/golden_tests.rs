//! P63's golden suite. Each fixture fills the TUI's own view state and a
//! tab's published content, draws them through `tui_view::screen`, and must
//! match its file under `goldens/`, byte for byte.

use std::sync::Arc;

use er_engine::config::ErConfig;
use er_engine::tab_identity::DiffMode;
use er_service::{DiffSection, TabContent};
use ratatui::buffer::Buffer;
use ratatui::{backend::TestBackend, Terminal};

use super::agent_log::Jobs;
use super::tests::{fixture, loaded, path_of, settle, shell};
use super::tui_view::{self, Frame, Mode, Overlay, TabViewState, TuiView};
use crate::ui::highlight::Highlighter;
use er_engine::ai::PanelContent;

/// What one fixture draws: the view state, the published content and the
/// settings the frame reads.
pub(crate) struct Scene {
    pub(crate) view: TuiView,
    pub(crate) state: TabViewState,
    pub(crate) content: TabContent,
    pub(crate) config: ErConfig,
    pub(crate) tab_names: Vec<String>,
    pub(crate) watching: bool,
    pub(crate) config_load_error: Option<String>,
    pub(crate) jobs: Jobs,
    /// The settings view the hubs open on; its config is the scene's.
    pub(crate) settings: er_service::SettingsView,
}

impl Scene {
    /// The settings view, carrying the scene's config as it now stands.
    fn settings(&self) -> er_service::SettingsView {
        let mut view = self.settings.clone();
        view.config = Arc::new(self.config.clone());
        view
    }

    pub(crate) fn ai(&mut self, edit: impl FnOnce(&mut er_engine::ai::AiState)) {
        let ai = &self.content.ai;
        let mut state = ai.state.clone();
        edit(&mut state);
        self.content.ai = Arc::new(er_service::AiSection {
            revision: ai.revision,
            written: ai.written,
            state,
            stamp: ai.stamp.clone(),
            extras: ai.extras.clone(),
        });
    }

    pub(crate) fn marks(&mut self, paths: &[&str]) {
        let reviewed = &self.content.reviewed;
        self.content.reviewed = Arc::new(er_service::ReviewedSection {
            revision: reviewed.revision,
            marks: paths
                .iter()
                .map(|path| ((*path).to_string(), "hash".to_string()))
                .collect(),
            file_hashes: reviewed.file_hashes.clone(),
        });
    }

    pub(crate) fn files(&mut self, files: Vec<er_engine::git::DiffFile>) {
        let diff = &self.content.diff;
        self.content.diff = Arc::new(DiffSection {
            revision: diff.revision,
            files: files.into_iter().map(Arc::new).collect(),
            meta: Arc::clone(&diff.meta),
            expanded: Default::default(),
            context: Default::default(),
        });
    }

    /// A merge in progress with `unresolved` conflicted files, as the load
    /// reads it.
    pub(crate) fn merge(&mut self, active: bool, unresolved: usize) {
        let diff = &self.content.diff;
        let meta = &diff.meta;
        let meta = er_service::DiffMeta {
            headers: meta.headers.clone(),
            raw: meta.raw.clone(),
            lazy: meta.lazy,
            diff_hash: meta.diff_hash.clone(),
            branch_diff_hash: meta.branch_diff_hash.clone(),
            preview_head_oid: meta.preview_head_oid.clone(),
            stamps: meta.stamps.clone(),
            scope_stats: meta.scope_stats,
            rules_key: meta.rules_key.clone(),
            risk_artifact: meta.risk_artifact.clone(),
            commits: meta.commits.clone(),
            committed_unpushed: meta.committed_unpushed,
            merge_active: active,
            unresolved,
        };
        self.content.diff = Arc::new(DiffSection {
            revision: diff.revision,
            files: diff.files.clone(),
            meta: Arc::new(meta),
            expanded: Default::default(),
            context: Default::default(),
        });
    }

    fn overlay(&mut self, overlay: Overlay) {
        self.view.overlay = Some(overlay);
    }
}

struct Fixture {
    name: &'static str,
    width: u16,
    height: u16,
    build: fn(&mut Scene),
}

const fn fixture_of(name: &'static str, width: u16, height: u16, build: fn(&mut Scene)) -> Fixture {
    Fixture {
        name,
        width,
        height,
        build,
    }
}

/// A tab as `App::new_for_test` makes it: `feature` over `main` at
/// `/tmp/test`, in Branch, showing `golden.rs`'s diff, with the default
/// config and no AI selection.
fn scene(loaded: &TabContent, settings: &er_service::SettingsView) -> Scene {
    let mut content = loaded.clone();
    content.tab.repo_root = "/tmp/test".into();
    content.tab.current_branch = "feature".into();
    content.tab.base_branch = "main".into();
    content.tab.mode = DiffMode::Branch;
    content.tab.remote_repo = None;
    content.tab.pr_number = None;
    content.tab.local_branch_view = None;
    content.tab.checkout_root = None;
    let files = er_engine::git::parse_diff(crate::golden::RAW_DIFF);
    content.diff = Arc::new(DiffSection {
        revision: content.diff.revision,
        files: files.into_iter().map(Arc::new).collect(),
        meta: Arc::clone(&content.diff.meta),
        expanded: Default::default(),
        context: Default::default(),
    });
    let tab_names = vec![tui_view::tab_name(&content)];
    let mut state = TabViewState::default();
    state.file_tree_width = 32;
    state.panel_width = 40;
    Scene {
        view: TuiView::default(),
        state,
        content,
        config: ErConfig::default(),
        tab_names,
        watching: false,
        config_load_error: None,
        jobs: Jobs::default(),
        settings: settings.clone(),
    }
}

/// The screen the renderers read for `scene`, as the loop builds it.
pub(crate) fn screen_of(scene: &Scene) -> crate::screen::Screen<'_> {
    let content = &scene.content;
    let total = content.diff.files.len();
    let reviewed = content
        .diff
        .files
        .iter()
        .filter(|file| content.reviewed.marks.contains_key(&file.path))
        .count();
    let frame = Frame {
        tab_names: scene.tab_names.clone(),
        active_tab: 0,
        content,
        config: &scene.config,
        config_load_error: scene.config_load_error.as_deref(),
        ai_selection_label: er_service::ai_labels(&scene.config).label,
        reviewed_count: (reviewed, total),
        watching: scene.watching,
        jobs: &scene.jobs,
    };
    tui_view::screen(&scene.view, &scene.state, frame)
}

fn draw(scene: &Scene, width: u16, height: u16, hl: &mut Highlighter) -> Buffer {
    let screen = screen_of(scene);
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).unwrap_or_else(|e| panic!("{e}"));
    terminal
        .draw(|f| crate::ui::draw(f, &screen, hl))
        .unwrap_or_else(|e| panic!("{e}"));
    terminal.backend().buffer().clone()
}

/// Loads a real tab once and hands its content and the settings view to
/// `run`, which builds its scenes from them.
fn with_loaded(run: impl FnOnce(&TabContent, &er_service::SettingsView)) {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let args = super::startup::Args {
            paths: vec![path],
            ..super::startup::Args::default()
        };
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        let loaded = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        run(&loaded, &shell.host().settings().view());
    });
}

/// `golden.rs`'s base tab as a `SectionData` scene, for a renderer's unit
/// test to change and draw through [`screen_of`].
pub(crate) fn section_scene() -> Scene {
    let mut built = None;
    with_loaded(|loaded, settings| built = Some(scene(loaded, settings)));
    built.unwrap_or_else(|| panic!("no scene"))
}

fn check(fixtures: &[Fixture]) {
    with_loaded(|loaded, settings| {
        let mut hl = Highlighter::new();
        let changed: Vec<String> = fixtures
            .iter()
            .filter_map(|fixture| {
                let mut scene = scene(loaded, settings);
                (fixture.build)(&mut scene);
                // The loop follows the tour each tick, as the App rebuilds it.
                super::tour::follow(&mut scene.state.tour, &scene.content);
                let drawn = draw(&scene, fixture.width, fixture.height, &mut hl);
                crate::golden::differs(fixture.name, &drawn)
            })
            .collect();
        assert!(
            changed.is_empty(),
            "{} golden frame(s) differ on the flag path\n{}",
            changed.len(),
            changed.join("\n")
        );
    });
}

/// The panels, the diff modes, the split diff and the watched files.
const PANES: &[Fixture] = &[
    fixture_of("pane_file_detail", 140, 24, |scene| {
        scene.ai(crate::golden::seed_review);
        scene.state.panel = Some(PanelContent::FileDetail);
        scene.state.focused_finding_id = Some("f1".into());
        scene.marks(&["README.md"]);
    }),
    fixture_of("pane_ai_summary", 140, 24, |scene| {
        scene.ai(|ai| {
            crate::golden::seed_review(ai);
            ai.summary = Some("Reads the audience before the token check.".into());
            ai.checklist = Some(
                serde_json::from_value(serde_json::json!({
                    "version": 1, "diff_hash": "hash",
                    "items": [{"id": "c1", "text": "Empty tokens are rejected", "checked": true}]
                }))
                .unwrap_or_else(|e| panic!("{e}")),
            );
        });
        scene.state.panel = Some(PanelContent::AiSummary);
    }),
    fixture_of("pane_pr_overview", 140, 20, |scene| {
        scene.content.tab.pr_number = Some(7);
        scene.state.panel = Some(PanelContent::PrOverview);
        let overview = serde_json::from_value(serde_json::json!({
            "number": 7, "title": "Validate the token audience",
            "body": "Rejects empty tokens.", "state": "OPEN", "author": "octo",
            "url": "https://github.com/o/r/pull/7", "base_branch": "main",
            "head_branch": "feature",
            "checks": [{"name": "test", "status": "COMPLETED", "conclusion": "SUCCESS"}],
            "reviewers": [{"login": "hubot", "state": "APPROVED"}]
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        let pr = &scene.content.pr;
        scene.content.pr = Arc::new(er_service::PrSection {
            revision: pr.revision,
            pr_number: Some(7),
            overview: Some(overview),
            commits: Vec::new(),
            head_oid: None,
            stack: None,
            branch: None,
        });
    }),
    fixture_of("pane_agent_log", 140, 16, |scene| {
        use crate::screen::view::{AgentLogEntry, AgentLogSource};
        scene.state.panel = Some(PanelContent::AgentLog);
        scene.jobs.running = vec!["review".into()];
        let now = std::time::Instant::now();
        for (source, text) in [
            (AgentLogSource::Status, "started"),
            (AgentLogSource::Stdout, "reading src/auth.rs"),
            (AgentLogSource::Stderr, "warning: no tests touched"),
        ] {
            scene.jobs.log.push(AgentLogEntry {
                timestamp: now,
                command_name: "review".into(),
                source,
                text: text.into(),
            });
        }
    }),
    fixture_of("diff_unstaged", 100, 12, |scene| {
        scene.content.tab.mode = DiffMode::Unstaged;
    }),
    fixture_of("diff_staged", 100, 12, |scene| {
        scene.content.tab.mode = DiffMode::Staged;
    }),
    fixture_of("diff_pr", 100, 12, |scene| {
        scene.content.tab.pr_number = Some(7);
        scene.content.tab.mode = DiffMode::PrDiff;
    }),
    fixture_of("diff_tour", 110, 18, |scene| {
        scene.ai(|ai| {
            ai.tour = Some(
                serde_json::from_value(serde_json::json!({
                    "version": 1, "diff_hash": "hash", "created_at": "", "title": "Audience",
                    "overview": "", "pillars": [{
                        "id": "p1", "title": "Token check", "order": 0, "importance": 3,
                        "description": "Where the audience is read.",
                        "files": [{"path": "src/auth.rs"}]
                    }]
                }))
                .unwrap_or_else(|e| panic!("{e}")),
            );
        });
        scene.content.tab.mode = DiffMode::Tour;
        scene.marks(&["src/auth.rs"]);
    }),
    fixture_of("diff_conflicts", 100, 12, |scene| {
        scene.content.tab.mode = DiffMode::Conflicts;
        scene.merge(true, 1);
    }),
    fixture_of("diff_history", 110, 16, |scene| {
        history(scene, "2 days ago");
    }),
    fixture_of("diff_split", 120, 14, |scene| {
        scene.config.display.split_diff = true;
    }),
    fixture_of("watched_snapshot", 100, 16, |scene| {
        scene.config.watched.diff_mode = "snapshot".into();
        let watched = &mut scene.state.watched;
        watched.shown = Some(true);
        watched.files = vec![watched_file(".env.local", 48, 42)];
        watched.selected = Some(0);
        watched.read(".env.local".into(), snapshot_changed());
    }),
    fixture_of("diff_hidden", 100, 16, |scene| {
        scene.content.tab.mode = DiffMode::Hidden;
        scene.files(Vec::new());
        scene.config.watched.paths = vec![".env*".into()];
        scene.config.watched.diff_mode = "snapshot".into();
        let watched = &mut scene.state.watched;
        watched.shown = Some(true);
        watched.files = vec![
            watched_file(".env.local", 48, 42),
            watched_file(".env.test", 72, 17),
        ];
        watched.not_ignored = vec![".env.test".into()];
        watched.selected = Some(0);
        watched.read(".env.local".into(), snapshot_changed());
    }),
];

fn watched_file(path: &str, hours: u64, size: u64) -> er_engine::git::WatchedFile {
    er_engine::git::WatchedFile {
        path: path.into(),
        modified: std::time::SystemTime::now() - std::time::Duration::from_hours(hours),
        size,
    }
}

/// The snapshot diff `golden.rs`'s watched fixtures script.
fn snapshot_changed() -> crate::screen::watched::SnapshotDiff {
    let snapshot = "diff --git a/.env.local b/.env.local\n--- a/.env.local\n+++ b/.env.local\n@@ -1,1 +1,2 @@\n KEY=1\n+TOKEN=abc\n";
    let file = er_engine::git::parse_diff(snapshot).into_iter().next();
    crate::screen::watched::SnapshotDiff::Changed(file)
}

/// History on `golden.rs`'s one commit, aged `age`.
fn history(scene: &mut Scene, age: &str) {
    let commit = er_engine::git::CommitInfo {
        hash: "a".repeat(40),
        short_hash: "aaaaaaa".into(),
        subject: "Validate the token audience".into(),
        author: "octo".into(),
        date: "2026-11-20".into(),
        relative_date: age.into(),
        file_count: 2,
        adds: 3,
        dels: 1,
        is_merge: false,
        diffstat_known: true,
    };
    scene.content.tab.mode = DiffMode::History;
    scene.content.history = Some(Arc::new(er_service::HistorySection {
        commits: vec![commit],
        all_loaded: true,
        selected: Some("a".repeat(40)),
        return_mode: DiffMode::Branch,
        revision: scene.content.diff.revision,
    }));
    scene.state.history.follow(&scene.content);
}

/// The History row draws a commit's age after ` · <hash> · `, where the
/// frozen frames collapse it to `<age>`; a row that moved it elsewhere
/// would freeze a raw age into the goldens, so this draws the real row.
#[test]
fn the_history_rows_age_collapses_in_a_frozen_frame() {
    with_loaded(|loaded, settings| {
        let mut hl = Highlighter::new();
        let drawn: Vec<String> = ["9 years ago", "10 years ago", "1 year, 2 months ago"]
            .into_iter()
            .map(|age| {
                let mut scene = scene(loaded, settings);
                history(&mut scene, age);
                super::parity::stable_text(&draw(&scene, 110, 16, &mut hl))
            })
            .collect();
        assert!(drawn[0].contains("aaaaaaa · <age>"), "{}", drawn[0]);
        assert_eq!(drawn[0], drawn[1]);
        assert_eq!(drawn[0], drawn[2]);
    });
}

/// The app chrome, the hubs, the other overlays and the status-bar input
/// modes. The search line is pinned by the frozen frames of
/// `filter_tests::search_keys_match_the_app`.
const CHROME: &[Fixture] = &[
    fixture_of("chrome_tabs_update_notice", 100, 16, |scene| {
        scene.tab_names.push("docs".into());
        scene.view.available_update = Some("1.0.1".into());
        scene.watching = true;
        crate::golden::providers_with_efforts(&mut scene.config);
        scene.view.notification = Some("2 files changed".into());
    }),
    fixture_of("chrome_config_error", 100, 12, |scene| {
        scene.config_load_error = Some("config.toml: expected `=` at line 3".into());
    }),
    fixture_of("hub_git", 90, 20, |scene| {
        let mut hub =
            super::worktree::GitHub::open(&scene.content, super::tui_view::PrKnown::Unknown);
        hub.drop_review_row();
        scene.overlay(Overlay::GitHub(hub));
    }),
    fixture_of("hub_ai", 90, 20, |scene| {
        let flow = super::flows::FlowFacts::of(&scene.content, None);
        let facts = super::ai_picker::HubFacts::of(&scene.content.ai.state, true, flow);
        let label = er_service::ai_labels(&scene.config).label;
        let step = super::ai_picker::open_ai_hub(&scene.settings(), &label, facts);
        scene.overlay(opened(step));
    }),
    fixture_of("hub_verify", 90, 20, |scene| {
        crate::golden::package_web(&mut scene.config);
        scene.config.commands.test = Some("cargo test".into());
        let label = er_service::ai_labels(&scene.config).label;
        let step = super::ai_picker::open_verify_hub(&scene.settings(), &label);
        scene.overlay(opened(step));
    }),
    fixture_of("hub_help", 90, 20, |scene| {
        scene.overlay(Overlay::Help(super::chrome::HelpHub::open()));
    }),
    fixture_of("hub_open", 90, 20, |scene| {
        let hub = super::open_hub::OpenHub::open(&scene.content, false, None);
        scene.overlay(Overlay::Open(hub));
    }),
    fixture_of("hub_copy", 90, 20, |scene| {
        let hub = super::copy::CopyHub::open(&scene.state, &scene.content);
        scene.overlay(Overlay::Copy(hub));
    }),
    fixture_of("overlay_worktrees", 90, 20, |scene| {
        let worktree = |path: &str, branch: &str| er_engine::git::Worktree {
            path: path.into(),
            branch: branch.into(),
            head: "abc1234".into(),
        };
        scene.overlay(Overlay::Worktrees(super::open::Worktrees {
            worktrees: vec![worktree("/src/er", "main"), worktree("/src/er-fix", "fix")],
            selected: 1,
        }));
    }),
    fixture_of("overlay_directory", 90, 20, |scene| {
        let entry = |name: &str, is_dir, is_git_repo| crate::screen::view::DirEntry {
            name: name.into(),
            is_dir,
            is_git_repo,
        };
        scene.overlay(Overlay::Directory(super::open::Directory {
            current_path: "/src".into(),
            entries: vec![
                entry("er", true, true),
                entry("notes", true, false),
                entry("todo.md", false, false),
            ],
            selected: 0,
        }));
    }),
    fixture_of("overlay_filter_history", 90, 20, |scene| {
        scene.state.filter_history = vec!["+*.rs".into(), "-*.lock".into()];
        scene.overlay(Overlay::FilterHistory {
            history: scene.state.filter_history.clone(),
            selected: 0,
        });
    }),
    fixture_of("overlay_export", 90, 20, |scene| {
        scene.overlay(super::export::open());
    }),
    fixture_of("hub_ai_provider", 90, 20, |scene| {
        crate::golden::providers_with_efforts(&mut scene.config);
        open_ai_hub(scene);
        drive(scene, "Change agent/model");
    }),
    fixture_of("hub_ai_model", 90, 20, |scene| {
        crate::golden::providers_with_efforts(&mut scene.config);
        open_ai_hub(scene);
        drive(scene, "Change agent/model");
        drive(scene, "Claude");
    }),
    fixture_of("hub_ai_effort", 90, 20, |scene| {
        crate::golden::providers_with_efforts(&mut scene.config);
        open_ai_hub(scene);
        drive(scene, "Change agent/model");
        drive(scene, "Claude");
        drive(scene, "OPUS");
    }),
    fixture_of("hub_ai_expert", 90, 20, |scene| {
        open_ai_hub(scene);
        drive(scene, "Specialized review");
    }),
    fixture_of("hub_verify_package", 90, 20, |scene| {
        crate::golden::package_web(&mut scene.config);
        let label = er_service::ai_labels(&scene.config).label;
        let step = super::ai_picker::open_verify_hub(&scene.settings(), &label);
        scene.overlay(opened(step));
        drive(scene, "Web");
    }),
    fixture_of("overlay_config_hub", 90, 24, |scene| {
        use crate::input::{Edit, OverlayAction};
        let view = scene.settings();
        let mut hub = super::config_hub::ConfigHub::open(&view);
        hub.selected = hub
            .items
            .iter()
            .position(|item| matches!(item, er_engine::config::ConfigItem::StringEdit { .. }))
            .unwrap_or(0);
        super::config_hub::act(&mut hub, OverlayAction::ActivateSetting, &view);
        let typed = hub
            .editing
            .as_ref()
            .map_or(0, |edit| edit.buffer.chars().count());
        for _ in 0..typed {
            super::config_hub::act(&mut hub, OverlayAction::EditSetting(Edit::Backspace), &view);
        }
        for c in "vim".chars() {
            super::config_hub::act(&mut hub, OverlayAction::EditSetting(Edit::Insert(c)), &view);
        }
        scene.overlay(Overlay::ConfigHub(hub));
    }),
    fixture_of("overlay_flow_progress", 90, 20, |scene| {
        use er_engine::ai::review_flow::*;
        let mut flow = ReviewFlow::new("hash");
        flow.tasks.push(FlowTaskProgress {
            id: "finder".into(),
            stage: FlowTaskStage::Finding,
            lens: Some("security".into()),
            label: "Security".into(),
            purpose: "Inspect authorization boundaries".into(),
            provider_id: "claude".into(),
            provider_label: "Claude".into(),
            model_id: "opus".into(),
            model_label: "Opus".into(),
            status: FlowTaskStatus::Running,
            error: None,
            background_task_id: None,
            round: Some(1),
            total_rounds: Some(2),
        });
        let progress = super::flows::Progress::new("/er/bucket".into(), flow);
        scene.overlay(Overlay::FlowProgress(Box::new(progress)));
    }),
    fixture_of("overlay_flow_results", 100, 24, |scene| {
        use er_engine::review_flow_driver::{ReviewFlowResultFinding, ReviewFlowResults};
        let finding = |id: &str, title: &str, accepted| ReviewFlowResultFinding {
            id: id.into(),
            file: "src/auth.rs".into(),
            line: Some(2),
            title: title.into(),
            body: "The audience is read before the token is validated.".into(),
            confidence: 0.82,
            raised_by: vec!["security".into()],
            verdict: er_engine::arena::Verdict::Kept,
            accepted,
            selectable: !accepted,
        };
        let mut results = super::flows::Results::new(ReviewFlowResults {
            er_dir: "/er/bucket".into(),
            run_id: "run-1".into(),
            diff_hash: "hash".into(),
            findings: vec![
                finding("f1", "Audience read early", false),
                finding("f2", "Empty token", true),
            ],
        });
        results.selected_ids = ["f1".to_string()].into();
        scene.overlay(Overlay::FlowResults(Box::new(results)));
    }),
    fixture_of("input_comment", 100, 12, |scene| {
        scene.view.input_mode = Mode::Comment;
        let draft = &mut scene.state.draft;
        draft.file = "src/auth.rs".into();
        draft.line = Some(2);
        draft.kind = er_engine::ai::CommentType::Question;
        draft.textarea = vec!["Why read the audience first?".to_string()].into();
    }),
    fixture_of("input_confirm", 100, 10, |scene| {
        scene.view.input_mode = Mode::Confirm(super::tui_view::Ask::DeleteWatchedFile);
        scene.state.watched.deleting = Some(".env.local".into());
    }),
    fixture_of("input_filter", 100, 10, |scene| {
        scene.view.input_mode = Mode::Filter;
        scene.state.filter_input = "+*.rs".into();
    }),
    fixture_of("input_commit", 100, 10, |scene| {
        scene.view.input_mode = Mode::Commit;
        scene.state.commit_input = "Validate the token audience".into();
    }),
    fixture_of("input_remote_url", 100, 10, |scene| {
        scene.view.input_mode = Mode::RemoteUrl;
        scene.view.base_input = "https://github.com/o/r/pull/1".into();
    }),
    fixture_of("input_base_branch", 100, 10, |scene| {
        scene.view.input_mode = Mode::BaseBranch;
        scene.view.base_input = "develop".into();
    }),
];

/// Opens the AI hub as `A` does, on the scene's settings.
fn open_ai_hub(scene: &mut Scene) {
    let flow = super::flows::FlowFacts::of(&scene.content, None);
    let facts = super::ai_picker::HubFacts::of(&scene.content.ai.state, true, flow);
    let label = er_service::ai_labels(&scene.config).label;
    let step = super::ai_picker::open_ai_hub(&scene.settings(), &label, facts);
    scene.overlay(opened(step));
}

/// Moves the open picker down to the row holding `to` and runs it, as `j`
/// and Enter do; the picker it opens replaces it.
fn drive(scene: &mut Scene, to: &str) {
    use crate::input::OverlayAction;
    let view = scene.settings();
    let label = er_service::ai_labels(&scene.config).label;
    let Some(Overlay::Picker(picker)) = scene.view.overlay.as_mut() else {
        panic!("no picker open");
    };
    for _ in 0..40 {
        if picker.rows[picker.selected].label.contains(to) {
            break;
        }
        super::ai_picker::act(picker, OverlayAction::Next, &view, &label);
    }
    assert!(
        picker.rows[picker.selected].label.contains(to),
        "no row {to}"
    );
    let step = super::ai_picker::act(picker, OverlayAction::SelectRow, &view, &label);
    scene.overlay(opened(step));
}

/// The overlay a hub's opening answers.
fn opened(step: super::config_hub::Step) -> Overlay {
    match step {
        super::config_hub::Step::Open(overlay, _) => *overlay,
        _ => panic!("the hub did not open"),
    }
}

#[test]
fn the_flag_path_draws_the_panes_goldens() {
    check(PANES);
}

#[test]
fn the_flag_path_draws_the_chrome_goldens() {
    check(CHROME);
}
