use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

use super::styles;
use crate::app::{App, ConfigEditState};
use crate::config::{self, ConfigItem};

/// Render the config hub overlay
pub fn render_config_hub(
    f: &mut Frame,
    area: Rect,
    app: &App,
    selected: usize,
    editing: &Option<ConfigEditState>,
) {
    let items = config::config_hub_items(&app.config);

    // Calculate popup dimensions — taller and wider than old settings overlay
    let max_height = area.height.saturating_sub(4);
    let popup_height = (items.len() as u16 + 4).min(max_height).max(12);
    let popup_width = 70u16.min(area.width.saturating_sub(4));
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    // Viewport: how many items can fit (subtract 2 for border, 2 for help lines)
    let visible_rows = popup_height.saturating_sub(4) as usize;

    // Scroll so selected item stays in view
    let scroll_offset = if selected >= visible_rows {
        selected - visible_rows + 1
    } else {
        0
    };

    let mut list_items: Vec<ListItem> = Vec::new();

    for (idx, item) in items
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_rows)
    {
        let is_sel = idx == selected;
        let is_editing_this = editing
            .as_ref()
            .map(|e| e.item_index == idx)
            .unwrap_or(false);

        match item {
            ConfigItem::SectionHeader(title) => {
                let line = Line::from(vec![Span::styled(
                    format!(" {}", title),
                    ratatui::style::Style::default()
                        .fg(styles::CYAN())
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )]);
                list_items.push(
                    ListItem::new(line).style(ratatui::style::Style::default().bg(styles::PANEL())),
                );
            }

            ConfigItem::BoolToggle {
                label,
                description,
                get,
                ..
            } => {
                let value = get(&app.config);
                let checkbox = if value { "[x]" } else { "[ ]" };
                let marker = if is_sel { "▸ " } else { "  " };

                let mut spans = vec![
                    Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                    Span::styled(
                        format!("{} ", checkbox),
                        ratatui::style::Style::default().fg(if value {
                            styles::GREEN()
                        } else {
                            styles::DIM()
                        }),
                    ),
                    Span::styled(
                        label.as_str(),
                        if is_sel {
                            ratatui::style::Style::default().fg(styles::BRIGHT())
                        } else {
                            ratatui::style::Style::default().fg(styles::TEXT())
                        },
                    ),
                ];
                if is_sel && !description.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", description),
                        ratatui::style::Style::default().fg(styles::MUTED()),
                    ));
                }
                let line = Line::from(spans);

                let style = if is_sel {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().bg(styles::PANEL())
                };
                list_items.push(ListItem::new(line).style(style));
            }

            ConfigItem::StringCycle {
                label,
                description,
                get,
                ..
            } => {
                let value = get(&app.config);
                let marker = if is_sel { "▸ " } else { "  " };

                let mut spans = vec![
                    Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                    Span::styled(
                        format!("{}: ", label),
                        if is_sel {
                            ratatui::style::Style::default().fg(styles::BRIGHT())
                        } else {
                            ratatui::style::Style::default().fg(styles::TEXT())
                        },
                    ),
                    Span::styled(value, ratatui::style::Style::default().fg(styles::YELLOW())),
                    Span::styled("  ◀▶", ratatui::style::Style::default().fg(styles::DIM())),
                ];
                if is_sel && !description.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", description),
                        ratatui::style::Style::default().fg(styles::MUTED()),
                    ));
                }
                let line = Line::from(spans);

                let style = if is_sel {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().bg(styles::PANEL())
                };
                list_items.push(ListItem::new(line).style(style));
            }

            ConfigItem::StringEdit {
                label,
                description,
                placeholder,
                get,
                ..
            } => {
                let marker = if is_sel { "▸ " } else { "  " };

                let line = if is_editing_this {
                    let edit = editing.as_ref().unwrap();
                    let buf = &edit.buffer;
                    Line::from(vec![
                        Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                        Span::styled(
                            format!("{}: ", label),
                            ratatui::style::Style::default().fg(styles::BRIGHT()),
                        ),
                        Span::styled(
                            buf.as_str(),
                            ratatui::style::Style::default().fg(styles::YELLOW()),
                        ),
                        Span::styled("█", ratatui::style::Style::default().fg(styles::CYAN())),
                    ])
                } else {
                    let value = get(&app.config);
                    let (value_span, _placeholder_used) = if value.is_empty() {
                        (
                            Span::styled(
                                placeholder.as_str(),
                                ratatui::style::Style::default().fg(styles::MUTED()),
                            ),
                            true,
                        )
                    } else {
                        (
                            Span::styled(
                                value,
                                ratatui::style::Style::default().fg(styles::YELLOW()),
                            ),
                            false,
                        )
                    };
                    let mut spans = vec![
                        Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                        Span::styled(
                            format!("{}: ", label),
                            if is_sel {
                                ratatui::style::Style::default().fg(styles::BRIGHT())
                            } else {
                                ratatui::style::Style::default().fg(styles::TEXT())
                            },
                        ),
                        value_span,
                    ];
                    if is_sel && !description.is_empty() {
                        spans.push(Span::styled(
                            format!("  {}", description),
                            ratatui::style::Style::default().fg(styles::MUTED()),
                        ));
                    }
                    Line::from(spans)
                };

                let style = if is_sel {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().bg(styles::PANEL())
                };
                list_items.push(ListItem::new(line).style(style));
            }

            ConfigItem::NumberEdit {
                label,
                description,
                get,
                ..
            } => {
                let value = get(&app.config);
                let marker = if is_sel { "▸ " } else { "  " };

                let mut spans = vec![
                    Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                    Span::styled(
                        format!("{}: ", label),
                        if is_sel {
                            ratatui::style::Style::default().fg(styles::BRIGHT())
                        } else {
                            ratatui::style::Style::default().fg(styles::TEXT())
                        },
                    ),
                    Span::styled(
                        value.to_string(),
                        ratatui::style::Style::default().fg(styles::YELLOW()),
                    ),
                    Span::styled("  ◀▶", ratatui::style::Style::default().fg(styles::DIM())),
                ];
                if is_sel && !description.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", description),
                        ratatui::style::Style::default().fg(styles::MUTED()),
                    ));
                }
                let line = Line::from(spans);

                let style = if is_sel {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().bg(styles::PANEL())
                };
                list_items.push(ListItem::new(line).style(style));
            }

            ConfigItem::ListEntry { label, .. } => {
                let marker = if is_sel { "▸ " } else { "  " };
                let mut spans = vec![
                    Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                    Span::styled("· ", ratatui::style::Style::default().fg(styles::DIM())),
                    Span::styled(
                        label.as_str(),
                        if is_sel {
                            ratatui::style::Style::default().fg(styles::BRIGHT())
                        } else {
                            ratatui::style::Style::default().fg(styles::TEXT())
                        },
                    ),
                ];
                if is_sel {
                    spans.push(Span::styled(
                        "  [d] delete",
                        ratatui::style::Style::default().fg(styles::MUTED()),
                    ));
                }
                let line = Line::from(spans);
                let style = if is_sel {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().bg(styles::PANEL())
                };
                list_items.push(ListItem::new(line).style(style));
            }

            ConfigItem::ListAdd { label, .. } => {
                let marker = if is_sel { "▸ " } else { "  " };

                let line = if is_editing_this {
                    let edit = editing.as_ref().unwrap();
                    Line::from(vec![
                        Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                        Span::styled("+ ", ratatui::style::Style::default().fg(styles::CYAN())),
                        Span::styled(
                            edit.buffer.as_str(),
                            ratatui::style::Style::default().fg(styles::YELLOW()),
                        ),
                        Span::styled("█", ratatui::style::Style::default().fg(styles::CYAN())),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                        Span::styled(
                            format!("+ {}", label),
                            ratatui::style::Style::default().fg(styles::CYAN()),
                        ),
                    ])
                };

                let style = if is_sel {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().bg(styles::PANEL())
                };
                list_items.push(ListItem::new(line).style(style));
            }
        }
    }

    // Help line
    let help_line = if editing.is_some() {
        Line::from(vec![
            Span::styled(
                " Enter",
                ratatui::style::Style::default()
                    .fg(styles::TEXT())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                " confirm  ",
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
            Span::styled(
                "Esc",
                ratatui::style::Style::default()
                    .fg(styles::TEXT())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                " cancel edit",
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " j/k",
                ratatui::style::Style::default()
                    .fg(styles::TEXT())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(" nav  ", ratatui::style::Style::default().fg(styles::DIM())),
            Span::styled(
                "Enter",
                ratatui::style::Style::default()
                    .fg(styles::TEXT())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                " edit  ",
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
            Span::styled(
                "s",
                ratatui::style::Style::default()
                    .fg(styles::TEXT())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                " save (repo)  ",
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
            Span::styled(
                "S",
                ratatui::style::Style::default()
                    .fg(styles::TEXT())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                " save (global)  ",
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
            Span::styled(
                "d",
                ratatui::style::Style::default()
                    .fg(styles::TEXT())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                " delete  ",
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
            Span::styled(
                "Esc",
                ratatui::style::Style::default()
                    .fg(styles::TEXT())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                " cancel",
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
        ])
    };

    list_items.push(
        ListItem::new(Line::from("")).style(ratatui::style::Style::default().bg(styles::PANEL())),
    );
    list_items
        .push(ListItem::new(help_line).style(ratatui::style::Style::default().bg(styles::PANEL())));

    let block = Block::default()
        .title(Span::styled(
            " Config ",
            ratatui::style::Style::default()
                .fg(styles::CYAN())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

    let list = List::new(list_items).block(block);
    f.render_widget(list, popup);
}

// Use the shared centered_rect from utils (deduplicated from overlay + settings)
use super::utils::centered_rect;
