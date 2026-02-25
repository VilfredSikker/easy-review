mod file_tree;
mod diff_view;
mod ai_panel;
mod ai_review_view;
pub mod highlight;
mod overlay;
mod settings;
mod status_bar;
mod styles;
mod utils;

use crate::ai::ViewMode;
use crate::app::{App, OverlayData};
use highlight::Highlighter;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

/// Render the entire UI
pub fn draw(f: &mut Frame, app: &App, hl: &mut Highlighter) {
    let top_height = status_bar::top_bar_height(app, f.area().width);

    let bottom_height = status_bar::bottom_bar_height(app, f.area().width);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_height),   // top bar (dynamic rows)
            Constraint::Min(1),              // main content
            Constraint::Length(bottom_height), // bottom bar (dynamic rows)
        ])
        .split(f.area());

    // Top bar
    status_bar::render_top_bar(f, outer[0], app);

    // Main content — layout depends on view mode
    match app.tab().ai.view_mode {
        ViewMode::AiReview => {
            // Full-screen AI review replaces file tree + diff
            ai_review_view::render(f, outer[1], app);
        }
        ViewMode::SidePanel => {
            if outer[1].width < 80 {
                // Narrow terminal: fall back to 2-column layout (file tree + diff)
                let main_area = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(32),
                        Constraint::Min(1),
                    ])
                    .split(outer[1]);

                file_tree::render(f, main_area[0], app);
                diff_view::render(f, main_area[1], app, hl);
            } else {
                // Three columns: file tree + diff (2/3) + AI panel (1/3)
                let main_area = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(32),  // file tree
                        Constraint::Fill(2),     // diff view (2/3 of remaining)
                        Constraint::Fill(1),     // AI panel (1/3 of remaining)
                    ])
                    .split(outer[1]);

                file_tree::render(f, main_area[0], app);
                diff_view::render(f, main_area[1], app, hl);
                ai_panel::render(f, main_area[2], app);
            }
        }
        _ => {
            // Default & Overlay: file tree + diff view
            let main_area = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(32), // file tree width
                    Constraint::Min(1),    // diff view
                ])
                .split(outer[1]);

            file_tree::render(f, main_area[0], app);
            diff_view::render(f, main_area[1], app, hl);
        }
    }

    // Bottom status bar
    status_bar::render_bottom_bar(f, outer[2], app);

    // Watch notification overlay
    if let Some(ref msg) = app.watch_message {
        status_bar::render_watch_notification(f, f.area(), msg);
    }

    // Popup overlay (worktree picker, directory browser, settings)
    if let Some(ref overlay_data) = app.overlay {
        match overlay_data {
            OverlayData::Settings { selected, .. } => {
                settings::render_settings(f, f.area(), app, *selected);
            }
            _ => {
                overlay::render_overlay(f, f.area(), overlay_data);
            }
        }
    }
}
