use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};

use super::highlight::Highlighter;
use super::styles;
use super::utils::word_wrap;
use crate::ai::{CommentRef, CommentType, RiskLevel};
use crate::app::{App, DiffMode, SplitSide};
use crate::config::ErConfig;
use crate::git::LineType;

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
    let scroll = tab.active_diff_scroll() as usize;
    let render_start = if use_viewport {
        scroll.saturating_sub(buffer_lines)
    } else {
        0
    };
    let render_end = if use_viewport {
        scroll + viewport_height + buffer_lines
    } else {
        usize::MAX
    };

    // Build diff lines (only within viewport window when virtualized)
    let mut lines: Vec<Line> = Vec::with_capacity(if use_viewport {
        viewport_height + buffer_lines * 2
    } else {
        total_diff_lines + total_hunks * 2 + 4
    });
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
                    lines.push(Line::from(vec![Span::styled(
                        format!("  \u{2139} {}", fr.summary),
                        ratatui::style::Style::default().fg(styles::MUTED),
                    )]));
                }
                logical_line += 1;
            }
        }
    }

    if logical_line >= render_start && logical_line < render_end {
        lines.push(Line::from(""));
    }
    logical_line += 1;

    // ── File-level (unanchored) comments ──
    {
        let unanchored = tab.ai.comments_for_file_unanchored(&file.path);
        for comment in &unanchored {
            let visible = match comment {
                CommentRef::Question(_) => tab.layers.show_questions,
                CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => {
                    tab.layers.show_github_comments
                }
            };
            if !visible {
                continue;
            }
            let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
            let pre_len = lines.len();
            render_comment_lines(&mut lines, comment, area.width, false, is_focused);
            let comment_line_count = lines.len() - pre_len;
            if logical_line < render_start || logical_line >= render_end {
                lines.truncate(pre_len);
            }
            logical_line += comment_line_count;

            // Render replies
            let replies = tab.ai.replies_to(comment.id());
            for reply in &replies {
                let pre_len = lines.len();
                let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                render_reply_lines(&mut lines, reply, area.width, false, is_focused);
                let reply_line_count = lines.len() - pre_len;
                if logical_line < render_start || logical_line >= render_end {
                    lines.truncate(pre_len);
                }
                logical_line += reply_line_count;
            }
        }
    }

    // Unified gutter: "{old_num} {new_num} │" = 4+1+4+1+1=11 chars, plus prefix char = 12 total
    let unified_gutter_width: u16 = 12;
    let wrap_lines = app.config.display.wrap_lines;
    // Content width for wrapping: area width minus right padding (1) minus gutter
    let unified_wrap_width = (area
        .width
        .saturating_sub(1)
        .saturating_sub(unified_gutter_width)) as usize;

    // Render hunks
    for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
        let is_current = hunk_idx == tab.active_current_hunk();

        // Early exit — past viewport, no need to process remaining hunks
        if use_viewport && logical_line > render_end + buffer_lines {
            break;
        }

        // Hunk header
        if logical_line >= render_start && logical_line < render_end {
            let marker = if is_current { "\u{25b6}" } else { " " };
            lines.push(
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", marker),
                        if is_current {
                            ratatui::style::Style::default()
                                .fg(styles::CYAN)
                                .bg(styles::HUNK_BG)
                        } else {
                            ratatui::style::Style::default()
                                .fg(styles::DIM)
                                .bg(styles::HUNK_BG)
                        },
                    ),
                    Span::styled(&hunk.header, styles::hunk_header_style()),
                ])
                .style(styles::hunk_header_style()),
            );
        }
        logical_line += 1;

        // ── Hunk-level comments right after the @@ header ──
        {
            let hunk_comments = tab.ai.comments_for_hunk_only(&file.path, hunk_idx);
            for comment in &hunk_comments {
                let visible = match comment {
                    CommentRef::Question(_) => tab.layers.show_questions,
                    CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => {
                        tab.layers.show_github_comments
                    }
                };
                if !visible {
                    continue;
                }
                let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
                let pre_len = lines.len();
                render_comment_lines(&mut lines, comment, area.width, false, is_focused);
                let comment_line_count = lines.len() - pre_len;
                if logical_line < render_start || logical_line >= render_end {
                    lines.truncate(pre_len);
                }
                logical_line += comment_line_count;

                // Render replies to this hunk comment (GitHub comments only)
                let replies = tab.ai.replies_to(comment.id());
                for reply in &replies {
                    let pre_len = lines.len();
                    let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                    render_reply_lines(&mut lines, reply, area.width, false, is_focused);
                    let reply_line_count = lines.len() - pre_len;
                    if logical_line < render_start || logical_line >= render_end {
                        lines.truncate(pre_len);
                    }
                    logical_line += reply_line_count;
                }
            }
        }

        // Hunk lines
        for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
            let is_selected_line = is_current && tab.active_current_line() == Some(line_idx);

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
                ratatui::style::Style::default()
                    .fg(styles::BRIGHT)
                    .bg(styles::LINE_CURSOR_BG)
            } else {
                match diff_line.line_type {
                    LineType::Add => ratatui::style::Style::default()
                        .fg(styles::DIM)
                        .bg(styles::ADD_BG),
                    LineType::Delete => ratatui::style::Style::default()
                        .fg(styles::DIM)
                        .bg(styles::DEL_BG),
                    LineType::Context => ratatui::style::Style::default().fg(styles::DIM),
                }
            };

            if wrap_lines && !diff_line.content.is_empty() {
                // Wrap the content and emit multiple logical lines.
                // Segments are owned Strings; highlight them and convert to Span<'static>
                // so they can safely outlive the local `segments` Vec.
                let segments = word_wrap(&diff_line.content, unified_wrap_width.max(1));
                let blank_gutter = format!("{} {} \u{2502}", "    ", "    ");
                for (seg_idx, segment) in segments.iter().enumerate() {
                    if logical_line >= render_start && logical_line < render_end {
                        let mut spans: Vec<Span<'static>> = if seg_idx == 0 {
                            vec![
                                Span::styled(
                                    format!("{} {} \u{2502}", old_num, new_num),
                                    gutter_style,
                                ),
                                Span::styled(prefix, base_style),
                            ]
                        } else {
                            vec![
                                Span::styled(blank_gutter.clone(), gutter_style),
                                Span::styled(" ", base_style),
                            ]
                        };
                        // highlight_line borrows `segment`, so we eagerly clone span text to 'static
                        let highlighted: Vec<Span<'static>> = hl
                            .highlight_line(segment, &file.path, base_style)
                            .into_iter()
                            .map(|s| Span::styled(s.content.into_owned(), s.style))
                            .collect();
                        spans.extend(highlighted);
                        lines.push(Line::from(spans).style(base_style));
                    }
                    logical_line += 1;
                }
            } else {
                if logical_line >= render_start && logical_line < render_end {
                    // Build the line: gutter + prefix + syntax-highlighted content
                    let mut spans = vec![
                        Span::styled(format!("{} {} \u{2502}", old_num, new_num), gutter_style),
                        Span::styled(prefix, base_style),
                    ];

                    // Syntax highlight the code content
                    if diff_line.content.is_empty() {
                        spans.push(Span::styled("", base_style));
                    } else {
                        let highlighted =
                            hl.highlight_line(&diff_line.content, &file.path, base_style);
                        spans.extend(highlighted);
                    }

                    lines.push(Line::from(spans).style(base_style));
                }
                logical_line += 1;
            }

            // ── Inline line comments (rendered directly after the target line) ──
            if let Some(new_line_num) = diff_line.new_num {
                let line_comments = tab.ai.comments_for_line(&file.path, hunk_idx, new_line_num);
                for comment in &line_comments {
                    let visible = match comment {
                        CommentRef::Question(_) => tab.layers.show_questions,
                        CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => {
                            tab.layers.show_github_comments
                        }
                    };
                    if !visible {
                        continue;
                    }
                    let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
                    let pre_len = lines.len();
                    render_comment_lines(&mut lines, comment, area.width, true, is_focused);
                    let comment_line_count = lines.len() - pre_len;
                    if logical_line < render_start || logical_line >= render_end {
                        lines.truncate(pre_len);
                    }
                    logical_line += comment_line_count;

                    // Render replies to this line comment (GitHub comments only)
                    let replies = tab.ai.replies_to(comment.id());
                    for reply in &replies {
                        let pre_len = lines.len();
                        let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                        render_reply_lines(&mut lines, reply, area.width, true, is_focused);
                        let reply_line_count = lines.len() - pre_len;
                        if logical_line < render_start || logical_line >= render_end {
                            lines.truncate(pre_len);
                        }
                        logical_line += reply_line_count;
                    }
                }
            }
        }

        // ── AI finding banners after each hunk (overlay mode) ──
        if in_overlay {
            let findings = match tab.mode {
                DiffMode::Branch => tab.ai.findings_for_hunk(&file.path, hunk_idx),
                DiffMode::Unstaged | DiffMode::Staged => tab.ai.findings_for_hunk_by_line_range(
                    &file.path,
                    hunk.new_start,
                    hunk.new_count,
                ),
                DiffMode::History | DiffMode::Conflicts => vec![], // AI findings not shown in History/Conflicts mode
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
                    lines.push(
                        Line::from(vec![
                            Span::styled(
                                format!("  {} ", finding.severity.symbol()),
                                severity_style,
                            ),
                            Span::styled(
                                format!("[{}]", finding.category),
                                ratatui::style::Style::default()
                                    .fg(styles::DIM)
                                    .bg(styles::FINDING_BG),
                            ),
                            Span::styled(
                                format!(" {}{}", finding.title, stale_tag),
                                ratatui::style::Style::default()
                                    .fg(styles::ORANGE)
                                    .bg(styles::FINDING_BG),
                            ),
                        ])
                        .style(styles::finding_style()),
                    );
                }
                logical_line += 1;

                if !finding.description.is_empty() {
                    if logical_line >= render_start && logical_line < render_end {
                        let desc = finding.description.lines().next().unwrap_or("");
                        let max_len = area.width.saturating_sub(6) as usize;
                        let truncated = if desc.chars().count() > max_len {
                            format!(
                                "{}\u{2026}",
                                desc.chars()
                                    .take(max_len.saturating_sub(1))
                                    .collect::<String>()
                            )
                        } else {
                            desc.to_string()
                        };
                        lines.push(
                            Line::from(vec![Span::styled(
                                format!("    {}", truncated),
                                ratatui::style::Style::default()
                                    .fg(styles::MUTED)
                                    .bg(styles::FINDING_BG),
                            )])
                            .style(ratatui::style::Style::default().bg(styles::FINDING_BG)),
                        );
                    }
                    logical_line += 1;
                }

                if !finding.suggestion.is_empty() {
                    if logical_line >= render_start && logical_line < render_end {
                        let sug = finding.suggestion.lines().next().unwrap_or("");
                        let max_len = area.width.saturating_sub(8) as usize;
                        let truncated = if sug.chars().count() > max_len {
                            format!(
                                "{}\u{2026}",
                                sug.chars()
                                    .take(max_len.saturating_sub(1))
                                    .collect::<String>()
                            )
                        } else {
                            sug.to_string()
                        };
                        lines.push(
                            Line::from(vec![Span::styled(
                                format!("    \u{2192} {}", truncated),
                                ratatui::style::Style::default()
                                    .fg(styles::GREEN)
                                    .bg(styles::FINDING_BG),
                            )])
                            .style(ratatui::style::Style::default().bg(styles::FINDING_BG)),
                        );
                    }
                    logical_line += 1;
                }

                // Render response comments for this finding
                let finding_comments = tab.ai.comments_for_finding(&finding.id);
                for fc in &finding_comments {
                    if !tab.layers.show_github_comments {
                        continue;
                    }
                    let is_focused = tab.focused_comment_id.as_deref() == Some(fc.id());
                    let pre_len = lines.len();
                    render_reply_lines(&mut lines, fc, area.width, false, is_focused);
                    let fc_line_count = lines.len() - pre_len;
                    if logical_line < render_start || logical_line >= render_end {
                        lines.truncate(pre_len);
                    }
                    logical_line += fc_line_count;
                }
            }
        }

        // Blank line between hunks
        if logical_line >= render_start && logical_line < render_end {
            lines.push(Line::from(""));
        }
        logical_line += 1;
    }

    // ── Orphaned lost comments (hunk_index exceeded file hunk count) ──
    // Render after the last hunk so they don't disappear entirely
    {
        let num_hunks = file.hunks.len();
        let orphaned: Vec<CommentRef> = {
            let mut v = Vec::new();
            if let Some(qs) = &tab.ai.questions {
                for q in &qs.questions {
                    if q.file == file.path
                        && q.anchor_status == "lost"
                        && q.hunk_index.is_some_and(|hi| hi >= num_hunks)
                        && tab.layers.show_questions
                    {
                        v.push(CommentRef::Question(q));
                    }
                }
            }
            if let Some(gc) = &tab.ai.github_comments {
                for c in &gc.comments {
                    if c.file == file.path
                        && c.anchor_status == "lost"
                        && c.in_reply_to.is_none()
                        && c.hunk_index.is_some_and(|hi| hi >= num_hunks)
                        && tab.layers.show_github_comments
                    {
                        v.push(CommentRef::GitHubComment(c));
                    }
                }
            }
            v
        };
        if !orphaned.is_empty() {
            if logical_line >= render_start && logical_line < render_end {
                lines.push(Line::from(Span::styled(
                    "  -- comments from deleted hunks --",
                    ratatui::style::Style::default().fg(styles::MUTED),
                )));
            }
            logical_line += 1;

            for comment in &orphaned {
                let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
                let pre_len = lines.len();
                render_comment_lines(&mut lines, comment, area.width, false, is_focused);
                let comment_line_count = lines.len() - pre_len;
                if logical_line < render_start || logical_line >= render_end {
                    lines.truncate(pre_len);
                }
                logical_line += comment_line_count;
            }
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

    // Apply scroll: for virtualized rendering, adjust scroll to offset into the rendered window.
    // When wrap_lines is enabled, disable horizontal scroll (lines fit within the viewport).
    let effective_h_scroll = if app.config.display.wrap_lines {
        0
    } else {
        tab.h_scroll
    };
    // TODO(risk:medium): scroll.saturating_sub(render_start) is a usize value cast
    // directly to u16. For viewport-mode diffs with large scroll offsets (> 65535 lines
    // rendered before the viewport window), this overflows silently. In practice diffs
    // that large are auto-compacted, but the cast should use min(u16::MAX) or be
    // validated to match the actual rendered buffer height.
    let visible_scroll = if use_viewport {
        let scroll_into_rendered = scroll.saturating_sub(render_start) as u16;
        (scroll_into_rendered, effective_h_scroll)
    } else {
        (tab.active_diff_scroll(), effective_h_scroll)
    };

    let paragraph = Paragraph::new(lines).block(block).scroll(visible_scroll);

    f.render_widget(paragraph, area);

    // Render hunk indicator overlay in top-right corner
    if total_hunks > 0 {
        let indicator_text = format!("Hunk {}/{}", tab.active_current_hunk() + 1, total_hunks);
        // TODO(risk:minor): indicator_width is usize cast to u16. If total_hunks is very
        // large (> 9999) the formatted string can exceed u16::MAX chars — harmless in
        // practice but the cast is silent. saturating_as or a length cap would be safer.
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

/// Render the diff view in split (side-by-side) mode.
/// Falls back to `render()` when the area is too narrow or when conditions
/// (compacted file, watched file, empty, etc.) make split inapplicable.
pub fn render_split(f: &mut Frame, area: Rect, app: &App, hl: &mut Highlighter, config: &ErConfig) {
    let tab = app.tab();

    // Width guard — fall back to unified view when too narrow for split
    if area.width < 60 {
        render(f, area, app, hl);
        return;
    }

    // Delegate to unified for special states
    if tab.selected_watched_file().is_some() {
        render(f, area, app, hl);
        return;
    }
    let file = match tab.selected_diff_file() {
        Some(f) => f,
        None => {
            render(f, area, app, hl);
            return;
        }
    };
    if file.compacted {
        render(f, area, app, hl);
        return;
    }
    let _ = config; // used by caller for guard; flag already checked before calling

    // Split area 50/50
    let halves =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    // Render Old side first (borrows hl mutably), then New side
    render_split_side(f, halves[0], app, hl, SplitSide::Old);
    render_split_side(f, halves[1], app, hl, SplitSide::New);
}

/// Render one pane of the split diff view.
fn render_split_side(f: &mut Frame, area: Rect, app: &App, hl: &mut Highlighter, side: SplitSide) {
    let tab = app.tab();

    // Border with focus indicator
    let border_style = if tab.split_focus == side {
        styles::split_border_focused()
    } else {
        styles::split_border_inactive()
    };
    let title = match side {
        SplitSide::Old => " Old ",
        SplitSide::New => " New ",
    };
    let block = Block::default()
        .title(Span::styled(title, border_style))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(ratatui::style::Style::default().bg(styles::BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let file = match tab.selected_diff_file() {
        Some(f) => f,
        None => return,
    };

    // Viewport parameters — shared vertical scroll between both sides
    let total_diff_lines: usize = file.hunks.iter().map(|h| h.lines.len()).sum();
    let use_viewport = total_diff_lines > VIRTUALIZE_THRESHOLD;
    let viewport_height = inner.height as usize;
    let buffer_lines = if use_viewport { 20 } else { 0 };
    let scroll = tab.active_diff_scroll() as usize;
    let render_start = if use_viewport {
        scroll.saturating_sub(buffer_lines)
    } else {
        0
    };
    let render_end = if use_viewport {
        scroll + viewport_height + buffer_lines
    } else {
        usize::MAX
    };

    // In History mode there is a single h_scroll shared across both sides
    let h_scroll = if tab.mode == crate::app::DiffMode::History {
        tab.history.as_ref().map_or(0, |h| h.h_scroll)
    } else {
        match side {
            SplitSide::Old => tab.h_scroll_old,
            SplitSide::New => tab.h_scroll_new,
        }
    };

    let mut lines: Vec<Line> = Vec::with_capacity(if use_viewport {
        viewport_height + buffer_lines * 2
    } else {
        total_diff_lines + file.hunks.len() + 4
    });
    let mut logical_line: usize = 0;

    // File header (only on New side to avoid duplication; Old side gets a blank line instead)
    if side == SplitSide::New {
        if logical_line >= render_start && logical_line < render_end {
            let header_spans = vec![
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
            lines.push(Line::from(header_spans));
        }
    } else if logical_line >= render_start && logical_line < render_end {
        lines.push(Line::from(""));
    }
    logical_line += 1;

    // Blank separator after header
    if logical_line >= render_start && logical_line < render_end {
        lines.push(Line::from(""));
    }
    logical_line += 1;

    // File-level (unanchored) comments — only on New side
    if side == SplitSide::New {
        let unanchored = tab.ai.comments_for_file_unanchored(&file.path);
        for comment in &unanchored {
            let visible = match comment {
                CommentRef::Question(_) => tab.layers.show_questions,
                CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => {
                    tab.layers.show_github_comments
                }
            };
            if !visible {
                continue;
            }
            let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
            let pre_len = lines.len();
            render_comment_lines(&mut lines, comment, inner.width, false, is_focused);
            let comment_line_count = lines.len() - pre_len;
            if logical_line < render_start || logical_line >= render_end {
                lines.truncate(pre_len);
            }
            logical_line += comment_line_count;

            let replies = tab.ai.replies_to(comment.id());
            for reply in &replies {
                let pre_len = lines.len();
                let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                render_reply_lines(&mut lines, reply, inner.width, false, is_focused);
                let reply_line_count = lines.len() - pre_len;
                if logical_line < render_start || logical_line >= render_end {
                    lines.truncate(pre_len);
                }
                logical_line += reply_line_count;
            }
        }
    }

    // Split gutter: "{num} │" = 4+1+1=6 chars, plus prefix char = 7 total
    let split_gutter_width: u16 = 7;
    let wrap_lines = app.config.display.wrap_lines;
    // Content width for wrapping: inner width minus gutter
    let split_wrap_width = (inner.width.saturating_sub(split_gutter_width)) as usize;

    // Render hunks
    for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
        let is_current = hunk_idx == tab.active_current_hunk();

        // Early exit past viewport
        if use_viewport && logical_line > render_end + buffer_lines {
            break;
        }

        // Hunk header — shown on both sides
        if logical_line >= render_start && logical_line < render_end {
            let marker = if is_current { "\u{25b6}" } else { " " };
            lines.push(
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", marker),
                        if is_current {
                            ratatui::style::Style::default()
                                .fg(styles::CYAN)
                                .bg(styles::HUNK_BG)
                        } else {
                            ratatui::style::Style::default()
                                .fg(styles::DIM)
                                .bg(styles::HUNK_BG)
                        },
                    ),
                    Span::styled(&hunk.header, styles::hunk_header_style()),
                ])
                .style(styles::hunk_header_style()),
            );
        }
        logical_line += 1;

        // Hunk-level comments — only on New side for simplicity
        // TODO: split view — render hunk comments on both sides aligned
        if side == SplitSide::New {
            let hunk_comments = tab.ai.comments_for_hunk_only(&file.path, hunk_idx);
            for comment in &hunk_comments {
                let visible = match comment {
                    CommentRef::Question(_) => tab.layers.show_questions,
                    CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => {
                        tab.layers.show_github_comments
                    }
                };
                if !visible {
                    continue;
                }
                let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
                let pre_len = lines.len();
                render_comment_lines(&mut lines, comment, inner.width, false, is_focused);
                let comment_line_count = lines.len() - pre_len;
                if logical_line < render_start || logical_line >= render_end {
                    lines.truncate(pre_len);
                }
                logical_line += comment_line_count;

                let replies = tab.ai.replies_to(comment.id());
                for reply in &replies {
                    let pre_len = lines.len();
                    let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                    render_reply_lines(&mut lines, reply, inner.width, false, is_focused);
                    let reply_line_count = lines.len() - pre_len;
                    if logical_line < render_start || logical_line >= render_end {
                        lines.truncate(pre_len);
                    }
                    logical_line += reply_line_count;
                }
            }
        }

        // Hunk lines
        for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
            // Determine if this side should show content or a blank padding line
            let show_content = match diff_line.line_type {
                LineType::Context => true,
                LineType::Add => side == SplitSide::New,
                LineType::Delete => side == SplitSide::Old,
            };

            if show_content && wrap_lines && !diff_line.content.is_empty() {
                // Wrap the content and emit multiple logical lines
                let is_selected_line = is_current && tab.active_current_line() == Some(line_idx);

                let line_num = match side {
                    SplitSide::Old => diff_line.old_num,
                    SplitSide::New => diff_line.new_num,
                };
                let num_str = line_num
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
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT)
                        .bg(styles::LINE_CURSOR_BG)
                } else {
                    match diff_line.line_type {
                        LineType::Add => ratatui::style::Style::default()
                            .fg(styles::DIM)
                            .bg(styles::ADD_BG),
                        LineType::Delete => ratatui::style::Style::default()
                            .fg(styles::DIM)
                            .bg(styles::DEL_BG),
                        LineType::Context => ratatui::style::Style::default().fg(styles::DIM),
                    }
                };

                // Segments are owned Strings; highlight them and convert to Span<'static>
                // so they can safely outlive the local `segments` Vec.
                let segments = word_wrap(&diff_line.content, split_wrap_width.max(1));
                let blank_gutter = format!("{} \u{2502}", "    ");
                for (seg_idx, segment) in segments.iter().enumerate() {
                    if logical_line >= render_start && logical_line < render_end {
                        let mut spans: Vec<Span<'static>> = if seg_idx == 0 {
                            vec![
                                Span::styled(format!("{} \u{2502}", num_str), gutter_style),
                                Span::styled(prefix, base_style),
                            ]
                        } else {
                            vec![
                                Span::styled(blank_gutter.clone(), gutter_style),
                                Span::styled(" ", base_style),
                            ]
                        };
                        // highlight_line borrows `segment`, so we eagerly clone span text to 'static
                        let highlighted: Vec<Span<'static>> = hl
                            .highlight_line(segment, &file.path, base_style)
                            .into_iter()
                            .map(|s| Span::styled(s.content.into_owned(), s.style))
                            .collect();
                        spans.extend(highlighted);
                        lines.push(Line::from(spans).style(base_style));
                    }
                    logical_line += 1;
                }
            } else {
                if logical_line >= render_start && logical_line < render_end {
                    if show_content {
                        let is_selected_line =
                            is_current && tab.active_current_line() == Some(line_idx);

                        // Line number: Old side shows old_num, New side shows new_num
                        let line_num = match side {
                            SplitSide::Old => diff_line.old_num,
                            SplitSide::New => diff_line.new_num,
                        };
                        let num_str = line_num
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
                            ratatui::style::Style::default()
                                .fg(styles::BRIGHT)
                                .bg(styles::LINE_CURSOR_BG)
                        } else {
                            match diff_line.line_type {
                                LineType::Add => ratatui::style::Style::default()
                                    .fg(styles::DIM)
                                    .bg(styles::ADD_BG),
                                LineType::Delete => ratatui::style::Style::default()
                                    .fg(styles::DIM)
                                    .bg(styles::DEL_BG),
                                LineType::Context => {
                                    ratatui::style::Style::default().fg(styles::DIM)
                                }
                            }
                        };

                        let mut spans = vec![
                            Span::styled(format!("{} \u{2502}", num_str), gutter_style),
                            Span::styled(prefix, base_style),
                        ];

                        if diff_line.content.is_empty() {
                            spans.push(Span::styled("", base_style));
                        } else {
                            let highlighted =
                                hl.highlight_line(&diff_line.content, &file.path, base_style);
                            spans.extend(highlighted);
                        }

                        lines.push(Line::from(spans).style(base_style));
                    } else {
                        // Blank padding line for the side that doesn't show this change
                        let pad_bg = match diff_line.line_type {
                            LineType::Add => styles::ADD_BG,
                            LineType::Delete => styles::DEL_BG,
                            LineType::Context => styles::BG,
                        };
                        lines.push(
                            Line::from(Span::styled(
                                "",
                                ratatui::style::Style::default().bg(pad_bg),
                            ))
                            .style(ratatui::style::Style::default().bg(pad_bg)),
                        );
                    }
                }
                logical_line += 1;
            }

            // Inline line comments — on their natural side
            // Add lines → New side; Delete lines → Old side; Context → New side
            let comment_side = match diff_line.line_type {
                LineType::Add => SplitSide::New,
                LineType::Delete => SplitSide::Old,
                LineType::Context => SplitSide::New,
            };
            if side == comment_side {
                if let Some(new_line_num) = diff_line.new_num {
                    let line_comments =
                        tab.ai.comments_for_line(&file.path, hunk_idx, new_line_num);
                    for comment in &line_comments {
                        let visible = match comment {
                            CommentRef::Question(_) => tab.layers.show_questions,
                            CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => {
                                tab.layers.show_github_comments
                            }
                        };
                        if !visible {
                            continue;
                        }
                        let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
                        let pre_len = lines.len();
                        render_comment_lines(&mut lines, comment, inner.width, true, is_focused);
                        let comment_line_count = lines.len() - pre_len;
                        if logical_line < render_start || logical_line >= render_end {
                            lines.truncate(pre_len);
                        }
                        logical_line += comment_line_count;

                        let replies = tab.ai.replies_to(comment.id());
                        for reply in &replies {
                            let pre_len = lines.len();
                            let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                            render_reply_lines(&mut lines, reply, inner.width, true, is_focused);
                            let reply_line_count = lines.len() - pre_len;
                            if logical_line < render_start || logical_line >= render_end {
                                lines.truncate(pre_len);
                            }
                            logical_line += reply_line_count;
                        }
                    }
                }
            } else {
                // For the opposite side, advance logical_line by however many comment
                // lines the other side has — so both panes stay vertically aligned.
                // TODO: split view — implement aligned comment padding across panes
            }
        }

        // AI finding banners — only on New side (same as unified view)
        if side == SplitSide::New && tab.layers.show_ai_findings {
            let file_stale = tab.ai.is_file_stale(&file.path);
            let findings = match tab.mode {
                DiffMode::Branch => tab.ai.findings_for_hunk(&file.path, hunk_idx),
                DiffMode::Unstaged | DiffMode::Staged => tab.ai.findings_for_hunk_by_line_range(
                    &file.path,
                    hunk.new_start,
                    hunk.new_count,
                ),
                DiffMode::History | DiffMode::Conflicts => vec![],
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
                    lines.push(
                        Line::from(vec![
                            Span::styled(
                                format!("  {} ", finding.severity.symbol()),
                                severity_style,
                            ),
                            Span::styled(
                                format!("[{}]", finding.category),
                                ratatui::style::Style::default()
                                    .fg(styles::DIM)
                                    .bg(styles::FINDING_BG),
                            ),
                            Span::styled(
                                format!(" {}{}", finding.title, stale_tag),
                                ratatui::style::Style::default()
                                    .fg(styles::ORANGE)
                                    .bg(styles::FINDING_BG),
                            ),
                        ])
                        .style(styles::finding_style()),
                    );
                }
                logical_line += 1;

                if !finding.description.is_empty() {
                    if logical_line >= render_start && logical_line < render_end {
                        let desc = finding.description.lines().next().unwrap_or("");
                        let max_len = inner.width.saturating_sub(6) as usize;
                        let truncated = if desc.chars().count() > max_len {
                            format!(
                                "{}\u{2026}",
                                desc.chars()
                                    .take(max_len.saturating_sub(1))
                                    .collect::<String>()
                            )
                        } else {
                            desc.to_string()
                        };
                        lines.push(
                            Line::from(vec![Span::styled(
                                format!("    {}", truncated),
                                ratatui::style::Style::default()
                                    .fg(styles::MUTED)
                                    .bg(styles::FINDING_BG),
                            )])
                            .style(ratatui::style::Style::default().bg(styles::FINDING_BG)),
                        );
                    }
                    logical_line += 1;
                }
            }
        }

        // Blank line between hunks
        if logical_line >= render_start && logical_line < render_end {
            lines.push(Line::from(""));
        }
        logical_line += 1;
    }

    // Apply scroll: adjust into the rendered viewport window.
    // When wrap_lines is enabled, disable horizontal scroll (lines fit within the viewport).
    let effective_h_scroll = if app.config.display.wrap_lines {
        0
    } else {
        h_scroll
    };
    // TODO(risk:medium): same silent usize→u16 truncation as in the unified render path.
    // scroll.saturating_sub(render_start) can exceed u16::MAX on pathologically large diffs.
    let visible_scroll = if use_viewport {
        let scroll_into_rendered = scroll.saturating_sub(render_start) as u16;
        (scroll_into_rendered, effective_h_scroll)
    } else {
        (tab.active_diff_scroll(), effective_h_scroll)
    };

    let paragraph = Paragraph::new(lines).scroll(visible_scroll);

    f.render_widget(paragraph, inner);
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
        // TODO(risk:high): history.selected_commit is not bounds-checked before indexing.
        // If selected_commit >= history.commits.len() this panics. commits.is_empty() is
        // checked above, but selected_commit could still be out of range if state is stale.
        // Use .get() and fall back gracefully.
        let commit = &history.commits[history.selected_commit];
        render_history_empty(f, area, &format!("Empty commit: {}", commit.short_hash));
        return;
    }

    // TODO(risk:high): same unchecked index — selected_commit must be validated against
    // history.commits.len() before this point, not just after a commits.is_empty() guard.
    let commit = &history.commits[history.selected_commit];
    let title = format!(" {} · {} ", commit.short_hash, commit.subject);
    let total_files = history.commit_files.len();

    // TODO(risk:medium): render_history_diff builds the entire multi-file diff as an
    // unbounded Vec<Line> with no viewport culling. A commit touching hundreds of large
    // files can produce tens of thousands of Line objects, all allocated every frame.
    // This function bypasses the VIRTUALIZE_THRESHOLD guard used in the normal diff path.
    // Apply the same viewport-based rendering used in render() and render_split_side().
    let mut lines: Vec<Line> = Vec::new();
    // Track the line index where each file header starts (for sticky header)
    let mut file_header_line_indices: Vec<usize> = Vec::new();

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
                    .fg(if is_current_file {
                        styles::CYAN
                    } else {
                        styles::DIM
                    })
                    .bg(file_header_bg),
            ),
            Span::styled(
                format!("{} ", file.status.symbol()),
                match &file.status {
                    crate::git::FileStatus::Added => ratatui::style::Style::default()
                        .fg(styles::GREEN)
                        .bg(file_header_bg),
                    crate::git::FileStatus::Deleted => ratatui::style::Style::default()
                        .fg(styles::RED)
                        .bg(file_header_bg),
                    _ => ratatui::style::Style::default()
                        .fg(styles::YELLOW)
                        .bg(file_header_bg),
                },
            ),
            Span::styled(
                &file.path,
                ratatui::style::Style::default()
                    .fg(if is_current_file {
                        styles::BRIGHT
                    } else {
                        styles::TEXT
                    })
                    .bg(file_header_bg),
            ),
            Span::styled(
                format!("  +{} -{}", file.adds, file.dels),
                ratatui::style::Style::default()
                    .fg(styles::DIM)
                    .bg(file_header_bg),
            ),
        ];

        // Pad the rest of the file header line
        let header_len: usize = header_spans.iter().map(|s| s.content.chars().count()).sum();
        let remaining = (area.width as usize).saturating_sub(header_len);
        header_spans.push(Span::styled(
            " ".repeat(remaining),
            ratatui::style::Style::default().bg(file_header_bg),
        ));

        file_header_line_indices.push(lines.len());
        lines.push(Line::from(header_spans));
        lines.push(Line::from(""));

        // Render hunks for this file
        for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
            let is_current_hunk = is_current_file && hunk_idx == history.current_hunk;

            // Hunk header
            let marker = if is_current_hunk { "▶" } else { " " };
            lines.push(
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", marker),
                        if is_current_hunk {
                            ratatui::style::Style::default()
                                .fg(styles::CYAN)
                                .bg(styles::HUNK_BG)
                        } else {
                            ratatui::style::Style::default()
                                .fg(styles::DIM)
                                .bg(styles::HUNK_BG)
                        },
                    ),
                    Span::styled(&hunk.header, styles::hunk_header_style()),
                ])
                .style(styles::hunk_header_style()),
            );

            // Hunk lines
            for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
                let is_selected_line = is_current_hunk && history.current_line == Some(line_idx);

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
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT)
                        .bg(styles::LINE_CURSOR_BG)
                } else {
                    match diff_line.line_type {
                        LineType::Add => ratatui::style::Style::default()
                            .fg(styles::DIM)
                            .bg(styles::ADD_BG),
                        LineType::Delete => ratatui::style::Style::default()
                            .fg(styles::DIM)
                            .bg(styles::DEL_BG),
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

    // Sticky filename header: when a file's header scrolls above the viewport,
    // pin it at the top so the user always knows which file they're looking at.
    // The block title occupies the first row of `area`, so content starts at area.y + 1.
    let scroll = history.diff_scroll as usize;
    if scroll > 0 && !file_header_line_indices.is_empty() {
        // Find which file's section is at the top of the viewport:
        // the last file whose header line index <= scroll position.
        let topmost_file_idx = file_header_line_indices
            .iter()
            .rposition(|&line_idx| line_idx <= scroll)
            .unwrap_or(0);

        // Only show the sticky header if the file's header has scrolled off-screen
        // (i.e., the scroll position is past the header line itself).
        // TODO(risk:high): file_header_line_indices[topmost_file_idx] and
        // history.commit_files[topmost_file_idx] are both unchecked index accesses.
        // rposition() returns an index into file_header_line_indices which was built in
        // the same loop as the files, so lengths should match — but if commit_files was
        // mutated between the build and this read (concurrent watch refresh) they can
        // diverge and both accesses panic. Take a snapshot of commit_files at the top of
        // render_history_diff and use it throughout.
        let header_line = file_header_line_indices[topmost_file_idx];
        if scroll > header_line {
            let file = &history.commit_files[topmost_file_idx];
            let sticky_bg = styles::PANEL;

            let mut sticky_spans = vec![
                Span::styled(
                    format!("{} ", file.status.symbol()),
                    match &file.status {
                        crate::git::FileStatus::Added => ratatui::style::Style::default()
                            .fg(styles::GREEN)
                            .bg(sticky_bg),
                        crate::git::FileStatus::Deleted => ratatui::style::Style::default()
                            .fg(styles::RED)
                            .bg(sticky_bg),
                        _ => ratatui::style::Style::default()
                            .fg(styles::YELLOW)
                            .bg(sticky_bg),
                    },
                ),
                Span::styled(
                    &file.path,
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT)
                        .bg(sticky_bg),
                ),
                Span::styled(
                    format!("  +{} -{}", file.adds, file.dels),
                    ratatui::style::Style::default()
                        .fg(styles::DIM)
                        .bg(sticky_bg),
                ),
            ];

            // Pad the sticky header to fill the full width
            let sticky_len: usize = sticky_spans.iter().map(|s| s.content.chars().count()).sum();
            let sticky_remaining = (area.width as usize).saturating_sub(sticky_len);
            sticky_spans.push(Span::styled(
                " ".repeat(sticky_remaining),
                ratatui::style::Style::default().bg(sticky_bg),
            ));

            let sticky_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 1,
            };
            f.render_widget(Paragraph::new(Line::from(sticky_spans)), sticky_area);
        }
    }

    // File indicator overlay in top-right corner
    if total_files > 0 {
        let indicator_text = format!("File {}/{}", history.selected_file + 1, total_files);
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
            if focused {
                // Bright left marker when focused
                if inline {
                    format!("  ▸  {} ", icon)
                } else {
                    format!("▸ {} ", icon)
                }
            } else if inline {
                format!("     {} ", icon)
            } else {
                format!("  {} ", icon)
            },
            ratatui::style::Style::default()
                .fg(if focused { styles::PURPLE } else { accent })
                .bg(bg),
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
        let time_part = ts.split('T').nth(1).unwrap_or("").trim_end_matches('Z');
        header_spans.push(Span::styled(
            format!("  {}", time_part),
            ratatui::style::Style::default().fg(styles::DIM).bg(bg),
        ));
    }

    // Relocated/lost anchor indicators
    let anchor = comment.anchor_status();
    if anchor == "relocated" {
        header_spans.push(Span::styled(
            "  \u{21aa} moved",
            ratatui::style::Style::default()
                .fg(styles::RELOCATED_INDICATOR)
                .bg(bg),
        ));
    } else if anchor == "lost" {
        header_spans.push(Span::styled(
            "  ? lost",
            ratatui::style::Style::default()
                .fg(styles::LOST_INDICATOR)
                .bg(bg),
        ));
    }

    // Stale indicator
    if is_stale {
        header_spans.push(Span::styled(
            "  \u{26a0} stale",
            ratatui::style::Style::default().fg(styles::STALE).bg(bg),
        ));
    }

    // Synced indicator (GitHub comments only)
    if comment.is_synced() {
        header_spans.push(Span::styled(
            "  ↑ synced",
            ratatui::style::Style::default().fg(styles::GREEN).bg(bg),
        ));
    } else if comment.comment_type() == CommentType::GitHubComment {
        header_spans.push(Span::styled(
            "  ↑ local",
            ratatui::style::Style::default().fg(styles::DIM).bg(bg),
        ));
    }

    // Focus indicator
    if focused {
        header_spans.push(Span::styled(
            "  ◆ focused",
            ratatui::style::Style::default()
                .fg(styles::PURPLE)
                .bg(bg)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
    }

    lines.push(Line::from(header_spans).style(ratatui::style::Style::default().bg(bg)));

    // Comment text — split by lines first so paragraph breaks and bullet points are preserved
    let indent: usize = if inline { 8 } else { 6 };
    let max_len = width.saturating_sub(indent as u16) as usize;
    let text = comment.text();
    let is_lost = anchor == "lost";
    let text_fg = if is_stale || is_lost {
        styles::DIM
    } else {
        styles::TEXT
    };
    let padding = " ".repeat(indent.saturating_sub(2));
    for wrapped in word_wrap(text, max_len) {
        lines.push(
            Line::from(vec![Span::styled(
                format!("  {}{}", padding, wrapped),
                ratatui::style::Style::default().fg(text_fg).bg(bg),
            )])
            .style(ratatui::style::Style::default().bg(bg)),
        );
    }
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

    let is_question = reply.comment_type() == CommentType::Question;
    let accent = if is_question {
        styles::YELLOW
    } else {
        styles::CYAN
    };
    let icon = if is_question { "❓" } else { "💬" };
    let author = reply.author();

    let prefix = if inline {
        format!("       ↳ {} ", icon)
    } else {
        format!("    ↳ {} ", icon)
    };
    let mut header_spans = vec![
        Span::styled(
            prefix,
            ratatui::style::Style::default().fg(styles::DIM).bg(bg),
        ),
        Span::styled(
            author.to_string(),
            ratatui::style::Style::default()
                .fg(accent)
                .bg(bg)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
    ];

    let ts = reply.timestamp();
    if !ts.is_empty() {
        let time_part = ts.split('T').nth(1).unwrap_or("").trim_end_matches('Z');
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
    } else if reply.comment_type() == CommentType::GitHubComment {
        header_spans.push(Span::styled(
            "  ↑ local",
            ratatui::style::Style::default().fg(styles::DIM).bg(bg),
        ));
    }

    if focused {
        header_spans.push(Span::styled(
            "  ◆",
            ratatui::style::Style::default().fg(styles::PURPLE).bg(bg),
        ));
    }

    lines.push(Line::from(header_spans).style(ratatui::style::Style::default().bg(bg)));

    // Reply text — split by lines first so paragraph breaks and bullet points are preserved
    let indent: usize = if inline { 12 } else { 10 };
    let max_len = width.saturating_sub(indent as u16) as usize;
    let text = reply.text();
    let padding = " ".repeat(indent.saturating_sub(2));
    for wrapped in word_wrap(text, max_len) {
        lines.push(
            Line::from(vec![Span::styled(
                format!("  {}{}", padding, wrapped),
                ratatui::style::Style::default().fg(styles::TEXT).bg(bg),
            )])
            .style(ratatui::style::Style::default().bg(bg)),
        );
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
    let size_str = format_size(size);
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
                        lines.push(
                            Line::from(Span::styled(
                                format!("  {}", hunk.header.clone()),
                                styles::hunk_header_style(),
                            ))
                            .style(styles::hunk_header_style()),
                        );

                        for diff_line in &hunk.lines {
                            let (prefix, base_style) = match diff_line.line_type {
                                LineType::Add => ("+", styles::add_style()),
                                LineType::Delete => ("-", styles::del_style()),
                                LineType::Context => (" ", styles::default_style()),
                            };
                            let gutter_style = match diff_line.line_type {
                                LineType::Add => ratatui::style::Style::default()
                                    .fg(styles::DIM)
                                    .bg(styles::ADD_BG),
                                LineType::Delete => ratatui::style::Style::default()
                                    .fg(styles::DIM)
                                    .bg(styles::DEL_BG),
                                LineType::Context => {
                                    ratatui::style::Style::default().fg(styles::DIM)
                                }
                            };
                            let old_num = diff_line
                                .old_num
                                .map(|n| format!("{:>4}", n))
                                .unwrap_or_else(|| "    ".to_string());
                            let new_num = diff_line
                                .new_num
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
fn render_watched_content_lines(lines: &mut Vec<Line>, repo_root: &str, path: &str, size: u64) {
    // Binary check
    if size > 10 * 1024 * 1024 {
        lines.push(Line::from(Span::styled(
            format!(
                "  Binary or large file ({:.1} MB)",
                size as f64 / (1024.0 * 1024.0)
            ),
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

/// Format a file size in human-readable form (B, KB, MB)
fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_bytes_range() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_kb_range() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
    }

    #[test]
    fn format_size_mb_range() {
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.0 MB");
    }
}
