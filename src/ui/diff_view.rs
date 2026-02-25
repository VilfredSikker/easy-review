use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Padding},
    Frame,
};

use crate::ai::{RiskLevel, ViewMode};
use crate::app::{App, DiffMode};
use crate::git::LineType;
use super::highlight::Highlighter;
use super::styles;

/// Threshold (total diff lines) above which viewport-based rendering is used
const VIRTUALIZE_THRESHOLD: usize = 200;

/// Threshold above which a "large file" warning is shown in the title
const LARGE_FILE_WARNING_LINES: usize = 2000;

/// Render the diff view panel (right side)
pub fn render(f: &mut Frame, area: Rect, app: &App, hl: &mut Highlighter) {
    let tab = app.tab();
    let file = match tab.selected_diff_file() {
        Some(f) => f,
        None => {
            render_empty(f, area);
            return;
        }
    };

    // Handle compacted files — show summary instead of full diff
    if file.compacted {
        render_compacted(f, area, file);
        return;
    }

    let in_overlay = tab.ai.view_mode == ViewMode::Overlay;
    let file_stale = tab.ai.is_file_stale(&file.path);

    let total_hunks = file.hunks.len();

    // Count total lines to decide rendering strategy
    let total_diff_lines: usize = file.hunks.iter().map(|h| h.lines.len()).sum();
    let use_viewport = total_diff_lines > VIRTUALIZE_THRESHOLD;

    let title = if total_diff_lines > LARGE_FILE_WARNING_LINES {
        format!(" {} \u{26a0} +{} lines ", file.path, total_diff_lines)
    } else {
        format!(" {} ", file.path)
    };

    // Viewport window parameters
    let viewport_height = area.height as usize;
    let buffer_lines = if use_viewport { 20 } else { 0 };
    let scroll = tab.diff_scroll as usize;
    let render_start = if use_viewport { scroll.saturating_sub(buffer_lines) } else { 0 };
    let render_end = if use_viewport { scroll + viewport_height + buffer_lines } else { usize::MAX };

    // Build diff lines (only within viewport window when virtualized)
    let mut lines: Vec<Line> = Vec::with_capacity(if use_viewport { viewport_height + buffer_lines * 2 } else { total_diff_lines + total_hunks * 2 + 4 });
    let mut logical_line: usize = 0;

    // File header (always rendered since it's at the top)
    let mut header_spans = vec![
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
    ];

    // Add AI risk + summary to file header in AI modes
    let show_ai_header = matches!(tab.ai.view_mode, ViewMode::Overlay | ViewMode::SidePanel);
    if show_ai_header {
        if let Some(fr) = tab.ai.file_review(&file.path) {
            let risk_style = if file_stale {
                styles::stale_style()
            } else {
                match fr.risk {
                    RiskLevel::High => styles::risk_high(),
                    RiskLevel::Medium => styles::risk_medium(),
                    RiskLevel::Low => styles::risk_low(),
                    RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE),
                }
            };
            let risk_label = match fr.risk {
                RiskLevel::High => "HIGH",
                RiskLevel::Medium => "MED",
                RiskLevel::Low => "LOW",
                RiskLevel::Info => "INFO",
            };
            header_spans.push(Span::styled("  ", ratatui::style::Style::default()));
            header_spans.push(Span::styled(
                format!("{} {}", fr.risk.symbol(), risk_label),
                risk_style,
            ));
            if !fr.risk_reason.is_empty() {
                header_spans.push(Span::styled(
                    format!(" — {}", fr.risk_reason),
                    ratatui::style::Style::default().fg(styles::DIM),
                ));
            }
        }
    }

    if logical_line >= render_start && logical_line < render_end {
        lines.push(Line::from(header_spans));
    }
    logical_line += 1;

    // Add file summary line in overlay mode
    if in_overlay {
        if let Some(fr) = tab.ai.file_review(&file.path) {
            if !fr.summary.is_empty() {
                if logical_line >= render_start && logical_line < render_end {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  \u{2139} {}", fr.summary),
                            ratatui::style::Style::default().fg(styles::MUTED),
                        ),
                    ]));
                }
                logical_line += 1;
            }
        }
    }

    if logical_line >= render_start && logical_line < render_end {
        lines.push(Line::from(""));
    }
    logical_line += 1;

    // Render hunks
    for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
        let is_current = hunk_idx == tab.current_hunk;

        // Early exit — past viewport, no need to process remaining hunks
        if use_viewport && logical_line > render_end + buffer_lines {
            break;
        }

        // Hunk header
        if logical_line >= render_start && logical_line < render_end {
            let marker = if is_current { "\u{25b6}" } else { " " };
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
        }
        logical_line += 1;

        // Hunk lines
        for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
            if logical_line >= render_start && logical_line < render_end {
                let is_selected_line = is_current
                    && tab.current_line == Some(line_idx);

                let old_num = diff_line
                    .old_num
                    .map(|n| format!("{:>4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_num = diff_line
                    .new_num
                    .map(|n| format!("{:>4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let (prefix, base_style) = if is_selected_line {
                    match diff_line.line_type {
                        LineType::Add => ("+", styles::line_cursor_add()),
                        LineType::Delete => ("-", styles::line_cursor_del()),
                        LineType::Context => (" ", styles::line_cursor()),
                    }
                } else {
                    match diff_line.line_type {
                        LineType::Add => ("+", styles::add_style()),
                        LineType::Delete => ("-", styles::del_style()),
                        LineType::Context => (" ", styles::default_style()),
                    }
                };

                let gutter_style = if is_selected_line {
                    ratatui::style::Style::default().fg(styles::BRIGHT).bg(styles::LINE_CURSOR_BG)
                } else {
                    match diff_line.line_type {
                        LineType::Add => ratatui::style::Style::default().fg(styles::DIM).bg(styles::ADD_BG),
                        LineType::Delete => ratatui::style::Style::default().fg(styles::DIM).bg(styles::DEL_BG),
                        LineType::Context => ratatui::style::Style::default().fg(styles::DIM),
                    }
                };

                // Build the line: gutter + prefix + syntax-highlighted content
                let mut spans = vec![
                    Span::styled(format!("{} {} \u{2502}", old_num, new_num), gutter_style),
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
            logical_line += 1;

            // Early exit for virtualized mode
            if use_viewport && logical_line > render_end + buffer_lines {
                break;
            }
        }

        // ── AI finding banners after each hunk (overlay mode) ──
        if in_overlay {
            let findings = match tab.mode {
                DiffMode::Branch => tab.ai.findings_for_hunk(&file.path, hunk_idx),
                DiffMode::Unstaged | DiffMode::Staged => {
                    tab.ai.findings_for_hunk_by_line_range(
                        &file.path,
                        hunk.new_start,
                        hunk.new_count,
                    )
                }
            };
            for finding in &findings {
                let severity_style = if file_stale {
                    styles::stale_style()
                } else {
                    match finding.severity {
                        RiskLevel::High => styles::risk_high(),
                        RiskLevel::Medium => styles::risk_medium(),
                        RiskLevel::Low => styles::risk_low(),
                        RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE),
                    }
                };

                let stale_tag = if file_stale { " [stale]" } else { "" };

                if logical_line >= render_start && logical_line < render_end {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  {} ", finding.severity.symbol()),
                            severity_style,
                        ),
                        Span::styled(
                            format!("[{}]", finding.category),
                            ratatui::style::Style::default().fg(styles::DIM).bg(styles::FINDING_BG),
                        ),
                        Span::styled(
                            format!(" {}{}", finding.title, stale_tag),
                            ratatui::style::Style::default().fg(styles::ORANGE).bg(styles::FINDING_BG),
                        ),
                    ]).style(styles::finding_style()));
                }
                logical_line += 1;

                if !finding.description.is_empty() {
                    if logical_line >= render_start && logical_line < render_end {
                        let desc = finding.description.lines().next().unwrap_or("");
                        let max_len = area.width.saturating_sub(6) as usize;
                        let truncated = if desc.chars().count() > max_len {
                            format!("{}\u{2026}", desc.chars().take(max_len.saturating_sub(1)).collect::<String>())
                        } else {
                            desc.to_string()
                        };
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("    {}", truncated),
                                ratatui::style::Style::default().fg(styles::MUTED).bg(styles::FINDING_BG),
                            ),
                        ]).style(ratatui::style::Style::default().bg(styles::FINDING_BG)));
                    }
                    logical_line += 1;
                }

                if !finding.suggestion.is_empty() {
                    if logical_line >= render_start && logical_line < render_end {
                        let sug = finding.suggestion.lines().next().unwrap_or("");
                        let max_len = area.width.saturating_sub(8) as usize;
                        let truncated = if sug.chars().count() > max_len {
                            format!("{}\u{2026}", sug.chars().take(max_len.saturating_sub(1)).collect::<String>())
                        } else {
                            sug.to_string()
                        };
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("    \u{2192} {}", truncated),
                                ratatui::style::Style::default().fg(styles::GREEN).bg(styles::FINDING_BG),
                            ),
                        ]).style(ratatui::style::Style::default().bg(styles::FINDING_BG)));
                    }
                    logical_line += 1;
                }
            }
        }

        // ── Human comments after each hunk ──
        {
            let comments = tab.ai.comments_for_hunk(&file.path, hunk_idx);
            for comment in &comments {
                if logical_line >= render_start && logical_line < render_end {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "  \u{1f4ac} ",
                            styles::comment_style(),
                        ),
                        Span::styled(
                            "You",
                            ratatui::style::Style::default()
                                .fg(styles::CYAN)
                                .bg(styles::COMMENT_BG)
                                .add_modifier(ratatui::style::Modifier::BOLD),
                        ),
                        Span::styled(
                            if comment.timestamp.is_empty() {
                                String::new()
                            } else {
                                let time_part = comment.timestamp
                                    .split('T')
                                    .nth(1)
                                    .unwrap_or("")
                                    .trim_end_matches('Z');
                                format!("  {}", time_part)
                            },
                            ratatui::style::Style::default().fg(styles::DIM).bg(styles::COMMENT_BG),
                        ),
                    ]).style(ratatui::style::Style::default().bg(styles::COMMENT_BG)));
                }
                logical_line += 1;

                if logical_line >= render_start && logical_line < render_end {
                    let max_len = area.width.saturating_sub(6) as usize;
                    let text = &comment.comment;
                    let truncated = if text.chars().count() > max_len {
                        format!("{}\u{2026}", text.chars().take(max_len.saturating_sub(1)).collect::<String>())
                    } else {
                        text.to_string()
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("    {}", truncated),
                            ratatui::style::Style::default().fg(styles::TEXT).bg(styles::COMMENT_BG),
                        ),
                    ]).style(ratatui::style::Style::default().bg(styles::COMMENT_BG)));
                }
                logical_line += 1;
            }
        }

        // Blank line between hunks
        if logical_line >= render_start && logical_line < render_end {
            lines.push(Line::from(""));
        }
        logical_line += 1;
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

    // Apply scroll: for virtualized rendering, adjust scroll to offset into the rendered window
    let visible_scroll = if use_viewport {
        let scroll_into_rendered = scroll.saturating_sub(render_start) as u16;
        (scroll_into_rendered, tab.h_scroll)
    } else {
        (tab.diff_scroll, tab.h_scroll)
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll(visible_scroll);

    f.render_widget(paragraph, area);

    // Render hunk indicator overlay in top-right corner
    if total_hunks > 0 {
        let indicator_text = format!("Hunk {}/{}", tab.current_hunk + 1, total_hunks);
        let indicator_width = indicator_text.len() + 3;
        let indicator_area = Rect {
            x: area.x + area.width.saturating_sub(indicator_width as u16 + 1),
            y: area.y,
            width: indicator_width as u16,
            height: 1,
        };
        let indicator = Paragraph::new(Line::from(Span::styled(
            indicator_text,
            ratatui::style::Style::default().fg(styles::MUTED),
        )));
        f.render_widget(indicator, indicator_area);
    }
}

/// Render a compacted file summary
fn render_compacted(f: &mut Frame, area: Rect, file: &crate::git::DiffFile) {
    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", file.path),
            ratatui::style::Style::default().fg(styles::BRIGHT),
        ))
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG))
        .padding(Padding::new(0, 1, 0, 0));

    let hunks_label = if file.raw_hunk_count > 0 {
        format!("  {} hunks", file.raw_hunk_count)
    } else {
        String::new()
    };

    let text = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  \u{1f4e6} ",
                ratatui::style::Style::default().fg(styles::MUTED),
            ),
            Span::styled(
                &file.path,
                ratatui::style::Style::default().fg(styles::TEXT),
            ),
            Span::styled(
                format!("  +{} \u{2212}{}{}", file.adds, file.dels, hunks_label),
                ratatui::style::Style::default().fg(styles::DIM),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  (compacted \u{2014} press Enter to expand)",
            ratatui::style::Style::default().fg(styles::MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Lock files, generated code, and large diffs are",
            ratatui::style::Style::default().fg(styles::DIM),
        )),
        Line::from(Span::styled(
            "  compacted automatically to save memory.",
            ratatui::style::Style::default().fg(styles::DIM),
        )),
    ])
    .block(block);

    f.render_widget(text, area);
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
