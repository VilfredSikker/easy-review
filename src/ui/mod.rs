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

use crate::app::{App, OverlayData};
use highlight::Highlighter;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

/// Render the entire UI
pub fn draw(f: &mut Frame, app: &App, hl: &mut Highlighter) {
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

    // Main content — layout depends on panel state
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
                selected, editing, ..
            } => {
                settings::render_config_hub(f, f.area(), app, *selected, editing);
            }
            _ => {
                overlay::render_overlay(f, f.area(), overlay_data);
            }
        }
    }
}
