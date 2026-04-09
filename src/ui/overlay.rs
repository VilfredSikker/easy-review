use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use super::styles;
use crate::app::{DirEntry, HubItem, HubKind, OverlayData, Worktree};

/// Render the active overlay on top of the main UI
/// Note: ConfigHub overlay is rendered separately in ui/mod.rs since it needs App access.
pub fn render_overlay(f: &mut Frame, area: Rect, overlay: &OverlayData) {
    match overlay {
        OverlayData::WorktreePicker {
            worktrees,
            selected,
        } => {
            render_worktree_picker(f, area, worktrees, *selected);
        }
        OverlayData::DirectoryBrowser {
            current_path,
            entries,
            selected,
        } => {
            render_directory_browser(f, area, current_path, entries, *selected);
        }
        OverlayData::ConfigHub { .. } => {
            // Handled in ui/mod.rs draw()
        }
        OverlayData::FilterHistory {
            history,
            selected,
            preset_count,
        } => {
            render_filter_history(f, area, history, *selected, *preset_count);
        }
        OverlayData::ModalHub {
            kind,
            title,
            items,
            selected,
        } => {
            render_modal_hub(f, area, *kind, title.as_deref(), items, *selected);
        }
    }
}

fn render_worktree_picker(f: &mut Frame, area: Rect, worktrees: &[Worktree], selected: usize) {
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
                Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                Span::styled(
                    format!("{:<20}", wt.branch),
                    if is_sel {
                        ratatui::style::Style::default().fg(styles::BRIGHT())
                    } else {
                        ratatui::style::Style::default().fg(styles::TEXT())
                    },
                ),
                Span::styled(&wt.path, ratatui::style::Style::default().fg(styles::DIM())),
            ]);

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(
            " WORKTREES (Enter=select, Esc=close) ",
            ratatui::style::Style::default().fg(styles::CYAN()),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

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
    let popup_height = (entries.len() as u16 + 2)
        .min(area.height.saturating_sub(6))
        .max(5);
    let popup_width = 70u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    if entries.is_empty() {
        let block = Block::default()
            .title(Span::styled(
                format!(" {} ", current_path),
                ratatui::style::Style::default().fg(styles::CYAN()),
            ))
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
            .style(ratatui::style::Style::default().bg(styles::PANEL()));

        let empty = Paragraph::new(Line::from(Span::styled(
            "  (empty directory)",
            ratatui::style::Style::default().fg(styles::MUTED()),
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
                ratatui::style::Style::default().fg(styles::GREEN())
            } else if entry.is_dir {
                ratatui::style::Style::default().fg(styles::BLUE())
            } else {
                ratatui::style::Style::default().fg(styles::TEXT())
            };

            let mut spans = vec![
                Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                Span::styled(icon, name_style),
                Span::styled(
                    &entry.name,
                    if is_sel {
                        ratatui::style::Style::default().fg(styles::BRIGHT())
                    } else {
                        name_style
                    },
                ),
            ];

            if entry.is_git_repo {
                spans.push(Span::styled(
                    "  [git]",
                    ratatui::style::Style::default().fg(styles::GREEN()),
                ));
            } else if entry.is_dir {
                spans.push(Span::styled(
                    "/",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ));
            }

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    // Shorten path for title if too long, using char counts to avoid UTF-8 byte-boundary panics.
    let max_title_width = popup_width.saturating_sub(20) as usize;
    let title_path = {
        let char_count = current_path.chars().count();
        if char_count > max_title_width {
            let suffix: String = current_path
                .chars()
                .rev()
                .take(max_title_width)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("…{suffix}")
        } else {
            current_path.to_string()
        }
    };

    let block = Block::default()
        .title(Span::styled(
            format!(" {} (Enter=open, Bksp=up, Esc=close) ", title_path),
            ratatui::style::Style::default().fg(styles::CYAN()),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

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
    let popup_height = (total_rows as u16 + 2)
        .min(area.height.saturating_sub(6))
        .max(4);
    let popup_width = 60u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    let mut items: Vec<ListItem> = Vec::new();

    // Presets section
    for (idx, preset) in FILTER_PRESETS.iter().enumerate().take(preset_count) {
        let is_sel = idx == selected;
        let marker = if is_sel { "▶ " } else { "  " };

        let line = Line::from(vec![
            Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
            Span::styled(
                format!("{:<10}", preset.name),
                if is_sel {
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT())
                        .add_modifier(ratatui::style::Modifier::BOLD)
                } else {
                    ratatui::style::Style::default()
                        .fg(styles::BLUE())
                        .add_modifier(ratatui::style::Modifier::BOLD)
                },
            ),
            Span::styled(
                preset.expr,
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
        ]);

        let style = if is_sel {
            styles::selected_style()
        } else {
            ratatui::style::Style::default().bg(styles::PANEL())
        };

        items.push(ListItem::new(line).style(style));
    }

    // Separator + history section
    if !history.is_empty() {
        items.push(
            ListItem::new(Line::from(Span::styled(
                "── history ──",
                ratatui::style::Style::default().fg(styles::MUTED()),
            )))
            .style(ratatui::style::Style::default().bg(styles::PANEL())),
        );

        for (idx, expr) in history.iter().enumerate() {
            let abs_idx = preset_count + idx;
            let is_sel = abs_idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(
                    marker,
                    ratatui::style::Style::default().fg(styles::YELLOW()),
                ),
                Span::styled(
                    expr.as_str(),
                    if is_sel {
                        ratatui::style::Style::default().fg(styles::BRIGHT())
                    } else {
                        ratatui::style::Style::default().fg(styles::TEXT())
                    },
                ),
            ]);

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            items.push(ListItem::new(line).style(style));
        }
    }

    let block = Block::default()
        .title(Span::styled(
            " FILTERS (Enter=apply, Esc=close) ",
            ratatui::style::Style::default().fg(styles::CYAN()),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

// Use the shared centered_rect from utils (deduplicated from overlay + settings)
use super::utils::centered_rect;

fn render_modal_hub(
    f: &mut Frame,
    area: Rect,
    kind: HubKind,
    title_override: Option<&str>,
    items: &[HubItem],
    selected: usize,
) {
    // For Help hub, use wider popup to fit descriptions
    let is_help = kind == HubKind::Help;
    let popup_width = if is_help {
        70u16.min(area.width.saturating_sub(6))
    } else {
        55u16.min(area.width.saturating_sub(6))
    };
    let popup_height = (items.len() as u16 + 2)
        .min(area.height.saturating_sub(4))
        .max(5);
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    let title_color = match kind {
        HubKind::Git => styles::GREEN(),
        HubKind::Ai => styles::PURPLE(),
        HubKind::AiProvider | HubKind::AiModel => styles::PURPLE(),
        HubKind::Verify | HubKind::VerifyPackage => styles::YELLOW(),
        HubKind::Help => styles::CYAN(),
        HubKind::Open => styles::BLUE(),
        HubKind::Copy => styles::CYAN(),
    };

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            if item.is_header {
                // Section header: rendered as dimmed label
                return ListItem::new(Line::from(Span::styled(
                    &item.label,
                    ratatui::style::Style::default()
                        .fg(title_color)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )))
                .style(ratatui::style::Style::default().bg(styles::PANEL()));
            }

            let is_sel = idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let label_style = if !item.enabled {
                ratatui::style::Style::default().fg(styles::MUTED())
            } else if is_sel {
                ratatui::style::Style::default().fg(styles::BRIGHT())
            } else {
                ratatui::style::Style::default().fg(styles::TEXT())
            };

            let mut spans = vec![
                Span::styled(marker, ratatui::style::Style::default().fg(title_color)),
                Span::styled(&item.label, label_style),
            ];

            // For Help hub, show description inline after the label
            if is_help && !item.description.is_empty() {
                // Pad label to align descriptions
                let pad = 12usize.saturating_sub(item.label.len());
                spans.push(Span::raw(" ".repeat(pad)));
                spans.push(Span::styled(
                    &item.description,
                    ratatui::style::Style::default().fg(styles::DIM()),
                ));
            } else {
                // For action hubs, show hint right-aligned and description dimmed
                if !item.hint.is_empty() {
                    spans.push(Span::styled(
                        format!("  [{}]", item.hint),
                        ratatui::style::Style::default().fg(styles::DIM()),
                    ));
                }
                if !item.description.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", item.description),
                        ratatui::style::Style::default().fg(styles::MUTED()),
                    ));
                }
            }

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let close_hint = if is_help {
        "Esc=close"
    } else {
        "Enter=select, Esc=close"
    };

    let display_title = title_override.unwrap_or(kind.title());
    let block = Block::default()
        .title(Span::styled(
            format!(" {} ({}) ", display_title, close_hint),
            ratatui::style::Style::default().fg(title_color),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(title_color))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

    let list = List::new(list_items).block(block);
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(list, popup, &mut state);
}
