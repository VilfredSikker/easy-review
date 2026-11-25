//! Golden frames: each fixture draws a whole screen to `TestBackend` and
//! compares it with a file under `goldens/`. A file holds the rows as text and
//! a digest of every cell's style, so a colour or highlight change fails as
//! surely as a text change. `ER_UPDATE_GOLDENS=1` rewrites the files.

use crate::ui;
use er_engine::ai::PanelContent;
use er_engine::app::{App, ConfigEditState, ConfirmAction, DirEntry, InputMode, OverlayData};
use er_engine::config::{ConfigItem, PackageConfig};
use er_engine::ports::Ports;
use er_engine::tab_identity::DiffMode;
use er_git::FakeGit;
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use std::sync::Arc;
use std::time::Instant;

pub struct Fixture {
    pub name: &'static str,
    pub width: u16,
    pub height: u16,
    pub build: fn() -> App,
}

pub fn draw(app: &App, width: u16, height: u16, hl: &mut ui::highlight::Highlighter) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut cache = crate::screen::ViewCache::default();
    terminal
        .draw(|f| ui::draw(f, &crate::screen::from_app(app, &mut cache), hl))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn golden_text(buf: &Buffer) -> String {
    let rows = buf.content.chunks(usize::from(buf.area.width));
    rows_text(rows, |line| line.to_string())
}

/// The rows as text, each passed through `line` with its trailing blanks
/// trimmed, then an FNV-1a digest of every cell's style in cell order. The
/// parity goldens share it, so the two formats cannot drift.
pub(crate) fn rows_text<'a>(
    rows: impl IntoIterator<Item = &'a [ratatui::buffer::Cell]>,
    line: impl Fn(&str) -> String,
) -> String {
    let mut out = String::new();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for row in rows {
        let text: String = row.iter().map(ratatui::buffer::Cell::symbol).collect();
        out.push_str(&line(text.trim_end()));
        out.push('\n');
        for cell in row {
            for byte in format!("{:?}", cell.style()).bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    out.push_str(&format!("styles {hash:016x}\n"));
    out
}

/// Builds the fixture under a scratch storage root: some hub builders read it.
fn build(fixture: &Fixture) -> App {
    let root = tempfile::tempdir().unwrap();
    er_engine::storage::with_test_storage_root(root.path(), fixture.build)
}

pub fn check(fixtures: &[Fixture]) {
    let update = std::env::var_os("ER_UPDATE_GOLDENS").is_some();
    let mut hl = ui::highlight::Highlighter::new();
    let mut changed = Vec::new();
    for fixture in fixtures {
        let app = build(fixture);
        let actual = golden_text(&draw(&app, fixture.width, fixture.height, &mut hl));
        let path = format!(
            "{}/goldens/{}.txt",
            env!("CARGO_MANIFEST_DIR"),
            fixture.name
        );
        if update {
            std::fs::write(&path, &actual).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_default();
        if expected != actual {
            changed.push(format!(
                "{}:\n--- golden\n{expected}--- drawn\n{actual}",
                fixture.name
            ));
        }
    }
    assert!(
        changed.is_empty(),
        "{} golden frame(s) changed\n{}",
        changed.len(),
        changed.join("\n")
    );
}

/// How a frame drawn elsewhere differs from the golden file `name`, or
/// `None` when it matches. The flag path's suite reads the same files and
/// never rewrites them: the App's fixtures record them.
pub(crate) fn differs(name: &str, buffer: &Buffer) -> Option<String> {
    let path = format!("{}/goldens/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    let actual = golden_text(buffer);
    (expected != actual).then(|| format!("{name}:\n--- golden\n{expected}--- drawn\n{actual}"))
}

/// Mean draw time per fixture, printed for the PR's before and after.
pub fn print_frame_times(fixtures: &[Fixture]) {
    const FRAMES: u32 = 200;
    let mut hl = ui::highlight::Highlighter::new();
    let mut total = 0u128;
    for fixture in fixtures {
        let app = build(fixture);
        let start = Instant::now();
        for _ in 0..FRAMES {
            draw(&app, fixture.width, fixture.height, &mut hl);
        }
        let micros = start.elapsed().as_micros() / u128::from(FRAMES);
        total += micros;
        println!("frame-time {:<24} {micros:>6} us", fixture.name);
    }
    println!("frame-time {:<24} {total:>6} us", "SUM");
}

pub(crate) const RAW_DIFF: &str = "\
diff --git a/src/auth.rs b/src/auth.rs
index 1111111..2222222 100644
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -1,5 +1,6 @@
 fn check(token: &str) -> bool {
-    token.len() > 0
+    let audience = audience_of(token);
+    !token.is_empty() && audience == \"er\"
 }
 
 fn audience_of(token: &str) -> &str {
diff --git a/README.md b/README.md
index 3333333..4444444 100644
--- a/README.md
+++ b/README.md
@@ -1,2 +1,3 @@
 # er
+Review diffs as they land.
 A terminal diff reviewer.
";

pub fn base_app() -> App {
    App::new_for_test(er_engine::git::parse_diff(RAW_DIFF))
}

fn with(f: impl FnOnce(&mut App)) -> App {
    let mut app = base_app();
    f(&mut app);
    app
}

const fn fixture(name: &'static str, width: u16, height: u16, build: fn() -> App) -> Fixture {
    Fixture {
        name,
        width,
        height,
        build,
    }
}

/// The app chrome, the hubs, the other overlays and the status-bar input modes.
pub const CHROME: &[Fixture] = &[
    fixture("chrome_tabs_update_notice", 100, 16, || {
        with(|app| {
            let mut second = er_engine::app::TabState::new_for_test(Vec::new());
            second.current_branch = "docs".into();
            app.tabs.push(second);
            app.available_update = Some("1.0.1".into());
            app.watching = true;
            model_with_efforts(app);
            app.notify("2 files changed");
        })
    }),
    fixture("chrome_config_error", 100, 12, || {
        with(|app| app.config_load_error = Some("config.toml: expected `=` at line 3".into()))
    }),
    fixture("hub_git", 90, 20, || with(App::open_git_hub)),
    fixture("hub_ai", 90, 20, || with(App::open_ai_hub)),
    fixture("hub_ai_provider", 90, 20, || {
        with(|app| {
            model_with_efforts(app);
            app.open_ai_provider_picker(None);
        })
    }),
    fixture("hub_ai_model", 90, 20, || {
        with(|app| {
            let (provider, _) = model_with_efforts(app);
            app.open_ai_model_picker(provider, None);
        })
    }),
    fixture("hub_ai_effort", 90, 20, || {
        with(|app| {
            let (provider, model) = model_with_efforts(app);
            app.open_ai_effort_picker(provider, model, None);
        })
    }),
    fixture("hub_ai_expert", 90, 20, || with(App::open_ai_expert_picker)),
    fixture("hub_verify", 90, 20, || {
        with(|app| {
            add_package(app);
            app.config.commands.test = Some("cargo test".into());
            app.open_verify_hub();
        })
    }),
    fixture("hub_verify_package", 90, 20, || {
        with(|app| {
            add_package(app);
            app.open_package_commands_hub("web".into());
        })
    }),
    fixture("hub_help", 90, 20, || with(App::open_help_hub)),
    fixture("hub_open", 90, 20, || with(App::open_open_hub)),
    fixture("hub_copy", 90, 20, || with(App::open_copy_hub)),
    fixture("overlay_worktrees", 90, 20, || {
        with(|app| {
            let worktree = |path: &str, branch: &str| er_engine::git::Worktree {
                path: path.into(),
                branch: branch.into(),
                head: "abc1234".into(),
            };
            app.overlay = Some(OverlayData::WorktreePicker {
                worktrees: vec![worktree("/src/er", "main"), worktree("/src/er-fix", "fix")],
                selected: 1,
            });
        })
    }),
    fixture("overlay_directory", 90, 20, || {
        with(|app| {
            let entry = |name: &str, is_dir, is_git_repo| DirEntry {
                name: name.into(),
                is_dir,
                is_git_repo,
            };
            app.overlay = Some(OverlayData::DirectoryBrowser {
                current_path: "/src".into(),
                entries: vec![
                    entry("er", true, true),
                    entry("notes", true, false),
                    entry("todo.md", false, false),
                ],
                selected: 0,
            });
        })
    }),
    fixture("overlay_filter_history", 90, 20, || {
        with(|app| {
            app.tab_mut().filter_history = vec!["+*.rs".into(), "-*.lock".into()];
            app.open_filter_history();
        })
    }),
    fixture("overlay_config_hub", 90, 24, || {
        with(|app| {
            app.open_config_hub();
            if let Some(OverlayData::ConfigHub {
                items,
                selected,
                editing,
                ..
            }) = &mut app.overlay
            {
                let index = items
                    .iter()
                    .position(|item| matches!(item, ConfigItem::StringEdit { .. }))
                    .unwrap_or(0);
                *selected = index;
                *editing = Some(ConfigEditState {
                    item_index: index,
                    buffer: "vim".into(),
                    cursor_pos: 3,
                });
            }
        })
    }),
    fixture("overlay_export", 90, 20, || with(App::open_export_picker)),
    fixture("overlay_flow_progress", 90, 20, || {
        with(|app| {
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
            app.overlay = Some(OverlayData::ReviewFlowProgress {
                er_dir: "/er/bucket".into(),
                flow: Box::new(flow),
                selected: 0,
                detail_scroll: 0,
            });
        })
    }),
    fixture("overlay_flow_results", 100, 24, || {
        with(|app| {
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
            app.overlay = Some(OverlayData::ReviewFlowResults {
                results: ReviewFlowResults {
                    er_dir: "/er/bucket".into(),
                    run_id: "run-1".into(),
                    diff_hash: "hash".into(),
                    findings: vec![
                        finding("f1", "Audience read early", false),
                        finding("f2", "Empty token", true),
                    ],
                },
                selected: 0,
                selected_ids: ["f1".to_string()].into(),
                detail_scroll: 0,
            });
        })
    }),
    fixture("input_search", 100, 10, || {
        with(|app| {
            app.input_mode = InputMode::Search;
            app.tab_mut().search_query = "auth".into();
        })
    }),
    fixture("input_comment", 100, 12, || {
        with(|app| {
            app.input_mode = InputMode::Comment;
            let tab = app.tab_mut();
            tab.comment_file = "src/auth.rs".into();
            tab.comment_line_num = Some(2);
            tab.comment_type = er_engine::ai::CommentType::Question;
            tab.comment_textarea = vec!["Why read the audience first?".to_string()].into();
        })
    }),
    fixture("input_confirm", 100, 10, || {
        with(|app| {
            app.input_mode = InputMode::Confirm(ConfirmAction::DeleteWatchedFile {
                path: ".env.local".into(),
            });
        })
    }),
    fixture("input_filter", 100, 10, || {
        with(|app| {
            app.input_mode = InputMode::Filter;
            app.tab_mut().filter_input = "+*.rs".into();
        })
    }),
    fixture("input_commit", 100, 10, || {
        with(|app| {
            app.input_mode = InputMode::Commit;
            app.tab_mut().commit_input = "Validate the token audience".into();
        })
    }),
    fixture("input_remote_url", 100, 10, || {
        with(|app| {
            app.input_mode = InputMode::RemoteUrl;
            app.remote_url_input = "https://github.com/o/r/pull/1".into();
        })
    }),
    fixture("input_base_branch", 100, 10, || {
        with(|app| {
            app.input_mode = InputMode::BaseBranch;
            app.remote_url_input = "develop".into();
        })
    }),
];

/// The panels, the diff modes, the split diff and the watched files.
pub const PANES: &[Fixture] = &[
    fixture("pane_file_detail", 140, 24, || {
        with(|app| {
            with_review(app);
            let tab = app.tab_mut();
            tab.panel = Some(PanelContent::FileDetail);
            tab.focused_finding_id = Some("f1".into());
            tab.reviewed.insert("README.md".into(), "hash".into());
        })
    }),
    fixture("pane_ai_summary", 140, 24, || {
        with(|app| {
            with_review(app);
            let tab = app.tab_mut();
            tab.panel = Some(PanelContent::AiSummary);
            tab.ai.summary = Some("Reads the audience before the token check.".into());
            tab.ai.checklist = Some(
                serde_json::from_value(serde_json::json!({
                    "version": 1, "diff_hash": "hash",
                    "items": [{"id": "c1", "text": "Empty tokens are rejected", "checked": true}]
                }))
                .unwrap(),
            );
        })
    }),
    fixture("pane_pr_overview", 140, 20, || {
        with(|app| {
            let tab = app.tab_mut();
            tab.pr_number = Some(7);
            tab.panel = Some(PanelContent::PrOverview);
            tab.pr_data = Some(
                serde_json::from_value(serde_json::json!({
                    "number": 7, "title": "Validate the token audience",
                    "body": "Rejects empty tokens.", "state": "OPEN", "author": "octo",
                    "url": "https://github.com/o/r/pull/7", "base_branch": "main",
                    "head_branch": "feature",
                    "checks": [{"name": "test", "status": "COMPLETED", "conclusion": "SUCCESS"}],
                    "reviewers": [{"login": "hubot", "state": "APPROVED"}]
                }))
                .unwrap(),
            );
        })
    }),
    fixture("pane_agent_log", 140, 16, || {
        with(|app| {
            use er_engine::app::{AgentLogEntry, AgentLogSource, CommandStatus};
            let tab = app.tab_mut();
            tab.panel = Some(PanelContent::AgentLog);
            tab.command_status
                .insert("review".into(), CommandStatus::Running);
            let now = std::time::Instant::now();
            for (source, text) in [
                (AgentLogSource::Status, "started"),
                (AgentLogSource::Stdout, "reading src/auth.rs"),
                (AgentLogSource::Stderr, "warning: no tests touched"),
            ] {
                tab.agent_log.push_back(AgentLogEntry {
                    timestamp: now,
                    command_name: "review".into(),
                    source,
                    text: text.into(),
                });
            }
        })
    }),
    fixture("diff_unstaged", 100, 12, || {
        with(|app| app.tab_mut().mode = DiffMode::Unstaged)
    }),
    fixture("diff_staged", 100, 12, || {
        with(|app| app.tab_mut().mode = DiffMode::Staged)
    }),
    fixture("diff_pr", 100, 12, || {
        with(|app| {
            let tab = app.tab_mut();
            tab.pr_number = Some(7);
            tab.mode = DiffMode::PrDiff;
        })
    }),
    fixture("diff_conflicts", 100, 12, || {
        with(|app| {
            let tab = app.tab_mut();
            tab.mode = DiffMode::Conflicts;
            tab.merge_active = true;
            tab.unresolved_count = 1;
        })
    }),
    fixture("diff_history", 110, 16, || {
        let commit = er_engine::git::CommitInfo {
            hash: "a".repeat(40),
            short_hash: "aaaaaaa".into(),
            subject: "Validate the token audience".into(),
            author: "octo".into(),
            date: "2026-11-20".into(),
            relative_date: "2 days ago".into(),
            file_count: 2,
            adds: 3,
            dels: 1,
            is_merge: false,
            diffstat_known: true,
        };
        let git = FakeGit::default()
            .with_log_range(Ok(vec![commit]))
            .with_diff_commit(Ok(RAW_DIFF.into()));
        let mut app = App::new_for_test_with_ports(Vec::new(), Ports::with_git(Arc::new(git)));
        app.tab_mut().set_mode(DiffMode::History);
        app
    }),
    fixture("diff_tour", 110, 18, || {
        with(|app| {
            let tab = app.tab_mut();
            tab.ai.tour = Some(
                serde_json::from_value(serde_json::json!({
                    "version": 1, "diff_hash": "hash", "created_at": "", "title": "Audience",
                    "overview": "", "pillars": [{
                        "id": "p1", "title": "Token check", "order": 0, "importance": 3,
                        "description": "Where the audience is read.",
                        "files": [{"path": "src/auth.rs"}]
                    }]
                }))
                .unwrap(),
            );
            tab.rebuild_tour_state();
            tab.mode = DiffMode::Tour;
            tab.reviewed.insert("src/auth.rs".into(), "hash".into());
        })
    }),
    fixture("diff_split", 120, 14, || {
        with(|app| app.config.display.split_diff = true)
    }),
    fixture("watched_snapshot", 100, 16, || {
        let snapshot = "diff --git a/.env.local b/.env.local\n--- a/.env.local\n+++ b/.env.local\n@@ -1,1 +1,2 @@\n KEY=1\n+TOKEN=abc\n";
        let git = FakeGit::default().with_diff_watched_file_snapshot(Ok(Some(snapshot.into())));
        let mut app = App::new_for_test_with_ports(
            er_engine::git::parse_diff(RAW_DIFF),
            Ports::with_git(Arc::new(git)),
        );
        let tab = app.tab_mut();
        tab.watched_config.diff_mode = "snapshot".into();
        tab.show_watched = true;
        tab.watched_files = vec![er_engine::git::WatchedFile {
            path: ".env.local".into(),
            modified: std::time::SystemTime::now() - std::time::Duration::from_hours(48),
            size: 42,
        }];
        tab.selected_watched = Some(0);
        app
    }),
    fixture("diff_hidden", 100, 16, || {
        let snapshot = "diff --git a/.env.local b/.env.local\n--- a/.env.local\n+++ b/.env.local\n@@ -1,1 +1,2 @@\n KEY=1\n+TOKEN=abc\n";
        let git = FakeGit::default().with_diff_watched_file_snapshot(Ok(Some(snapshot.into())));
        let mut app = App::new_for_test_with_ports(
            er_engine::git::parse_diff(RAW_DIFF),
            Ports::with_git(Arc::new(git)),
        );
        // What `set_mode(Hidden)` leaves, without its global config read and
        // watched-file scan: the regular diff cleared, only watched files.
        let tab = app.tab_mut();
        tab.mode = DiffMode::Hidden;
        tab.files.clear();
        tab.watched_config.paths = vec![".env*".into()];
        tab.watched_config.diff_mode = "snapshot".into();
        tab.show_watched = true;
        let age = |hours| std::time::SystemTime::now() - std::time::Duration::from_hours(hours);
        tab.watched_files = vec![
            er_engine::git::WatchedFile {
                path: ".env.local".into(),
                modified: age(48),
                size: 42,
            },
            er_engine::git::WatchedFile {
                path: ".env.test".into(),
                modified: age(72),
                size: 17,
            },
        ];
        tab.watched_not_ignored = vec![".env.test".into()];
        tab.selected_watched = Some(0);
        app
    }),
];

/// A review of `src/auth.rs` with two findings and a question on line 3.
fn with_review(app: &mut App) {
    seed_review(&mut app.tab_mut().ai);
}

/// [`with_review`]'s review and question, on either path's AI state.
pub(crate) fn seed_review(ai: &mut er_engine::ai::AiState) {
    ai.review = Some(
        serde_json::from_value(serde_json::json!({
            "version": 1, "diff_hash": "hash",
            "files": {"src/auth.rs": {"risk": "high", "summary": "Token check", "findings": [
                {"id": "f1", "severity": "high", "title": "Audience read early",
                 "description": "Read before validation.", "hunk_index": 0, "line_start": 2},
                {"id": "f2", "severity": "low", "title": "Empty token", "hunk_index": 0}
            ]}}
        }))
        .unwrap(),
    );
    ai.questions = Some(
        serde_json::from_value(serde_json::json!({
            "version": 1, "diff_hash": "hash",
            "questions": [{"id": "q1", "file": "src/auth.rs", "hunk_index": 0,
                           "line_start": 3, "text": "Why compare to er?"}]
        }))
        .unwrap(),
    );
}

/// Configures one provider with two models, the first with effort levels.
fn model_with_efforts(app: &mut App) -> (String, String) {
    providers_with_efforts(&mut app.config)
}

/// [`model_with_efforts`]'s provider, on either path's config.
pub(crate) fn providers_with_efforts(config: &mut er_engine::config::ErConfig) -> (String, String) {
    use er_engine::config::{AiModelConfig, AiProviderConfig};
    let model = |id: &str, efforts: &[&str]| AiModelConfig {
        id: id.into(),
        label: Some(id.to_uppercase()),
        effort_levels: efforts.iter().map(|e| (*e).to_string()).collect(),
        ..AiModelConfig::default()
    };
    config.ai_hub.providers.insert(
        "claude".into(),
        AiProviderConfig {
            label: Some("Claude".into()),
            command: "claude".into(),
            args: Vec::new(),
            family: None,
            models_command: Vec::new(),
            models: vec![model("opus", &["low", "high"]), model("sonnet", &[])],
            removed_catalog_models: Vec::new(),
        },
    );
    ("claude".into(), "opus".into())
}

fn add_package(app: &mut App) {
    package_web(&mut app.config);
}

/// [`add_package`]'s package, on either path's config.
pub(crate) fn package_web(config: &mut er_engine::config::ErConfig) {
    config.packages.items.insert(
        "web".into(),
        PackageConfig {
            label: Some("Web".into()),
            test: Some("pnpm test".into()),
            lint: Some("pnpm lint".into()),
            typecheck: None,
            security: None,
        },
    );
}

#[test]
fn chrome_overlays_and_input_modes_match_their_goldens() {
    check(CHROME);
}

#[test]
fn panes_and_diff_modes_match_their_goldens() {
    check(PANES);
}

#[test]
#[ignore = "prints frame times for the PR; run with --ignored --nocapture"]
fn frame_times() {
    print_frame_times(CHROME);
    print_frame_times(PANES);
}
