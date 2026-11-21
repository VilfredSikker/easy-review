//! Golden frames: each fixture draws a whole screen to `TestBackend` and
//! compares it with a file under `goldens/`. A file holds the rows as text and
//! a digest of every cell's style, so a colour or highlight change fails as
//! surely as a text change. `ER_UPDATE_GOLDENS=1` rewrites the files.

use crate::ui;
use er_engine::app::{App, ConfigEditState, ConfirmAction, DirEntry, InputMode, OverlayData};
use er_engine::config::{ConfigItem, PackageConfig};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use std::time::Instant;

pub struct Fixture {
    pub name: &'static str,
    pub width: u16,
    pub height: u16,
    pub build: fn() -> App,
}

pub fn draw(app: &App, width: u16, height: u16, hl: &mut ui::highlight::Highlighter) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui::draw(f, app, hl)).unwrap();
    terminal.backend().buffer().clone()
}

fn golden_text(buf: &Buffer) -> String {
    let mut out = String::new();
    for row in buf.content.chunks(usize::from(buf.area.width)) {
        let line: String = row.iter().map(|cell| cell.symbol()).collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    // FNV-1a over each cell's style, in cell order.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for cell in &buf.content {
        for byte in format!("{:?}", cell.style()).bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
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

const RAW_DIFF: &str = "\
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

/// Configures one provider with two models, the first with effort levels.
fn model_with_efforts(app: &mut App) -> (String, String) {
    use er_engine::config::{AiModelConfig, AiProviderConfig};
    let model = |id: &str, efforts: &[&str]| AiModelConfig {
        id: id.into(),
        label: Some(id.to_uppercase()),
        effort_levels: efforts.iter().map(|e| (*e).to_string()).collect(),
        ..AiModelConfig::default()
    };
    app.config.ai_hub.providers.insert(
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
    app.config.packages.items.insert(
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
#[ignore = "prints frame times for the PR; run with --ignored --nocapture"]
fn frame_times() {
    print_frame_times(CHROME);
}
