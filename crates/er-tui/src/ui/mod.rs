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

use crate::screen::view::OverlayData;
use crate::screen::Screen;
use highlight::Highlighter;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

/// Render the entire UI
pub fn draw(f: &mut Frame, screen: &Screen, hl: &mut Highlighter) {
    themes::set_theme_by_name(screen.chrome.theme);
    let top_height = status_bar::top_bar_height(screen, f.area().width);

    let bottom_height = status_bar::bottom_bar_height(screen, f.area().width);

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
    status_bar::render_top_bar(f, outer[0], screen);

    // Main content — layout depends on mode and panel state
    let tab = &screen.tab;

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
        file_tree::render(f, main_area[0], screen);
        diff_view::render(f, main_area[1], screen, hl);
        panel::render(f, main_area[2], screen);
    } else {
        // 2-col layout: file_tree + diff
        let main_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(tab.file_tree_width), Constraint::Min(1)])
            .split(outer[1]);
        file_tree::render(f, main_area[0], screen);
        if screen.chrome.split_diff {
            diff_view::render_split(f, main_area[1], screen, hl, screen.chrome.config);
        } else {
            diff_view::render(f, main_area[1], screen, hl);
        }
    }

    // Bottom status bar
    status_bar::render_bottom_bar(f, outer[2], screen);

    // Watch notification overlay. A config load failure keeps its own banner
    // once the one-time notification has cleared.
    if let Some(message) = screen.chrome.notification {
        status_bar::render_watch_notification(f, f.area(), message);
    } else if let Some(error) = screen.chrome.config_load_error {
        status_bar::render_config_error_banner(f, f.area(), error);
    }

    // Popup overlay (worktree picker, directory browser, config hub)
    if let Some(ref overlay_data) = screen.chrome.overlay {
        match overlay_data {
            OverlayData::ConfigHub {
                tab,
                items,
                selected,
                editing,
                ..
            } => {
                settings::render_config_hub(
                    f,
                    f.area(),
                    screen.chrome.config,
                    *tab,
                    items,
                    *selected,
                    editing.as_ref(),
                );
            }
            _ => {
                overlay::render_overlay(f, f.area(), overlay_data);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// The renderers read only the `Screen`: no module here names the App or
    /// a tab's state, or calls a port. Test modules and comments are skipped.
    #[test]
    fn renderers_name_no_app_type_and_call_no_port() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui");
        let banned = |word: &str, code: &str| match word {
            "ports." | "er_engine::app" => code.contains(word),
            _ => code
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|token| token == word),
        };
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let mut code = String::new();
            for (i, part) in source.split("#[cfg(test)]").enumerate() {
                if i > 0 && part.trim_start().starts_with("mod ") {
                    break;
                }
                code.push_str(part);
            }
            for (number, line) in code.lines().enumerate() {
                let line = line.split("//").next().unwrap_or_default();
                for word in ["er_engine::app", "App", "TabState", "ports."] {
                    if banned(word, line) {
                        found.push(format!("{}:{}: {word}", path.display(), number + 1));
                    }
                }
            }
        }
        assert!(
            found.is_empty(),
            "renderers reach past the Screen:\n{}",
            found.join("\n")
        );
    }
}
