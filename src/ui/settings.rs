use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

use crate::app::App;
use crate::config::{self, SettingsItem};
use super::styles;

/// Render the settings overlay
pub fn render_settings(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let items = config::settings_items();

    // Calculate popup size
    let content_height = items.len() as u16 + 4; // items + save/cancel + help line + padding
    let popup_height = content_height.min(area.height.saturating_sub(6)).max(10);
    let popup_width = 50u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    let mut list_items: Vec<ListItem> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        let is_sel = idx == selected;

        match item {
            SettingsItem::SectionHeader(title) => {
                let line = Line::from(vec![
                    Span::styled(
                        format!("  {}", title),
                        ratatui::style::Style::default()
                            .fg(styles::CYAN)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                ]);
                list_items.push(
                    ListItem::new(line)
                        .style(ratatui::style::Style::default().bg(styles::PANEL)),
                );
            }
            SettingsItem::BoolToggle { label, get, .. } => {
                let value = get(&app.config);
                let marker = if is_sel { "▸ " } else { "  " };
                let checkbox = if value { "[x]" } else { "[ ]" };

                let line = Line::from(vec![
                    Span::styled(
                        marker,
                        ratatui::style::Style::default().fg(styles::CYAN),
                    ),
                    Span::styled(
                        format!("{} ", checkbox),
                        ratatui::style::Style::default().fg(if value {
                            styles::GREEN
                        } else {
                            styles::DIM
                        }),
                    ),
                    Span::styled(
                        label.as_str(),
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

                list_items.push(ListItem::new(line).style(style));
            }
            SettingsItem::NumberEdit { label, get, .. } => {
                let value = get(&app.config);
                let marker = if is_sel { "▸ " } else { "  " };

                let line = Line::from(vec![
                    Span::styled(
                        marker,
                        ratatui::style::Style::default().fg(styles::CYAN),
                    ),
                    Span::styled(
                        label.as_str(),
                        if is_sel {
                            ratatui::style::Style::default().fg(styles::BRIGHT)
                        } else {
                            ratatui::style::Style::default().fg(styles::TEXT)
                        },
                    ),
                    Span::styled(
                        format!(": {}", value),
                        ratatui::style::Style::default().fg(styles::YELLOW),
                    ),
                ]);

                let style = if is_sel {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().bg(styles::PANEL)
                };

                list_items.push(ListItem::new(line).style(style));
            }
            SettingsItem::StringDisplay { label, get } => {
                let value = get(&app.config);
                let marker = if is_sel { "▸ " } else { "  " };

                let line = Line::from(vec![
                    Span::styled(
                        marker,
                        ratatui::style::Style::default().fg(styles::CYAN),
                    ),
                    Span::styled(
                        label.as_str(),
                        ratatui::style::Style::default().fg(styles::DIM),
                    ),
                    Span::styled(
                        format!(": {}", value),
                        ratatui::style::Style::default().fg(styles::TEXT),
                    ),
                ]);

                let style = if is_sel {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().bg(styles::PANEL)
                };

                list_items.push(ListItem::new(line).style(style));
            }
        }
    }

    // Help line at the bottom
    let help_line = Line::from(vec![
        Span::styled(
            " j/k",
            ratatui::style::Style::default()
                .fg(styles::TEXT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(
            " nav  ",
            ratatui::style::Style::default().fg(styles::DIM),
        ),
        Span::styled(
            "Space/Enter",
            ratatui::style::Style::default()
                .fg(styles::TEXT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(
            " toggle  ",
            ratatui::style::Style::default().fg(styles::DIM),
        ),
        Span::styled(
            "s",
            ratatui::style::Style::default()
                .fg(styles::TEXT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(
            " save  ",
            ratatui::style::Style::default().fg(styles::DIM),
        ),
        Span::styled(
            "Esc",
            ratatui::style::Style::default()
                .fg(styles::TEXT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(
            " cancel",
            ratatui::style::Style::default().fg(styles::DIM),
        ),
    ]);
    list_items.push(
        ListItem::new(Line::from(""))
            .style(ratatui::style::Style::default().bg(styles::PANEL)),
    );
    list_items.push(
        ListItem::new(help_line)
            .style(ratatui::style::Style::default().bg(styles::PANEL)),
    );

    let block = Block::default()
        .title(Span::styled(
            " Settings ",
            ratatui::style::Style::default()
                .fg(styles::CYAN)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN))
        .style(ratatui::style::Style::default().bg(styles::PANEL));

    let list = List::new(list_items).block(block);
    f.render_widget(list, popup);
}

/// Calculate a centered rectangle within an area
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
