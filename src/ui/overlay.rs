use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{DirEntry, OverlayData, Worktree};
use super::styles;

/// Render the active overlay on top of the main UI
/// Note: Settings overlay is rendered separately in ui/mod.rs since it needs App access.
pub fn render_overlay(f: &mut Frame, area: Rect, overlay: &OverlayData) {
    match overlay {
        OverlayData::WorktreePicker { worktrees, selected } => {
            render_worktree_picker(f, area, worktrees, *selected);
        }
        OverlayData::DirectoryBrowser { current_path, entries, selected } => {
            render_directory_browser(f, area, current_path, entries, *selected);
        }
        OverlayData::Settings { .. } => {
            // Handled in ui/mod.rs draw()
        }
        OverlayData::FilterHistory { history, selected, preset_count } => {
            render_filter_history(f, area, history, *selected, *preset_count);
        }
    }
}

fn render_worktree_picker(
    f: &mut Frame,
    area: Rect,
    worktrees: &[Worktree],
    selected: usize,
) {
    let popup_height = (worktrees.len() as u16 + 2).min(area.height.saturating_sub(6));
    let popup_width = 70u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    // Clear backdrop
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = worktrees
        .iter()
        .enumerate()
        .map(|(idx, wt)| {
            let is_sel = idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(
                    marker,
                    ratatui::style::Style::default().fg(styles::CYAN),
                ),
                Span::styled(
                    format!("{:<20}", wt.branch),
                    if is_sel {
                        ratatui::style::Style::default().fg(styles::BRIGHT)
                    } else {
                        ratatui::style::Style::default().fg(styles::TEXT)
                    },
                ),
                Span::styled(
                    &wt.path,
                    ratatui::style::Style::default().fg(styles::DIM),
                ),
            ]);

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL)
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(
            " WORKTREES (Enter=select, Esc=close) ",
            ratatui::style::Style::default().fg(styles::CYAN),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN))
        .style(ratatui::style::Style::default().bg(styles::PANEL));

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

fn render_directory_browser(
    f: &mut Frame,
    area: Rect,
    current_path: &str,
    entries: &[DirEntry],
    selected: usize,
) {
    let popup_height = (entries.len() as u16 + 2).min(area.height.saturating_sub(6)).max(5);
    let popup_width = 70u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    if entries.is_empty() {
        let block = Block::default()
            .title(Span::styled(
                format!(" {} ", current_path),
                ratatui::style::Style::default().fg(styles::CYAN),
            ))
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(styles::CYAN))
            .style(ratatui::style::Style::default().bg(styles::PANEL));

        let empty = Paragraph::new(Line::from(Span::styled(
            "  (empty directory)",
            ratatui::style::Style::default().fg(styles::MUTED),
        )))
        .block(block);

        f.render_widget(empty, popup);
        return;
    }

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let is_sel = idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let icon = if entry.is_git_repo || entry.is_dir {
                " "
            } else {
                "  "
            };

            let name_style = if entry.is_git_repo {
                ratatui::style::Style::default().fg(styles::GREEN)
            } else if entry.is_dir {
                ratatui::style::Style::default().fg(styles::BLUE)
            } else {
                ratatui::style::Style::default().fg(styles::TEXT)
            };

            let mut spans = vec![
                Span::styled(
                    marker,
                    ratatui::style::Style::default().fg(styles::CYAN),
                ),
                Span::styled(icon, name_style),
                Span::styled(&entry.name, if is_sel {
                    ratatui::style::Style::default().fg(styles::BRIGHT)
                } else {
                    name_style
                }),
            ];

            if entry.is_git_repo {
                spans.push(Span::styled(
                    "  [git]",
                    ratatui::style::Style::default().fg(styles::GREEN),
                ));
            } else if entry.is_dir {
                spans.push(Span::styled(
                    "/",
                    ratatui::style::Style::default().fg(styles::DIM),
                ));
            }

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL)
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    // Shorten path for title if too long (char-aware to avoid mid-codepoint slice panic)
    let max_title_width = popup_width.saturating_sub(20) as usize;
    let title_path = if current_path.chars().count() > max_title_width {
        let suffix: String = current_path.chars().rev().take(max_title_width).collect::<Vec<_>>().into_iter().rev().collect();
        format!("…{}", suffix)
    } else {
        current_path.to_string()
    };

    let block = Block::default()
        .title(Span::styled(
            format!(" {} (Enter=open, Bksp=up, Esc=close) ", title_path),
            ratatui::style::Style::default().fg(styles::CYAN),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN))
        .style(ratatui::style::Style::default().bg(styles::PANEL));

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

fn render_filter_history(
    f: &mut Frame,
    area: Rect,
    history: &[String],
    selected: usize,
    preset_count: usize,
) {
    use crate::app::filter::FILTER_PRESETS;

    let separator_lines = if !history.is_empty() { 1 } else { 0 };
    let total_rows = preset_count + separator_lines + history.len();
    let popup_height = (total_rows as u16 + 2).min(area.height.saturating_sub(6)).max(4);
    let popup_width = 60u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    let mut items: Vec<ListItem> = Vec::new();

    // Presets section
    for (idx, preset) in FILTER_PRESETS.iter().enumerate().take(preset_count) {
        let is_sel = idx == selected;
        let marker = if is_sel { "▶ " } else { "  " };

        let line = Line::from(vec![
            Span::styled(
                marker,
                ratatui::style::Style::default().fg(styles::CYAN),
            ),
            Span::styled(
                format!("{:<10}", preset.name),
                if is_sel {
                    ratatui::style::Style::default().fg(styles::BRIGHT).add_modifier(ratatui::style::Modifier::BOLD)
                } else {
                    ratatui::style::Style::default().fg(styles::BLUE).add_modifier(ratatui::style::Modifier::BOLD)
                },
            ),
            Span::styled(
                preset.expr,
                ratatui::style::Style::default().fg(styles::DIM),
            ),
        ]);

        let style = if is_sel {
            styles::selected_style()
        } else {
            ratatui::style::Style::default().bg(styles::PANEL)
        };

        items.push(ListItem::new(line).style(style));
    }

    // Separator + history section
    if !history.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "── history ──",
            ratatui::style::Style::default().fg(styles::MUTED),
        ))).style(ratatui::style::Style::default().bg(styles::PANEL)));

        for (idx, expr) in history.iter().enumerate() {
            let abs_idx = preset_count + idx;
            let is_sel = abs_idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(
                    marker,
                    ratatui::style::Style::default().fg(styles::YELLOW),
                ),
                Span::styled(
                    expr.as_str(),
                    if is_sel {
                        ratatui::style::Style::default().fg(styles::BRIGHT)
                    } else {
                        ratatui::style::Style::default().fg(styles::TEXT)
                    },
                ),
            ]);

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL)
            };

            items.push(ListItem::new(line).style(style));
        }
    }

    let block = Block::default()
        .title(Span::styled(
            " FILTERS (Enter=apply, Esc=close) ",
            ratatui::style::Style::default().fg(styles::CYAN),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN))
        .style(ratatui::style::Style::default().bg(styles::PANEL));

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

/// Calculate a centered rectangle within an area
// TODO(risk:medium): centered_rect assumes height <= r.height and width <= r.width because
// the callers pass .min(area.height.saturating_sub(6)) and .min(area.width.saturating_sub(6)).
// If the terminal is smaller than 6 rows/cols the saturating_sub(6) yields 0, so popup_height
// and popup_width are both 0. Passing height=0 to Constraint::Length is valid in Ratatui
// but the resulting Rect has height 0, and any widget rendered into it produces no output
// without panicking. The .max(N) guards in the callers (e.g. .max(5)) prevent height=0
// in most cases, but popup_width has no .max() guard, so a terminal narrower than 6 cols
// produces a zero-width popup with invisible content.
fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(r.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(r.width.saturating_sub(width) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1])[1]
}
