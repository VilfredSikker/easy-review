use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Padding},
    Frame,
};

use crate::ai::{CommentRef, CommentType, RiskLevel};
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

    // History mode: render multi-file commit diff
    if tab.mode == DiffMode::History {
        render_history_diff(f, area, app, hl);
        return;
    }

    // Check if a watched file is selected
    if let Some(watched) = tab.selected_watched_file() {
        render_watched(f, area, app, &watched.path.clone(), watched.size);
        return;
    }

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

    let in_overlay = tab.layers.show_ai_findings;
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
    let show_ai_header = tab.layers.show_ai_findings || tab.panel.is_some();
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

            // ── Inline line comments (rendered directly after the target line) ──
            if let Some(new_line_num) = diff_line.new_num {
                let line_comments = tab.ai.comments_for_line(&file.path, hunk_idx, new_line_num);
                let is_focused = |c: &CommentRef| {
                    tab.comment_focus.as_ref().map_or(false, |cf| cf.comment_id == c.id())
                };
                for comment in &line_comments {
                    let visible = match comment {
                        CommentRef::Question(_) => tab.layers.show_questions,
                        CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => tab.layers.show_github_comments,
                    };
                    if !visible {
                        continue;
                    }
                    render_comment_lines(
                        &mut lines,
                        comment,
                        area.width,
                        true,
                        is_focused(comment),
                    );
                    // Render replies to this line comment (GitHub comments only)
                    let replies = tab.ai.replies_to(comment.id());
                    for reply in &replies {
                        render_reply_lines(
                            &mut lines,
                            &reply,
                            area.width,
                            true,
                            is_focused(&reply),
                        );
                    }
                }
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
                DiffMode::History => vec![], // AI findings not shown in History mode
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

        // ── Hunk-level comments after the hunk ──
        {
            let hunk_comments = tab.ai.comments_for_hunk_only(&file.path, hunk_idx);
            let is_focused = |c: &CommentRef| {
                tab.comment_focus.as_ref().map_or(false, |cf| cf.comment_id == c.id())
            };
            for comment in &hunk_comments {
                let visible = match comment {
                    CommentRef::Question(_) => tab.layers.show_questions,
                    CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => tab.layers.show_github_comments,
                };
                if !visible {
                    continue;
                }
                render_comment_lines(
                    &mut lines,
                    comment,
                    area.width,
                    false,
                    is_focused(comment),
                );
                // Render replies to this hunk comment (GitHub comments only)
                let replies = tab.ai.replies_to(comment.id());
                for reply in &replies {
                    render_reply_lines(
                        &mut lines,
                        &reply,
                        area.width,
                        false,
                        is_focused(&reply),
                    );
                }
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

/// Render multi-file commit diff (History mode)
fn render_history_diff(f: &mut Frame, area: Rect, app: &App, hl: &mut Highlighter) {
    let tab = app.tab();
    let history = match tab.history.as_ref() {
        Some(h) => h,
        None => {
            render_history_empty(f, area, "No history available");
            return;
        }
    };

    if history.commits.is_empty() {
        render_history_empty(f, area, "No commits ahead of base branch");
        return;
    }

    if history.commit_files.is_empty() {
        let commit = &history.commits[history.selected_commit];
        render_history_empty(f, area, &format!("Empty commit: {}", commit.short_hash));
        return;
    }

    let commit = &history.commits[history.selected_commit];
    let title = format!(" {} · {} ", commit.short_hash, commit.subject);
    let total_files = history.commit_files.len();

    let mut lines: Vec<Line> = Vec::new();

    // Render each file as a section
    for (file_idx, file) in history.commit_files.iter().enumerate() {
        let is_current_file = file_idx == history.selected_file;

        // File header
        let file_header_bg = if is_current_file {
            styles::HUNK_BG
        } else {
            styles::BG
        };

        let mut header_spans = vec![
            Span::styled(
                if is_current_file { " ▶ " } else { "   " },
                ratatui::style::Style::default()
                    .fg(if is_current_file { styles::CYAN } else { styles::DIM })
                    .bg(file_header_bg),
            ),
            Span::styled(
                format!("{} ", file.status.symbol()),
                match &file.status {
                    crate::git::FileStatus::Added => ratatui::style::Style::default().fg(styles::GREEN).bg(file_header_bg),
                    crate::git::FileStatus::Deleted => ratatui::style::Style::default().fg(styles::RED).bg(file_header_bg),
                    _ => ratatui::style::Style::default().fg(styles::YELLOW).bg(file_header_bg),
                },
            ),
            Span::styled(
                &file.path,
                ratatui::style::Style::default()
                    .fg(if is_current_file { styles::BRIGHT } else { styles::TEXT })
                    .bg(file_header_bg),
            ),
            Span::styled(
                format!("  +{} -{}", file.adds, file.dels),
                ratatui::style::Style::default().fg(styles::DIM).bg(file_header_bg),
            ),
        ];

        // Pad the rest of the file header line
        let header_len: usize = header_spans.iter().map(|s| s.content.chars().count()).sum();
        let remaining = (area.width as usize).saturating_sub(header_len);
        header_spans.push(Span::styled(
            " ".repeat(remaining),
            ratatui::style::Style::default().bg(file_header_bg),
        ));

        lines.push(Line::from(header_spans));
        lines.push(Line::from(""));

        // Render hunks for this file
        for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
            let is_current_hunk = is_current_file && hunk_idx == history.current_hunk;

            // Hunk header
            let marker = if is_current_hunk { "▶" } else { " " };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", marker),
                    if is_current_hunk {
                        ratatui::style::Style::default().fg(styles::CYAN).bg(styles::HUNK_BG)
                    } else {
                        ratatui::style::Style::default().fg(styles::DIM).bg(styles::HUNK_BG)
                    },
                ),
                Span::styled(&hunk.header, styles::hunk_header_style()),
            ]).style(styles::hunk_header_style()));

            // Hunk lines
            for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
                let is_selected_line = is_current_hunk
                    && history.current_line == Some(line_idx);

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

                let mut spans = vec![
                    Span::styled(format!("{} {} │", old_num, new_num), gutter_style),
                    Span::styled(prefix, base_style),
                ];

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

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((history.diff_scroll, history.h_scroll));

    f.render_widget(paragraph, area);

    // File indicator overlay in top-right corner
    if total_files > 0 {
        let indicator_text = format!(
            "File {}/{}",
            history.selected_file + 1,
            total_files
        );
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

/// Render empty state for history mode
fn render_history_empty(f: &mut Frame, area: Rect, message: &str) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG));

    let text = Paragraph::new(vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", message),
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

/// Render a single comment (line-level or hunk-level) into the lines buffer
fn render_comment_lines(
    lines: &mut Vec<Line<'_>>,
    comment: &CommentRef,
    width: u16,
    inline: bool,
    focused: bool,
) {
    let is_question = comment.comment_type() == CommentType::Question;
    let is_stale = comment.is_stale();

    let bg = if focused {
        styles::COMMENT_FOCUS_BG
    } else if inline {
        styles::INLINE_COMMENT_BG
    } else {
        styles::COMMENT_BG
    };

    // Questions use yellow/orange, GitHub comments use cyan
    let accent = if is_stale {
        styles::STALE
    } else if is_question {
        styles::YELLOW
    } else {
        styles::CYAN
    };

    let icon = if is_question { "❓" } else { "💬" };
    let author = comment.author();

    let mut header_spans = vec![
        Span::styled(
            if inline { format!("     {} ", icon) } else { format!("  {} ", icon) },
            ratatui::style::Style::default().fg(accent).bg(bg),
        ),
        Span::styled(
            author.to_string(),
            ratatui::style::Style::default()
                .fg(accent)
                .bg(bg)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ];

    // Timestamp
    let ts = comment.timestamp();
    if !ts.is_empty() {
        let time_part = ts
            .split('T')
            .nth(1)
            .unwrap_or("")
            .trim_end_matches('Z');
        header_spans.push(Span::styled(
            format!("  {}", time_part),
            ratatui::style::Style::default().fg(styles::DIM).bg(bg),
        ));
    }

    // Stale indicator
    if is_stale {
        header_spans.push(Span::styled(
            "  ⚠ stale",
            ratatui::style::Style::default().fg(styles::STALE).bg(bg),
        ));
    }

    // Synced indicator (GitHub comments only)
    if comment.is_synced() {
        header_spans.push(Span::styled(
            "  ↑ synced",
            ratatui::style::Style::default().fg(styles::GREEN).bg(bg),
        ));
    }

    // Focus indicator
    if focused {
        header_spans.push(Span::styled(
            "  ◆",
            ratatui::style::Style::default().fg(styles::PURPLE).bg(bg),
        ));
    }

    lines.push(Line::from(header_spans).style(ratatui::style::Style::default().bg(bg)));

    // Comment text
    let indent: usize = if inline { 8 } else { 6 };
    let max_len = width.saturating_sub(indent as u16) as usize;
    let text = comment.text();
    let text_fg = if is_stale { styles::DIM } else { styles::TEXT };
    let truncated = if text.chars().count() > max_len {
        format!("{}…", text.chars().take(max_len.saturating_sub(1)).collect::<String>())
    } else {
        text.to_string()
    };
    let padding = " ".repeat(indent.saturating_sub(2));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {}{}", padding, truncated),
            ratatui::style::Style::default().fg(text_fg).bg(bg),
        ),
    ]).style(ratatui::style::Style::default().bg(bg)));
}

/// Render a reply comment (indented with ↳ prefix)
fn render_reply_lines(
    lines: &mut Vec<Line<'_>>,
    reply: &CommentRef,
    width: u16,
    inline: bool,
    focused: bool,
) {
    let bg = if focused {
        styles::COMMENT_FOCUS_BG
    } else if inline {
        styles::INLINE_COMMENT_BG
    } else {
        styles::COMMENT_BG
    };

    let author = reply.author();

    let prefix = if inline { "       ↳ 💬 " } else { "    ↳ 💬 " };
    let mut header_spans = vec![
        Span::styled(
            prefix,
            ratatui::style::Style::default().fg(styles::DIM).bg(bg),
        ),
        Span::styled(
            author.to_string(),
            ratatui::style::Style::default()
                .fg(styles::CYAN)
                .bg(bg)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ];

    let ts = reply.timestamp();
    if !ts.is_empty() {
        let time_part = ts
            .split('T')
            .nth(1)
            .unwrap_or("")
            .trim_end_matches('Z');
        header_spans.push(Span::styled(
            format!("  {}", time_part),
            ratatui::style::Style::default().fg(styles::DIM).bg(bg),
        ));
    }

    if reply.is_synced() {
        header_spans.push(Span::styled(
            "  ↑ synced",
            ratatui::style::Style::default().fg(styles::GREEN).bg(bg),
        ));
    }

    if focused {
        header_spans.push(Span::styled(
            "  ◆",
            ratatui::style::Style::default().fg(styles::PURPLE).bg(bg),
        ));
    }

    lines.push(Line::from(header_spans).style(ratatui::style::Style::default().bg(bg)));

    // Reply text
    let indent: usize = if inline { 12 } else { 10 };
    let max_len = width.saturating_sub(indent as u16) as usize;
    let text = reply.text();
    let truncated = if text.chars().count() > max_len {
        format!("{}…", text.chars().take(max_len.saturating_sub(1)).collect::<String>())
    } else {
        text.to_string()
    };
    let padding = " ".repeat(indent.saturating_sub(2));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {}{}", padding, truncated),
            ratatui::style::Style::default().fg(styles::TEXT).bg(bg),
        ),
    ]).style(ratatui::style::Style::default().bg(bg)));
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

/// Render a watched file's content in the diff view area
fn render_watched(f: &mut Frame, area: Rect, app: &App, path: &str, size: u64) {
    let tab = app.tab();
    let repo_root = &tab.repo_root;
    let not_ignored = tab.watched_not_ignored.contains(&path.to_string());

    let mut lines: Vec<Line> = Vec::new();

    // Header
    let status_label = if not_ignored {
        "watched · ⚠ not in .gitignore"
    } else {
        "watched · not tracked by git"
    };

    lines.push(Line::from(vec![
        Span::styled(
            format!("  ◉ {}", path),
            ratatui::style::Style::default().fg(styles::WATCHED_TEXT),
        ),
        Span::styled(
            format!("  ({})", status_label),
            ratatui::style::Style::default().fg(styles::WATCHED_MUTED),
        ),
    ]));

    // Size info
    let size_str = if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    };
    lines.push(Line::from(Span::styled(
        format!("  Size: {}", size_str),
        ratatui::style::Style::default().fg(styles::WATCHED_MUTED),
    )));

    lines.push(Line::from(""));

    // Check for snapshot diff mode
    let use_snapshot = tab.watched_config.diff_mode == "snapshot";

    if use_snapshot {
        // Try to get snapshot diff
        match crate::git::diff_watched_file_snapshot(repo_root, path) {
            Ok(Some(raw)) if raw.is_empty() => {
                lines.push(Line::from(Span::styled(
                    "  No changes since snapshot",
                    ratatui::style::Style::default().fg(styles::MUTED),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Press s to update snapshot",
                    ratatui::style::Style::default().fg(styles::DIM),
                )));
            }
            Ok(Some(raw)) => {
                // Parse and render the diff
                let parsed = crate::git::parse_diff(&raw);
                if let Some(diff_file) = parsed.into_iter().next() {
                    lines.push(Line::from(Span::styled(
                        "  diff vs snapshot",
                        ratatui::style::Style::default().fg(styles::WATCHED_MUTED),
                    )));
                    lines.push(Line::from(""));

                    // Render hunk data — use owned strings to avoid lifetime issues
                    for hunk in &diff_file.hunks {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", hunk.header.clone()),
                            styles::hunk_header_style(),
                        )).style(styles::hunk_header_style()));

                        for diff_line in &hunk.lines {
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
                            let old_num = diff_line.old_num
                                .map(|n| format!("{:>4}", n))
                                .unwrap_or_else(|| "    ".to_string());
                            let new_num = diff_line.new_num
                                .map(|n| format!("{:>4}", n))
                                .unwrap_or_else(|| "    ".to_string());

                            let spans = vec![
                                Span::styled(format!("{} {} │", old_num, new_num), gutter_style),
                                Span::styled(prefix, base_style),
                                Span::styled(diff_line.content.clone(), base_style),
                            ];
                            lines.push(Line::from(spans).style(base_style));
                        }
                        lines.push(Line::from(""));
                    }
                }
                lines.push(Line::from(Span::styled(
                    "  Press s to update snapshot",
                    ratatui::style::Style::default().fg(styles::DIM),
                )));
            }
            Ok(None) => {
                // New file — snapshot just created
                lines.push(Line::from(Span::styled(
                    "  Snapshot saved (first view)",
                    ratatui::style::Style::default().fg(styles::GREEN),
                )));
                // Fall through to show content
                render_watched_content_lines(&mut lines, repo_root, path, size);
            }
            Err(_) => {
                // Error — fall back to content mode
                render_watched_content_lines(&mut lines, repo_root, path, size);
            }
        }
    } else {
        // Content mode — show full file content
        render_watched_content_lines(&mut lines, repo_root, path, size);
    }

    let title = format!(" {} ", path);
    let block = Block::default()
        .title(Span::styled(
            title,
            ratatui::style::Style::default().fg(styles::WATCHED_TEXT),
        ))
        .title_position(ratatui::widgets::block::Position::Top)
        .title_alignment(ratatui::layout::Alignment::Left)
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG))
        .padding(Padding::new(0, 1, 0, 0));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((tab.diff_scroll, tab.h_scroll));

    f.render_widget(paragraph, area);
}

/// Render watched file content lines (content mode)
fn render_watched_content_lines(
    lines: &mut Vec<Line>,
    repo_root: &str,
    path: &str,
    size: u64,
) {
    // Binary check
    if size > 10 * 1024 * 1024 {
        lines.push(Line::from(Span::styled(
            format!("  Binary or large file ({:.1} MB)", size as f64 / (1024.0 * 1024.0)),
            ratatui::style::Style::default().fg(styles::MUTED),
        )));
        return;
    }

    match crate::git::read_watched_file_content(repo_root, path) {
        Ok(Some(content)) => {
            let total_lines = content.lines().count();
            if total_lines > 10_000 {
                lines.push(Line::from(Span::styled(
                    format!("  Large file ({} lines) — content truncated", total_lines),
                    ratatui::style::Style::default().fg(styles::MUTED),
                )));
                lines.push(Line::from(""));
            }

            let max_lines = total_lines.min(10_000);
            // Use owned strings to avoid lifetime issues with Span
            for (i, line_content) in content.lines().take(max_lines).enumerate() {
                let line_num = i + 1;
                let base_style = styles::watched_line_style();
                let gutter_style = styles::watched_gutter_style();

                let spans = vec![
                    Span::styled(format!("{:>5} │", line_num), gutter_style),
                    Span::styled(line_content.to_string(), base_style),
                ];
                lines.push(Line::from(spans).style(base_style));
            }

            if total_lines > max_lines {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("  ... {} more lines", total_lines - max_lines),
                    ratatui::style::Style::default().fg(styles::MUTED),
                )));
            }
        }
        Ok(None) => {
            // Binary file
            lines.push(Line::from(Span::styled(
                format!("  Binary file ({:.1} KB)", size as f64 / 1024.0),
                ratatui::style::Style::default().fg(styles::MUTED),
            )));
        }
        Err(e) => {
            lines.push(Line::from(Span::styled(
                format!("  Error reading file: {}", e),
                ratatui::style::Style::default().fg(styles::RED),
            )));
        }
    }
}
