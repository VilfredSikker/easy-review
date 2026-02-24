use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Padding},
    Frame,
};

use crate::app::App;
use crate::git::LineType;
use super::highlight::Highlighter;
use super::styles;

/// Render the diff view panel (right side)
pub fn render(f: &mut Frame, area: Rect, app: &App, hl: &Highlighter) {
    let tab = app.tab();
    let file = match tab.selected_diff_file() {
        Some(f) => f,
        None => {
            render_empty(f, area);
            return;
        }
    };

    let title = format!(" {} ", file.path);
    let total_hunks = file.hunks.len();

    // Build diff lines
    let mut lines: Vec<Line> = Vec::new();

    // Add file header
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {} ", file.status.symbol()),
            match &file.status {
                crate::git::FileStatus::Added => styles::status_added(),
                crate::git::FileStatus::Deleted => styles::status_deleted(),
                _ => styles::status_modified(),
            },
        ),
        Span::styled(
            &file.path,
            ratatui::style::Style::default().fg(styles::BRIGHT),
        ),
        Span::styled(
            format!("  +{} -{}", file.adds, file.dels),
            ratatui::style::Style::default().fg(styles::DIM),
        ),
    ]));
    lines.push(Line::from(""));

    // Render hunks
    for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
        let is_current = hunk_idx == tab.current_hunk;

        // Hunk header
        let marker = if is_current { "▶" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", marker),
                if is_current {
                    ratatui::style::Style::default().fg(styles::CYAN).bg(styles::HUNK_BG)
                } else {
                    ratatui::style::Style::default().fg(styles::DIM).bg(styles::HUNK_BG)
                },
            ),
            Span::styled(&hunk.header, styles::hunk_header_style()),
        ]).style(styles::hunk_header_style()));

        // Hunk lines
        for diff_line in &hunk.lines {
            let old_num = diff_line
                .old_num
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());
            let new_num = diff_line
                .new_num
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());

            let (prefix, base_style) = match diff_line.line_type {
                LineType::Add => ("+", styles::add_style()),
                LineType::Delete => ("-", styles::del_style()),
                LineType::Context => (" ", styles::default_style()),
            };

            let gutter_style = match diff_line.line_type {
                LineType::Add => ratatui::style::Style::default().fg(styles::DIM).bg(styles::ADD_BG),
                LineType::Delete => ratatui::style::Style::default().fg(styles::DIM).bg(styles::DEL_BG),
                LineType::Context => ratatui::style::Style::default().fg(styles::DIM),
            };

            // Build the line: gutter + prefix + syntax-highlighted content
            let mut spans = vec![
                Span::styled(format!("{} {} │", old_num, new_num), gutter_style),
                Span::styled(prefix, base_style),
            ];

            // Syntax highlight the code content
            if diff_line.content.is_empty() {
                spans.push(Span::styled("", base_style));
            } else {
                let highlighted = hl.highlight_line(&diff_line.content, &file.path, base_style);
                spans.extend(highlighted);
            }

            lines.push(Line::from(spans).style(base_style));
        }

        // Blank line between hunks
        lines.push(Line::from(""));
    }

    let block = Block::default()
        .title(Span::styled(
            title,
            ratatui::style::Style::default().fg(styles::BRIGHT),
        ))
        .title_position(ratatui::widgets::block::Position::Top)
        .title_alignment(ratatui::layout::Alignment::Left)
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG))
        .padding(Padding::new(0, 1, 0, 0));

    // Apply both vertical and horizontal scroll
    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((tab.diff_scroll, tab.h_scroll));

    f.render_widget(paragraph, area);

    // Render hunk indicator overlay in top-right corner
    if total_hunks > 0 {
        let indicator_width = (tab.current_hunk + 1).to_string().len()
            + total_hunks.to_string().len()
            + 9;
        let indicator_area = Rect {
            x: area.x + area.width.saturating_sub(indicator_width as u16 + 1),
            y: area.y,
            width: indicator_width as u16,
            height: 1,
        };
        let indicator = Paragraph::new(Line::from(Span::styled(
            format!("Hunk {}/{}", tab.current_hunk + 1, total_hunks),
            ratatui::style::Style::default().fg(styles::MUTED),
        )));
        f.render_widget(indicator, indicator_area);
    }
}

/// Render an empty state when no file is selected
fn render_empty(f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG));

    let text = Paragraph::new(vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "  No files changed",
            ratatui::style::Style::default().fg(styles::MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Switch modes with [1] [2] [3]",
            ratatui::style::Style::default().fg(styles::DIM),
        )),
    ])
    .block(block);

    f.render_widget(text, area);
}
