mod diff_view;
mod file_tree;
pub mod highlight;
mod overlay;
pub mod panel;
mod settings;
mod status_bar;
mod styles;
pub mod themes;
mod utils;

use er_engine::app::{App, OverlayData};
use highlight::Highlighter;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

/// Render the entire UI
pub fn draw(f: &mut Frame, app: &App, hl: &mut Highlighter) {
    themes::set_theme_by_name(&app.config.display.theme);
    let top_height = status_bar::top_bar_height(app, f.area().width);

    let bottom_height = status_bar::bottom_bar_height(app, f.area().width);

    let total_height = f.area().height;
    let min_content: u16 = 3;
    let mut top = top_height;
    let mut bottom = bottom_height;
    if top + bottom + min_content > total_height {
        bottom = bottom.min(1);
        if top + bottom + min_content > total_height {
            top = top.min(1);
        }
    }
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top),    // top bar (dynamic rows)
            Constraint::Min(1),         // main content
            Constraint::Length(bottom), // bottom bar (dynamic rows)
        ])
        .split(f.area());

    // Top bar
    status_bar::render_top_bar(f, outer[0], app);

    // Main content — layout depends on mode and panel state
    let tab = app.tab();

    if tab.panel.is_some() && outer[1].width >= (tab.file_tree_width + tab.panel_width + 20) {
        // 3-col layout: file_tree + diff + panel
        let main_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(tab.file_tree_width),
                Constraint::Min(20),
                Constraint::Length(tab.panel_width),
            ])
            .split(outer[1]);
        file_tree::render(f, main_area[0], app);
        diff_view::render(f, main_area[1], app, hl);
        panel::render(f, main_area[2], app);
    } else {
        // 2-col layout: file_tree + diff
        let main_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(tab.file_tree_width), Constraint::Min(1)])
            .split(outer[1]);
        file_tree::render(f, main_area[0], app);
        if app.split_diff_active(&app.config) {
            diff_view::render_split(f, main_area[1], app, hl, &app.config);
        } else {
            diff_view::render(f, main_area[1], app, hl);
        }
    }

    // Bottom status bar
    status_bar::render_bottom_bar(f, outer[2], app);

    // Watch notification overlay
    if let Some(ref msg) = app.watch_message {
        status_bar::render_watch_notification(f, f.area(), msg);
    }

    // Popup overlay (worktree picker, directory browser, config hub)
    if let Some(ref overlay_data) = app.overlay {
        match overlay_data {
            OverlayData::ConfigHub {
                tab,
                items,
                selected,
                editing,
                ..
            } => {
                settings::render_config_hub(f, f.area(), app, *tab, items, *selected, editing);
            }
            _ => {
                overlay::render_overlay(f, f.area(), overlay_data);
            }
        }
    }
}

#[cfg(test)]
mod draw_tests {
    use super::*;
    use er_engine::ai::PanelContent;
    use er_engine::app::Worktree;
    use er_engine::git::{DiffFile, DiffHunk, DiffLine, FileStatus, LineType};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    fn render_app(app: &App, width: u16, height: u16) -> Buffer {
        let mut hl = Highlighter::new();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|f| draw(f, app, &mut hl))
            .expect("draw whole ui");
        terminal.backend().buffer().clone()
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn text(rows: &[String]) -> String {
        rows.join("\n")
    }

    /// One modified file with a paired delete/add so both the unified and the
    /// split renderer have something to show.
    fn changed_file(path: &str) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks: vec![DiffHunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                lines: vec![
                    DiffLine {
                        line_type: LineType::Delete,
                        content: "old_value".to_string(),
                        old_num: Some(1),
                        new_num: None,
                    },
                    DiffLine {
                        line_type: LineType::Add,
                        content: "new_value".to_string(),
                        old_num: None,
                        new_num: Some(1),
                    },
                ],
            }],
            adds: 1,
            dels: 1,
            compacted: false,
            raw_hunk_count: 1,
        }
    }

    #[test]
    fn draw_lays_out_the_file_tree_beside_the_diff() {
        let app = App::new_for_test(vec![changed_file("src/main.rs")]);
        let buf = render_app(&app, 100, 24);
        let out = text(&rows(&buf));

        assert!(out.contains("FILES"), "file tree panel is drawn:\n{out}");
        assert!(out.contains("main.rs"), "{out}");
        assert!(out.contains("old_value"), "diff content is drawn:\n{out}");
        assert!(out.contains("new_value"), "{out}");
    }

    /// The left 32 columns are the file-tree pane. Scoping tree assertions to it
    /// keeps the diff pane's own file header from satisfying them.
    fn tree_pane(row: &str) -> String {
        row.chars().take(32).collect()
    }

    /// On a terminal too short for the real bar heights, `draw` squeezes *both*
    /// bars down to one row so the content area keeps its 3-row minimum. The
    /// squeeze is only observable as position: at 60x5 the tree title must land
    /// on row 1 (top bar = 1 row) and its two file rows on rows 2 and 3 (content
    /// = 3 rows, which needs the bottom bar clamped as well). Asserting mere
    /// presence would not catch a deleted clamp — the block title still paints
    /// in a 1-row content area.
    #[test]
    fn draw_shrinks_the_bars_instead_of_the_content_on_a_short_terminal() {
        let app = App::new_for_test(vec![
            changed_file("src/first.rs"),
            changed_file("src/second.rs"),
        ]);
        let buf = render_app(&app, 60, 5);
        let rows = rows(&buf);
        let out = text(&rows);

        assert!(
            tree_pane(&rows[1]).contains("FILES"),
            "top bar squeezed to 1 row, so the tree header sits on row 1:\n{out}"
        );
        assert!(
            tree_pane(&rows[2]).contains("first.rs"),
            "row 2 is the first file row:\n{out}"
        );
        assert!(
            tree_pane(&rows[3]).contains("second.rs"),
            "…and row 3 the second — which only fits if the bottom bar is 1 row too:\n{out}"
        );
    }

    #[test]
    fn draw_overlays_the_watch_notification_when_one_is_pending() {
        let mut app = App::new_for_test(vec![changed_file("src/main.rs")]);
        app.watch_message = Some("3 files changed".to_string());

        let buf = render_app(&app, 100, 24);
        assert!(
            text(&rows(&buf)).contains("3 files changed"),
            "{}",
            text(&rows(&buf))
        );

        app.watch_message = None;
        let cleared = render_app(&app, 100, 24);
        assert!(
            !text(&rows(&cleared)).contains("3 files changed"),
            "the notification disappears once cleared"
        );
    }

    #[test]
    fn draw_routes_a_popup_overlay_to_the_overlay_renderer() {
        let mut app = App::new_for_test(vec![changed_file("src/main.rs")]);
        app.overlay = Some(OverlayData::WorktreePicker {
            worktrees: vec![Worktree {
                path: "/repos/topic".to_string(),
                branch: "topic-branch".to_string(),
            }],
            selected: 0,
        });

        let out = text(&rows(&render_app(&app, 100, 24)));
        assert!(out.contains("WORKTREES"), "{out}");
        assert!(out.contains("topic-branch"), "{out}");
    }

    /// ConfigHub is the one overlay `render_overlay` does not handle — `draw`
    /// must send it to the settings renderer instead.
    #[test]
    fn draw_routes_the_config_hub_overlay_to_the_settings_renderer() {
        let mut app = App::new_for_test(vec![changed_file("src/main.rs")]);
        app.open_config_hub();

        let out = text(&rows(&render_app(&app, 100, 24)));
        assert!(
            out.contains("Config"),
            "the settings overlay title is drawn:\n{out}"
        );
        assert!(!out.contains("WORKTREES"), "{out}");
    }

    #[test]
    fn draw_adds_the_side_panel_only_when_the_terminal_is_wide_enough() {
        let mut app = App::new_for_test(vec![changed_file("src/main.rs")]);
        app.tab_mut().panel = Some(PanelContent::FileDetail);

        // file_tree (32) + panel (40) + 20 = 92 columns needed for three columns.
        let wide = text(&rows(&render_app(&app, 120, 24)));
        assert!(
            wide.contains("[File]"),
            "side panel is drawn at 120 cols:\n{wide}"
        );

        let narrow = text(&rows(&render_app(&app, 80, 24)));
        assert!(
            !narrow.contains("[File]"),
            "at 80 cols the panel is dropped for a two-column layout:\n{narrow}"
        );
    }

    /// Split view is the whole point of the flag: the removed and the added line
    /// sit on the *same* screen row instead of stacked.
    #[test]
    fn draw_pairs_deletions_with_additions_only_when_split_diff_is_enabled() {
        let mut app = App::new_for_test(vec![changed_file("src/main.rs")]);

        let unified = rows(&render_app(&app, 120, 24));
        assert!(
            unified
                .iter()
                .all(|r| !(r.contains("old_value") && r.contains("new_value"))),
            "unified view stacks the two lines:\n{}",
            text(&unified)
        );

        app.config.display.split_diff = true;
        let split = rows(&render_app(&app, 120, 24));
        assert!(
            split
                .iter()
                .any(|r| r.contains("old_value") && r.contains("new_value")),
            "split view puts old and new side by side:\n{}",
            text(&split)
        );
    }
}
