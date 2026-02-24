use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding},
    Frame,
};

use crate::app::App;
use crate::git::FileStatus;
use super::styles;

/// Render the file tree panel (left side)
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let visible = tab.visible_files();
    let total = tab.files.len();

    let title = format!(" FILES ({}) ", total);

    let items: Vec<ListItem> = visible
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

            // File path — show just the filename, or shortened path
            let path = shorten_path(&file.path, (area.width as usize).saturating_sub(16));

            // Stats: +adds -dels
            let stats = format!("+{} -{}", file.adds, file.dels);

            let is_reviewed = tab.reviewed.contains(&file.path);

            let line_style = if is_selected {
                styles::selected_style()
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

            let line = Line::from(vec![
                Span::styled(format!(" {} ", symbol), effective_symbol_style),
                Span::styled(
                    format!("{:<width$}", path, width = (area.width as usize).saturating_sub(14)),
                    if is_selected {
                        styles::selected_style()
                    } else if is_reviewed {
                        ratatui::style::Style::default().fg(styles::DIM)
                    } else {
                        ratatui::style::Style::default().fg(styles::TEXT)
                    },
                ),
                Span::styled(
                    format!("{:>8} ", stats),
                    ratatui::style::Style::default().fg(styles::DIM),
                ),
            ]);

            ListItem::new(line).style(line_style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(title, ratatui::style::Style::default().fg(styles::MUTED)))
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
