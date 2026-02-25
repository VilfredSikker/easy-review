use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Padding},
    Frame,
};

use crate::ai::{RiskLevel, ReviewFocus};
use crate::app::App;
use super::styles;
use super::utils::word_wrap;

/// Render the full-screen AI review (replaces file tree + diff view)
/// Shows: summary (left), checklist + review order (right)
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    // Split into two columns: summary (left) + checklist/order (right)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    render_summary_col(f, cols[0], app);
    render_checklist_col(f, cols[1], app);
}

/// Left column: overall summary + file risk overview
fn render_summary_col(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let ai_stale = tab.ai.is_stale;
    let is_focused = tab.ai.review_focus == ReviewFocus::Files;
    let cursor = tab.ai.review_cursor;
    let mut lines: Vec<Line> = Vec::new();

    let stale_tag = if ai_stale { " [stale]" } else { "" };

    // ── Summary section ──
    lines.push(Line::from(vec![
        Span::styled(
            format!(" AI Review Summary{}", stale_tag),
            ratatui::style::Style::default()
                .fg(styles::PURPLE)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    if let Some(ref summary) = tab.ai.summary {
        let max_w = area.width.saturating_sub(4) as usize;
        for line in summary.lines() {
            if line.is_empty() {
                lines.push(Line::from(""));
            } else if line.starts_with('#') {
                // Markdown headers — render as bold
                let text = line.trim_start_matches('#').trim();
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {}", text),
                        ratatui::style::Style::default()
                            .fg(styles::BRIGHT)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                ]));
            } else {
                for wrapped in word_wrap(line, max_w) {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!(" {}", wrapped),
                            ratatui::style::Style::default().fg(styles::TEXT),
                        ),
                    ]));
                }
            }
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                " No .er-summary.md found",
                ratatui::style::Style::default().fg(styles::MUTED),
            ),
        ]));
    }

    lines.push(Line::from(""));
    let risk_header_style = if is_focused {
        ratatui::style::Style::default().fg(styles::BRIGHT)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        ratatui::style::Style::default().fg(styles::BORDER)
    };
    lines.push(Line::from(vec![
        Span::styled(" ─── File Risk Overview ───", risk_header_style),
        Span::styled(
            if is_focused { "  ←→ Tab to switch" } else { "" },
            ratatui::style::Style::default().fg(styles::MUTED),
        ),
    ]));
    lines.push(Line::from(""));

    // File risk breakdown
    if let Some(ref review) = tab.ai.review {
        // Sort files by risk: high first
        let mut file_entries: Vec<_> = review.files.iter().collect();
        file_entries.sort_by(|a, b| {
            let risk_ord = |r: &RiskLevel| match r {
                RiskLevel::High => 0,
                RiskLevel::Medium => 1,
                RiskLevel::Low => 2,
                RiskLevel::Info => 3,
            };
            risk_ord(&a.1.risk).cmp(&risk_ord(&b.1.risk))
                .then_with(|| a.0.cmp(b.0))
        });

        for (idx, (path, fr)) in file_entries.iter().enumerate() {
            let is_selected = is_focused && idx == cursor;
            let per_file_stale = tab.ai.is_file_stale(path);

            let risk_style = if per_file_stale {
                styles::stale_style()
            } else {
                match fr.risk {
                    RiskLevel::High => styles::risk_high(),
                    RiskLevel::Medium => styles::risk_medium(),
                    RiskLevel::Low => styles::risk_low(),
                    RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE),
                }
            };

            let bg = if is_selected { styles::LINE_CURSOR_BG } else { styles::BG };
            let path_style = if is_selected {
                ratatui::style::Style::default().fg(styles::BRIGHT).bg(bg)
                    .add_modifier(ratatui::style::Modifier::BOLD)
            } else {
                ratatui::style::Style::default().fg(styles::BRIGHT)
            };

            let prefix = if is_selected { "▸" } else { " " };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}{} ", prefix, fr.risk.symbol()),
                    if is_selected { risk_style.bg(bg) } else { risk_style },
                ),
                Span::styled(
                    *path,
                    path_style,
                ),
                Span::styled(
                    format!("  {}", fr.summary),
                    ratatui::style::Style::default().fg(styles::DIM).bg(bg),
                ),
            ]));
        }
    }

    // Total findings count
    let total = tab.ai.total_findings();
    if total > 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} total findings across all files", total),
                ratatui::style::Style::default().fg(styles::MUTED),
            ),
        ]));
    }

    let block = Block::default()
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG))
        .padding(Padding::new(1, 1, 0, 0));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((tab.ai_panel_scroll, 0));

    f.render_widget(paragraph, area);
}

/// Right column: checklist + review order
/// Uses ai_panel_scroll for independent scrolling from the left column
fn render_checklist_col(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let is_focused = tab.ai.review_focus == ReviewFocus::Checklist;
    let cursor = tab.ai.review_cursor;
    let mut lines: Vec<Line> = Vec::new();

    // ── Checklist section ──
    let header_style = if is_focused {
        ratatui::style::Style::default()
            .fg(styles::CYAN)
            .add_modifier(ratatui::style::Modifier::BOLD | ratatui::style::Modifier::UNDERLINED)
    } else {
        ratatui::style::Style::default()
            .fg(styles::CYAN)
            .add_modifier(ratatui::style::Modifier::BOLD)
    };
    lines.push(Line::from(vec![
        Span::styled(" Review Checklist", header_style),
        Span::styled(
            if is_focused { "  ←→ Tab to switch" } else { "" },
            ratatui::style::Style::default().fg(styles::MUTED),
        ),
    ]));
    lines.push(Line::from(""));

    if let Some(ref checklist) = tab.ai.checklist {
        for (idx, item) in checklist.items.iter().enumerate() {
            let is_selected = is_focused && idx == cursor;
            let bg = if is_selected { styles::LINE_CURSOR_BG } else { styles::SURFACE };

            let check = if item.checked { "✓" } else { "○" };
            let check_style = if item.checked {
                ratatui::style::Style::default().fg(styles::GREEN).bg(bg)
            } else {
                ratatui::style::Style::default().fg(styles::DIM).bg(bg)
            };

            let prefix = if is_selected { "▸" } else { " " };

            let text_style = if is_selected {
                ratatui::style::Style::default().fg(styles::BRIGHT).bg(bg)
                    .add_modifier(ratatui::style::Modifier::BOLD)
            } else if item.checked {
                ratatui::style::Style::default().fg(styles::MUTED)
            } else {
                ratatui::style::Style::default().fg(styles::TEXT)
            };

            lines.push(Line::from(vec![
                Span::styled(format!("{}{} ", prefix, check), check_style),
                Span::styled(&item.text, text_style),
            ]));

            // Category + related files
            let mut meta_parts: Vec<String> = Vec::new();
            if !item.category.is_empty() {
                meta_parts.push(item.category.clone());
            }
            if !item.related_files.is_empty() {
                let files: Vec<&str> = item.related_files.iter()
                    .map(|f| f.rsplit('/').next().unwrap_or(f.as_str()))
                    .collect();
                meta_parts.push(files.join(", "));
            }
            if !meta_parts.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("   {}", meta_parts.join(" · ")),
                        ratatui::style::Style::default().fg(styles::MUTED).bg(bg),
                    ),
                ]));
            }

            lines.push(Line::from(""));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                " No .er-checklist.json found",
                ratatui::style::Style::default().fg(styles::MUTED),
            ),
        ]));
        lines.push(Line::from(""));
    }

    // ── Review Order section ──
    lines.push(Line::from(vec![
        Span::styled(
            " ─── Suggested Review Order ───",
            ratatui::style::Style::default().fg(styles::BORDER),
        ),
    ]));
    lines.push(Line::from(""));

    if let Some(ref order) = tab.ai.order {
        let mut current_group = String::new();
        for (idx, entry) in order.order.iter().enumerate() {
            // Group header when group changes
            if entry.group != current_group {
                current_group = entry.group.clone();
                let group_label = order.groups.get(&entry.group)
                    .map(|g| g.label.as_str())
                    .unwrap_or(&entry.group);
                let group_color = order.groups.get(&entry.group)
                    .map(|g| match g.color.as_str() {
                        "red" => styles::RED,
                        "green" => styles::GREEN,
                        "blue" => styles::BLUE,
                        "yellow" => styles::YELLOW,
                        "purple" => styles::PURPLE,
                        "cyan" => styles::CYAN,
                        _ => styles::DIM,
                    })
                    .unwrap_or(styles::DIM);

                if idx > 0 {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" ■ {}", group_label),
                        ratatui::style::Style::default()
                            .fg(group_color)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                ]));
            }

            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {}. ", idx + 1),
                    ratatui::style::Style::default().fg(styles::DIM),
                ),
                Span::styled(
                    &entry.path,
                    ratatui::style::Style::default().fg(styles::TEXT),
                ),
                Span::styled(
                    format!("  {}", entry.reason),
                    ratatui::style::Style::default().fg(styles::MUTED),
                ),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                " No .er-order.json found",
                ratatui::style::Style::default().fg(styles::MUTED),
            ),
        ]));
    }

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER))
        .style(ratatui::style::Style::default().bg(styles::SURFACE))
        .padding(Padding::new(0, 1, 0, 0));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((tab.ai_panel_scroll, 0));

    f.render_widget(paragraph, area);
}

