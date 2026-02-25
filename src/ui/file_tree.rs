use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding},
    Frame,
};

use crate::ai::{RiskLevel, ViewMode};
use crate::app::App;
use crate::git::FileStatus;
use super::styles;

/// Render the file tree panel (left side)
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let visible = tab.visible_files();
    let total = tab.files.len();
    let in_overlay = matches!(tab.ai.view_mode, ViewMode::Overlay | ViewMode::SidePanel);
    let ai_stale = tab.ai.is_stale;

    let stale_count = tab.ai.stale_files.len();
    let title = if in_overlay && tab.ai.has_data() {
        let findings = tab.ai.total_findings();
        if ai_stale && stale_count > 0 {
            format!(" FILES ({}) ⚠ {} findings · {} stale ", total, findings, stale_count)
        } else if ai_stale {
            format!(" FILES ({}) ⚠ {} findings [stale] ", total, findings)
        } else {
            format!(" FILES ({}) · {} findings ", total, findings)
        }
    } else {
        format!(" FILES ({}) ", total)
    };

    // Virtualized rendering: find which position the selected file is in the visible list,
    // then only render items in the viewport window
    let viewport_height = area.height.saturating_sub(1) as usize; // -1 for border/title
    let selected_pos = visible.iter().position(|(i, _)| *i == tab.selected_file).unwrap_or(0);

    // Calculate file_scroll to keep selection visible
    // We compute the scroll position based on the selected file's position in visible list
    let file_scroll = if visible.len() <= viewport_height {
        0 // Everything fits, no scroll needed
    } else if selected_pos < viewport_height / 2 {
        0 // Near the top
    } else if selected_pos > visible.len().saturating_sub(viewport_height / 2) {
        visible.len().saturating_sub(viewport_height) // Near the bottom
    } else {
        selected_pos.saturating_sub(viewport_height / 2) // Center the selection
    };

    let viewport_end = (file_scroll + viewport_height).min(visible.len());
    let viewport_slice = &visible[file_scroll..viewport_end];

    let items: Vec<ListItem> = viewport_slice
        .iter()
        .map(|(idx, file)| {
            let is_selected = *idx == tab.selected_file;

            // Status symbol with color
            let (symbol, symbol_style) = match &file.status {
                FileStatus::Added => ("+", styles::status_added()),
                FileStatus::Deleted => ("-", styles::status_deleted()),
                FileStatus::Modified => ("~", styles::status_modified()),
                FileStatus::Renamed(_) => ("R", styles::status_modified()),
                FileStatus::Copied(_) => ("C", styles::status_modified()),
            };

            // Risk dot (only in overlay mode with AI data)
            let risk_dot = if in_overlay {
                if let Some(fr) = tab.ai.file_review(&file.path) {
                    let file_stale = tab.ai.is_file_stale(&file.path);
                    let dot_style = if file_stale {
                        styles::stale_style()
                    } else {
                        match fr.risk {
                            RiskLevel::High => styles::risk_high(),
                            RiskLevel::Medium => styles::risk_medium(),
                            RiskLevel::Low => styles::risk_low(),
                            RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE),
                        }
                    };
                    Some(Span::styled(
                        format!("{} ", fr.risk.symbol()),
                        dot_style,
                    ))
                } else {
                    // No AI data for this file — show empty dot
                    Some(Span::styled(
                        "  ",
                        ratatui::style::Style::default(),
                    ))
                }
            } else {
                None
            };

            // Adjust path width to account for risk dot
            let extra_width = if risk_dot.is_some() { 2 } else { 0 };
            let path = shorten_path(
                &file.path,
                (area.width as usize).saturating_sub(16 + extra_width),
            );

            // Stats: +adds -dels
            let stats = format!("+{} -{}", file.adds, file.dels);

            let is_reviewed = tab.reviewed.contains(&file.path);
            let is_compacted = file.compacted;

            let line_style = if is_selected {
                styles::selected_style()
            } else if is_compacted {
                ratatui::style::Style::default().fg(styles::DIM).bg(styles::SURFACE)
            } else if is_reviewed {
                ratatui::style::Style::default().fg(styles::DIM).bg(styles::SURFACE)
            } else {
                styles::surface_style()
            };

            // Dim the symbol if reviewed (unless selected)
            let effective_symbol_style = if is_reviewed && !is_selected {
                ratatui::style::Style::default().fg(styles::DIM)
            } else {
                symbol_style
            };

            let path_width = (area.width as usize).saturating_sub(14 + extra_width).max(1);

            let mut spans = vec![
                Span::styled(format!(" {} ", symbol), effective_symbol_style),
            ];

            // Insert risk dot after status symbol
            if let Some(dot) = risk_dot {
                spans.push(dot);
            }

            spans.push(Span::styled(
                format!("{:<width$}", path, width = path_width),
                if is_selected {
                    styles::selected_style()
                } else if is_reviewed {
                    ratatui::style::Style::default().fg(styles::DIM)
                } else {
                    ratatui::style::Style::default().fg(styles::TEXT)
                },
            ));
            if area.width > 24 {
                spans.push(Span::styled(
                    format!("{:>8} ", stats),
                    ratatui::style::Style::default().fg(styles::DIM),
                ));
            }

            ListItem::new(Line::from(spans)).style(line_style)
        })
        .collect();

    let title_style = if in_overlay && tab.ai.has_data() && !ai_stale {
        ratatui::style::Style::default().fg(styles::PURPLE)
    } else if ai_stale {
        styles::stale_style()
    } else {
        ratatui::style::Style::default().fg(styles::MUTED)
    };

    let block = Block::default()
        .title(Span::styled(title, title_style))
        .borders(Borders::RIGHT)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER))
        .style(ratatui::style::Style::default().bg(styles::SURFACE))
        .padding(Padding::new(0, 0, 0, 0));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Shorten a file path to fit within max_width
fn shorten_path(path: &str, max_width: usize) -> String {
    if path.len() <= max_width {
        return path.to_string();
    }

    // Try to show just the filename
    if let Some(name) = path.rsplit('/').next() {
        if name.len() <= max_width {
            let remaining = max_width.saturating_sub(name.len() + 4);
            if remaining > 0 {
                // Show partial directory
                let dir_part: String = path[..path.len() - name.len() - 1]
                    .chars()
                    .take(remaining)
                    .collect();
                return format!("{}…/{}", dir_part, name);
            }
            return name.to_string();
        }
        // Truncate the filename itself
        let truncated: String = name.chars().take(max_width.saturating_sub(1)).collect();
        return format!("{}…", truncated);
    }

    let truncated: String = path.chars().take(max_width.saturating_sub(1)).collect();
    format!("{}…", truncated)
}

#[cfg(test)]
mod tests {
    use super::shorten_path;

    #[test]
    fn path_shorter_than_max_width_returned_as_is() {
        assert_eq!(shorten_path("src/main.rs", 30), "src/main.rs");
    }

    #[test]
    fn path_equal_to_max_width_returned_as_is() {
        assert_eq!(shorten_path("src/main.rs", 11), "src/main.rs");
    }

    #[test]
    fn long_path_filename_fits_directory_truncated() {
        // len("src/very/long/nested/path/main.rs") = 34 > 20
        // filename = "main.rs" (7), remaining = 20 - (7+4) = 9
        // dir_part = first 9 chars of "src/very/long/nested/path" = "src/very/"
        assert_eq!(
            shorten_path("src/very/long/nested/path/main.rs", 20),
            "src/very/…/main.rs"
        );
    }

    #[test]
    fn path_with_no_directory_returned_as_is() {
        assert_eq!(shorten_path("README.md", 30), "README.md");
    }

    #[test]
    fn filename_longer_than_max_width_truncated_with_ellipsis() {
        // len("very_long_filename_here.rs") = 26 > 10
        // name = "very_long_filename_here.rs" (no '/'), name.len() 26 > 10
        // truncated = first 9 chars = "very_long", result = "very_long…"
        assert_eq!(shorten_path("very_long_filename_here.rs", 10), "very_long…");
    }

    #[test]
    fn max_width_zero_does_not_panic() {
        // len("src/main.rs") = 11 > 0
        // name = "main.rs" (7), 7 > 0, so truncate: take(0) = "", result = "…"
        assert_eq!(shorten_path("src/main.rs", 0), "…");
    }

    #[test]
    fn single_component_deep_path_fits_max_width() {
        // len("a/b/c/d/e/f.rs") = 14 > 10
        // filename = "f.rs" (4), remaining = 10 - (4+4) = 2
        // dir_part = first 2 chars of "a/b/c/d/e" = "a/"
        assert_eq!(shorten_path("a/b/c/d/e/f.rs", 10), "a/…/f.rs");
    }

    #[test]
    fn exact_boundary_remaining_zero_returns_filename_only() {
        // len("some/dir/main.rs") = 16 > 11
        // filename = "main.rs" (7), remaining = 11 - (7+4) = 0
        // remaining is not > 0, so returns filename only
        assert_eq!(shorten_path("some/dir/main.rs", 11), "main.rs");
    }
}
