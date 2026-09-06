use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
    Frame,
};

use super::highlight::Highlighter;
use super::styles;
use super::utils::word_wrap;
use er_engine::ai::{CommentRef, CommentType, Finding, RiskLevel};
use er_engine::app::{App, DiffMode, SplitSide, TabState};
use er_engine::config::ErConfig;
use er_engine::git::{DiffHunk, DiffLine, LineType};

/// Expand tab characters to spaces based on configured tab width.
/// Uses column-aware expansion (tabs align to tab stops, not fixed width).
fn expand_tabs(line: &str, tab_width: u8) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let tw = (tab_width as usize).max(1);
    let mut result = String::with_capacity(line.len() + 16);
    let mut col = 0;
    for ch in line.chars() {
        if ch == '\t' {
            let spaces = tw - (col % tw);
            for _ in 0..spaces {
                result.push(' ');
            }
            col += spaces;
        } else {
            result.push(ch);
            col += 1;
        }
    }
    result
}

/// Threshold (total diff lines) above which viewport-based rendering is used
const VIRTUALIZE_THRESHOLD: usize = 200;

/// Threshold above which a "large file" warning is shown in the title
const LARGE_FILE_WARNING_LINES: usize = 2000;

/// Blank unified gutter matching `format!("{} {} \u{2502}", "    ", "    ")`
const BLANK_UNIFIED_GUTTER: &str = "          \u{2502}";

/// Blank split gutter matching `format!("{} \u{2502}", "    ")`
const BLANK_SPLIT_GUTTER: &str = "     \u{2502}";

/// A paired row in split diff view. Both panes advance `logical_line` by the same
/// `row_height` so they stay vertically aligned regardless of wrap asymmetry.
struct TuiSplitRow<'a> {
    left: Option<SplitCell<'a>>,
    right: Option<SplitCell<'a>>,
}

struct SplitCell<'a> {
    line_idx: usize,
    line: &'a er_engine::git::DiffLine,
}

/// Build aligned split rows from a hunk, pairing consecutive del/add runs by index.
/// Mirrors `splitRows()` in `desktop-ui/src/lib/splitRows.ts`.
fn build_split_rows(hunk: &er_engine::git::DiffHunk) -> Vec<TuiSplitRow<'_>> {
    let mut rows = Vec::new();
    let mut i = 0;
    let lines = &hunk.lines;
    while i < lines.len() {
        match lines[i].line_type {
            LineType::Context | LineType::Fold(_) => {
                rows.push(TuiSplitRow {
                    left: Some(SplitCell {
                        line_idx: i,
                        line: &lines[i],
                    }),
                    right: Some(SplitCell {
                        line_idx: i,
                        line: &lines[i],
                    }),
                });
                i += 1;
            }
            LineType::Delete | LineType::Add => {
                let mut del_idxs: Vec<usize> = Vec::new();
                while i < lines.len() && lines[i].line_type == LineType::Delete {
                    del_idxs.push(i);
                    i += 1;
                }
                let mut add_idxs: Vec<usize> = Vec::new();
                while i < lines.len() && lines[i].line_type == LineType::Add {
                    add_idxs.push(i);
                    i += 1;
                }
                let max_len = del_idxs.len().max(add_idxs.len());
                for k in 0..max_len {
                    rows.push(TuiSplitRow {
                        left: del_idxs.get(k).map(|&idx| SplitCell {
                            line_idx: idx,
                            line: &lines[idx],
                        }),
                        right: add_idxs.get(k).map(|&idx| SplitCell {
                            line_idx: idx,
                            line: &lines[idx],
                        }),
                    });
                }
            }
        }
    }
    rows
}

/// Whether a comment should render given layer visibility toggles.
const fn comment_layer_visible(tab: &TabState, comment: &CommentRef<'_>) -> bool {
    let visible = match comment {
        CommentRef::Question(_) | CommentRef::Note(_) => tab.layers.show_questions,
        CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => tab.layers.show_github_comments,
    };
    visible && !(tab.layers.hide_resolved && comment.is_resolved())
}

/// Resolve comments for a hunk using line-range fallback (matches desktop).
///
/// Uses the @@ header span (`hunk.new_count` / `hunk.old_count`), not the
/// visible Context+Add count. j/k auto-expand refetches tiny files with
/// full-file context; `fold_context_lines` then hides the long runs, so the
/// visible count is much smaller than the header span. A GitHub comment on an
/// add after the fold (still at its real `new_num`) would miss the hunk until
/// the next refresh if we matched on visible lines.
fn comments_for_hunk_resolved<'a>(
    tab: &'a TabState,
    path: &str,
    hunk_idx: usize,
    hunk: &DiffHunk,
) -> Vec<CommentRef<'a>> {
    tab.ai.comments_for_diff_hunk(path, hunk_idx, hunk)
}

fn hunk_level_comments<'a>(anchors: &'a [CommentRef<'a>]) -> Vec<&'a CommentRef<'a>> {
    anchors
        .iter()
        .filter(|c| c.in_reply_to().is_none() && c.line_start().is_none())
        .collect()
}

fn line_comments_at<'a>(anchors: &'a [CommentRef<'a>], line_num: usize) -> Vec<&'a CommentRef<'a>> {
    anchors
        .iter()
        .filter(|c| c.in_reply_to().is_none() && c.line_start() == Some(line_num))
        .collect()
}

/// Matches desktop `findingRendersInline` / `skipDelDuplicate`: when a line was
/// modified (delete + add at the same new-side number), render the comment once
/// on the add/context row, not again on the delete row.
fn should_render_inline_line_comment(
    diff_line: &DiffLine,
    hunk: &DiffHunk,
    line_num: usize,
) -> bool {
    if diff_line.new_num == Some(line_num) {
        return true;
    }
    if diff_line.old_num == Some(line_num)
        && matches!(diff_line.line_type, LineType::Delete)
        && hunk.lines.iter().any(|l| l.new_num == Some(line_num))
    {
        return false;
    }
    diff_line.old_num == Some(line_num)
}

/// Line-anchored findings to render inline for a given diff mode.
///
/// `Branch` matches by `hunk_index` (the review was generated against this exact
/// branch diff, so hunk indices align). Every other diff that a review can be
/// viewed against — a working-tree diff (`Unstaged`/`Staged`) or a PR head-vs-base
/// diff (`PrDiff`, incl. remote `gh pr diff`) — may not share the review's hunk
/// indexing, so those match by line only. This mirrors the desktop's
/// `findingMatchesHunk` rule (`mode !== "branch"` → line-only). Routing all four
/// render sites through this one dispatch keeps them from drifting apart.
fn line_findings_for_mode<'a>(
    ai: &'a er_engine::ai::AiState,
    mode: DiffMode,
    path: &str,
    hunk_idx: usize,
    new_line_num: usize,
) -> Vec<&'a Finding> {
    match mode {
        DiffMode::Branch => ai.findings_for_line(path, hunk_idx, new_line_num),
        DiffMode::Unstaged | DiffMode::Staged | DiffMode::PrDiff => {
            ai.findings_for_line_by_range(path, new_line_num)
        }
        DiffMode::History | DiffMode::Conflicts | DiffMode::Hidden | DiffMode::Tour => vec![],
    }
}

/// Hunk-level findings (no line anchor) to render after a hunk, for a given diff
/// mode. Same `Branch`-exact vs everything-else-by-range dispatch as
/// [`line_findings_for_mode`].
fn hunk_findings_for_mode<'a>(
    ai: &'a er_engine::ai::AiState,
    mode: DiffMode,
    path: &str,
    new_start: usize,
    new_count: usize,
    hunk_idx: usize,
    total_hunks: usize,
) -> Vec<&'a Finding> {
    match mode {
        DiffMode::Branch => ai.findings_for_hunk(path, hunk_idx, total_hunks),
        DiffMode::Unstaged | DiffMode::Staged | DiffMode::PrDiff => {
            ai.findings_for_hunk_by_line_range(path, new_start, new_count, hunk_idx, total_hunks)
        }
        DiffMode::History | DiffMode::Conflicts | DiffMode::Hidden | DiffMode::Tour => vec![],
    }
}

/// Number of terminal rows this cell will occupy given wrapping settings.
/// Uses the same pipeline as rendering: `expand_tabs` then `word_wrap`.
fn cell_wrap_height(
    cell: Option<&SplitCell<'_>>,
    wrap: bool,
    wrap_width: usize,
    tab_width: u8,
) -> usize {
    match cell {
        None => 1,
        Some(c) => {
            if let LineType::Fold(_) = c.line.line_type {
                return 1;
            }
            if wrap && !c.line.content.is_empty() {
                let expanded = expand_tabs(&c.line.content, tab_width);
                word_wrap(&expanded, wrap_width.max(1)).len().max(1)
            } else {
                1
            }
        }
    }
}

/// Pad `lines` with empty BG-styled rows so the Paragraph fills the entire visible area.
/// Without this, Ratatui's double-buffer reuses the previous frame's cell content for rows
/// below the Paragraph's last text line, causing stale content to bleed through.
fn pad_lines_to_fill(lines: &mut Vec<Line<'_>>, scroll_y: u16, visible_height: u16) {
    let needed = scroll_y as usize + visible_height as usize;
    while lines.len() < needed {
        lines.push(Line::from("").style(ratatui::style::Style::default().bg(styles::BG())));
    }
}

/// Render the diff view panel (right side)
pub fn render(f: &mut Frame, area: Rect, app: &App, hl: &mut Highlighter) {
    let tab = app.tab();

    // History mode: render multi-file commit diff
    if tab.mode == DiffMode::History {
        render_history_diff(f, area, app, hl);
        return;
    }

    // Tour mode: render the pillar-grouped walkthrough diff
    if tab.mode == DiffMode::Tour {
        render_tour_diff(f, area, app, hl);
        return;
    }

    // Check if a watched file is selected
    if let Some(watched) = tab.selected_watched_file() {
        render_watched(f, area, app, &watched.path, watched.size);
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

    let context_suffix = match tab.context_overrides.get(&file.path).copied() {
        Some(99999) => " [full context]".to_string(),
        Some(n) if n != 3 => format!(" [context: {}]", n),
        _ => String::new(),
    };
    let title = if total_diff_lines > LARGE_FILE_WARNING_LINES {
        format!(
            " {} \u{26a0} +{} lines{} ",
            file.path, total_diff_lines, context_suffix
        )
    } else {
        format!(" {}{} ", file.path, context_suffix)
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
            styles::status_style(&file.status),
        ),
        Span::styled(
            &file.path,
            ratatui::style::Style::default().fg(styles::BRIGHT()),
        ),
        Span::styled(
            format!("  +{} -{}", file.adds, file.dels),
            ratatui::style::Style::default().fg(styles::DIM()),
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
                    RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE()),
                }
            };
            let risk_label = match fr.risk {
                RiskLevel::High => "HIGH",
                RiskLevel::Medium => "MED",
                RiskLevel::Low => "LOW",
                RiskLevel::Info => "INFO",
            };
            header_spans.push(Span::styled("  ", ratatui::style::Style::default()));
            header_spans.push(Span::styled(format!("Risk {risk_label}"), risk_style));
            if !fr.risk_reason.is_empty() {
                header_spans.push(Span::styled(
                    format!(" — {}", fr.risk_reason),
                    ratatui::style::Style::default().fg(styles::DIM()),
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
                        ratatui::style::Style::default().fg(styles::MUTED()),
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
                CommentRef::Question(_) | CommentRef::Note(_) => tab.layers.show_questions,
                CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => {
                    tab.layers.show_github_comments
                }
            };
            if !visible {
                continue;
            }
            if tab.layers.hide_resolved && comment.is_resolved() {
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
        let hunk_anchors = comments_for_hunk_resolved(tab, &file.path, hunk_idx, hunk);

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
                                .fg(styles::CYAN())
                                .bg(styles::HUNK_BG())
                        } else {
                            ratatui::style::Style::default()
                                .fg(styles::DIM())
                                .bg(styles::HUNK_BG())
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
            for comment in hunk_level_comments(&hunk_anchors) {
                if !comment_layer_visible(tab, comment) {
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
            // Fold lines are rendered as a single "··· N lines ···" indicator.
            if let LineType::Fold(hidden) = diff_line.line_type {
                if logical_line >= render_start && logical_line < render_end {
                    let fold_text = format!(" ··· {} lines ···", hidden);
                    let fold_style = ratatui::style::Style::default().fg(styles::MUTED());
                    lines.push(Line::from(vec![Span::styled(fold_text, fold_style)]));
                }
                logical_line += 1;
                continue;
            }

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
                    LineType::Fold(_) => unreachable!(),
                }
            } else {
                match diff_line.line_type {
                    LineType::Add => ("+", styles::add_style()),
                    LineType::Delete => ("-", styles::del_style()),
                    LineType::Context => (" ", styles::default_style()),
                    LineType::Fold(_) => unreachable!(),
                }
            };

            let gutter_style = if is_selected_line {
                ratatui::style::Style::default()
                    .fg(styles::BRIGHT())
                    .bg(styles::LINE_CURSOR_BG())
            } else {
                match diff_line.line_type {
                    LineType::Add => ratatui::style::Style::default()
                        .fg(styles::DIM())
                        .bg(styles::ADD_BG()),
                    LineType::Delete => ratatui::style::Style::default()
                        .fg(styles::DIM())
                        .bg(styles::DEL_BG()),
                    LineType::Context => ratatui::style::Style::default().fg(styles::DIM()),
                    LineType::Fold(_) => unreachable!(),
                }
            };

            if wrap_lines && !diff_line.content.is_empty() {
                // Wrap the content and emit multiple logical lines.
                // Segments are owned Strings; highlight them and convert to Span<'static>
                // so they can safely outlive the local `segments` Vec.
                let content = expand_tabs(&diff_line.content, app.config.display.tab_width);
                let segments = word_wrap(&content, unified_wrap_width.max(1));
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
                                Span::styled(BLANK_UNIFIED_GUTTER, gutter_style),
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
                        spans.push(Span::styled(" ".repeat(area.width as usize), base_style));
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
                        let content = expand_tabs(&diff_line.content, app.config.display.tab_width);
                        let highlighted: Vec<Span<'static>> = hl
                            .highlight_line(&content, &file.path, base_style)
                            .into_iter()
                            .map(|s| Span::styled(s.content.into_owned(), s.style))
                            .collect();
                        spans.extend(highlighted);
                    }

                    spans.push(Span::styled(" ".repeat(area.width as usize), base_style));
                    lines.push(Line::from(spans).style(base_style));
                }
                logical_line += 1;
            }

            // ── Inline line comments (rendered directly after the target line) ──
            if let Some(line_num) = diff_line.new_num.or(diff_line.old_num) {
                if !should_render_inline_line_comment(diff_line, hunk, line_num) {
                    continue;
                }
                for comment in line_comments_at(&hunk_anchors, line_num) {
                    if !comment_layer_visible(tab, comment) {
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

            // ── Inline line-level findings (rendered after comments for the target line) ──
            if in_overlay {
                if let Some(new_line_num) = diff_line.new_num {
                    let line_findings = line_findings_for_mode(
                        &tab.ai,
                        tab.mode,
                        &file.path,
                        hunk_idx,
                        new_line_num,
                    );
                    let file_stale = tab.ai.is_file_stale(&file.path);
                    for finding in &line_findings {
                        let is_focused = tab.focused_finding_id.as_deref() == Some(&finding.id);
                        let pre_len = lines.len();
                        render_finding_banner(
                            &mut lines, finding, area.width, file_stale, is_focused,
                        );
                        let finding_line_count = lines.len() - pre_len;
                        if logical_line < render_start || logical_line >= render_end {
                            lines.truncate(pre_len);
                        }
                        logical_line += finding_line_count;

                        // Render response comments for this finding
                        let finding_comments = tab.ai.comments_for_finding(&finding.id);
                        for fc in &finding_comments {
                            if !tab.layers.show_github_comments {
                                continue;
                            }
                            if tab.layers.hide_resolved && fc.is_resolved() {
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
            }
        }

        // ── AI finding banners after each hunk (hunk-level only, overlay mode) ──
        if in_overlay {
            let total_hunks = file.hunks.len();
            let findings = hunk_findings_for_mode(
                &tab.ai,
                tab.mode,
                &file.path,
                hunk.new_start,
                hunk.new_count,
                hunk_idx,
                total_hunks,
            );
            for finding in &findings {
                let is_focused = tab.focused_finding_id.as_deref() == Some(&finding.id);
                let pre_len = lines.len();
                render_finding_banner(&mut lines, finding, area.width, file_stale, is_focused);
                let finding_line_count = lines.len() - pre_len;
                if logical_line < render_start || logical_line >= render_end {
                    lines.truncate(pre_len);
                }
                logical_line += finding_line_count;

                // Render response comments for this finding
                let finding_comments = tab.ai.comments_for_finding(&finding.id);
                for fc in &finding_comments {
                    if !tab.layers.show_github_comments {
                        continue;
                    }
                    if tab.layers.hide_resolved && fc.is_resolved() {
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

        // Gap indicator or blank line between hunks
        if logical_line >= render_start && logical_line < render_end {
            let gap = if hunk_idx + 1 < file.hunks.len() {
                let next = &file.hunks[hunk_idx + 1];
                next.old_start
                    .saturating_sub(hunk.old_start + hunk.old_count)
            } else {
                0
            };
            if gap > 0 {
                lines.push(Line::from(Span::styled(
                    format!("  ··· {} lines hidden (+/- to expand) ···", gap),
                    ratatui::style::Style::default().fg(styles::MUTED()),
                )));
            } else {
                lines.push(Line::from(""));
            }
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
                        && !(tab.layers.hide_resolved && q.resolved)
                    {
                        v.push(CommentRef::Question(q));
                    }
                }
            }
            if let Some(ns) = &tab.ai.notes {
                for n in &ns.notes {
                    if n.file == file.path
                        && n.anchor_status == "lost"
                        && n.hunk_index.is_some_and(|hi| hi >= num_hunks)
                        && tab.layers.show_questions
                        && !(tab.layers.hide_resolved && n.resolved)
                    {
                        v.push(CommentRef::Note(n));
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
                        && !(tab.layers.hide_resolved && c.resolved)
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
                    ratatui::style::Style::default().fg(styles::MUTED()),
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
            ratatui::style::Style::default().fg(styles::BRIGHT()),
        ))
        .title_alignment(ratatui::layout::Alignment::Left)
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG()))
        .padding(Padding::new(0, 1, 0, 0));

    // Apply scroll: for virtualized rendering, adjust scroll to offset into the rendered window.
    // When wrap_lines is enabled, disable horizontal scroll (lines fit within the viewport).
    let effective_h_scroll = if app.config.display.wrap_lines {
        0
    } else {
        tab.h_scroll
    };
    let visible_scroll = if use_viewport {
        let scroll_into_rendered = scroll.saturating_sub(render_start) as u16;
        (scroll_into_rendered, effective_h_scroll)
    } else {
        (tab.active_diff_scroll(), effective_h_scroll)
    };

    // Pre-slice to visible rows instead of relying on Paragraph::scroll() for vertical
    // dimension. This guarantees every row has explicit Line content — no empty rows left
    // for Block/Clear to fill, preventing stale content bleed-through.
    let inner_height = block.inner(area).height as usize;
    let scroll_y = visible_scroll.0 as usize;
    let visible_end = (scroll_y + inner_height).min(lines.len());
    let mut visible_lines: Vec<Line> = if scroll_y < lines.len() {
        lines.drain(scroll_y..visible_end).collect()
    } else {
        Vec::new()
    };
    let bg_line = Line::from("").style(ratatui::style::Style::default().bg(styles::BG()));
    while visible_lines.len() < inner_height {
        visible_lines.push(bg_line.clone());
    }
    let paragraph = Paragraph::new(visible_lines)
        .block(block)
        .scroll((0, effective_h_scroll));

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);

    // Sticky file path header: when the user scrolls down far enough that the file header
    // (logical_line=0) leaves the viewport, pin it at the top row of the diff area so
    // context is never lost. Only shown when scroll > 0 (header is off-screen).
    if scroll > 0 {
        let sticky_bg = styles::PANEL();
        let mut sticky_spans: Vec<Span> = vec![
            Span::styled(
                format!("  {} ", file.status.symbol()),
                styles::status_style(&file.status),
            ),
            Span::styled(
                file.path.clone(),
                ratatui::style::Style::default()
                    .fg(styles::BRIGHT())
                    .bg(sticky_bg),
            ),
            Span::styled(
                format!("  +{} -{}", file.adds, file.dels),
                ratatui::style::Style::default()
                    .fg(styles::DIM())
                    .bg(sticky_bg),
            ),
        ];
        // Pad to fill the full width so the background covers the row
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

    // Render hunk indicator overlay in top-right corner
    if total_hunks > 0 {
        let indicator_text = format!("Hunk {}/{}", tab.active_current_hunk() + 1, total_hunks);
        let indicator_width = indicator_text.len() + 3;
        let indicator_area = Rect {
            x: area.x + area.width.saturating_sub(indicator_width as u16 + 1),
            y: area.y,
            width: indicator_width as u16,
            height: 1,
        };
        let indicator = Paragraph::new(Line::from(Span::styled(
            indicator_text,
            ratatui::style::Style::default().fg(styles::MUTED()),
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
        .style(ratatui::style::Style::default().bg(styles::BG()));

    let inner = block.inner(area);
    f.render_widget(Clear, area);
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
    let h_scroll = if tab.mode == er_engine::app::DiffMode::History {
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
                    styles::status_style(&file.status),
                ),
                Span::styled(
                    &file.path,
                    ratatui::style::Style::default().fg(styles::BRIGHT()),
                ),
                Span::styled(
                    format!("  +{} -{}", file.adds, file.dels),
                    ratatui::style::Style::default().fg(styles::DIM()),
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

    // File-level (unanchored) comments — New side renders, Old side pads with blanks.
    {
        let unanchored = tab.ai.comments_for_file_unanchored(&file.path);
        for comment in &unanchored {
            let visible = match comment {
                CommentRef::Question(_) | CommentRef::Note(_) => tab.layers.show_questions,
                CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => {
                    tab.layers.show_github_comments
                }
            };
            if !visible {
                continue;
            }
            if tab.layers.hide_resolved && comment.is_resolved() {
                continue;
            }

            if side == SplitSide::New {
                let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
                let pre_len = lines.len();
                render_comment_lines(&mut lines, comment, inner.width, false, is_focused);
                let n = lines.len() - pre_len;
                if logical_line < render_start || logical_line >= render_end {
                    lines.truncate(pre_len);
                }
                logical_line += n;
            } else {
                let mut tmp: Vec<Line> = Vec::new();
                render_comment_lines(&mut tmp, comment, inner.width, false, false);
                let n = tmp.len();
                for k in 0..n {
                    if logical_line + k >= render_start && logical_line + k < render_end {
                        lines.push(
                            Line::from("").style(ratatui::style::Style::default().bg(styles::BG())),
                        );
                    }
                }
                logical_line += n;
            }

            let replies = tab.ai.replies_to(comment.id());
            for reply in &replies {
                if side == SplitSide::New {
                    let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                    let pre_len = lines.len();
                    render_reply_lines(&mut lines, reply, inner.width, false, is_focused);
                    let n = lines.len() - pre_len;
                    if logical_line < render_start || logical_line >= render_end {
                        lines.truncate(pre_len);
                    }
                    logical_line += n;
                } else {
                    let mut tmp: Vec<Line> = Vec::new();
                    render_reply_lines(&mut tmp, reply, inner.width, false, false);
                    let n = tmp.len();
                    for k in 0..n {
                        if logical_line + k >= render_start && logical_line + k < render_end {
                            lines.push(
                                Line::from("")
                                    .style(ratatui::style::Style::default().bg(styles::BG())),
                            );
                        }
                    }
                    logical_line += n;
                }
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
        let hunk_anchors = comments_for_hunk_resolved(tab, &file.path, hunk_idx, hunk);

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
                                .fg(styles::CYAN())
                                .bg(styles::HUNK_BG())
                        } else {
                            ratatui::style::Style::default()
                                .fg(styles::DIM())
                                .bg(styles::HUNK_BG())
                        },
                    ),
                    Span::styled(&hunk.header, styles::hunk_header_style()),
                ])
                .style(styles::hunk_header_style()),
            );
        }
        logical_line += 1;

        // Hunk-level comments — New side renders, Old side pads with blanks.
        {
            for comment in hunk_level_comments(&hunk_anchors) {
                if !comment_layer_visible(tab, comment) {
                    continue;
                }

                if side == SplitSide::New {
                    let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
                    let pre_len = lines.len();
                    render_comment_lines(&mut lines, comment, inner.width, false, is_focused);
                    let n = lines.len() - pre_len;
                    if logical_line < render_start || logical_line >= render_end {
                        lines.truncate(pre_len);
                    }
                    logical_line += n;
                } else {
                    let mut tmp: Vec<Line> = Vec::new();
                    render_comment_lines(&mut tmp, comment, inner.width, false, false);
                    let n = tmp.len();
                    for k in 0..n {
                        if logical_line + k >= render_start && logical_line + k < render_end {
                            lines.push(
                                Line::from("")
                                    .style(ratatui::style::Style::default().bg(styles::BG())),
                            );
                        }
                    }
                    logical_line += n;
                }

                let replies = tab.ai.replies_to(comment.id());
                for reply in &replies {
                    if side == SplitSide::New {
                        let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                        let pre_len = lines.len();
                        render_reply_lines(&mut lines, reply, inner.width, false, is_focused);
                        let n = lines.len() - pre_len;
                        if logical_line < render_start || logical_line >= render_end {
                            lines.truncate(pre_len);
                        }
                        logical_line += n;
                    } else {
                        let mut tmp: Vec<Line> = Vec::new();
                        render_reply_lines(&mut tmp, reply, inner.width, false, false);
                        let n = tmp.len();
                        for k in 0..n {
                            if logical_line + k >= render_start && logical_line + k < render_end {
                                lines.push(
                                    Line::from("")
                                        .style(ratatui::style::Style::default().bg(styles::BG())),
                                );
                            }
                        }
                        logical_line += n;
                    }
                }
            }
        }

        // Hunk lines — build aligned split rows so both panes advance logical_line identically.
        let split_rows = build_split_rows(hunk);
        for row in &split_rows {
            let cell = match side {
                SplitSide::Old => row.left.as_ref(),
                SplitSide::New => row.right.as_ref(),
            };
            let other_cell = match side {
                SplitSide::Old => row.right.as_ref(),
                SplitSide::New => row.left.as_ref(),
            };

            // ── Fold rows ─────────────────────────────────────────────────────────
            let is_fold = cell
                .or(other_cell)
                .is_some_and(|c| matches!(c.line.line_type, LineType::Fold(_)));
            if is_fold {
                if logical_line >= render_start && logical_line < render_end {
                    if side == SplitSide::New {
                        if let Some(c) = cell {
                            if let LineType::Fold(hidden) = c.line.line_type {
                                let fold_text = format!(" ··· {} lines ···", hidden);
                                lines.push(Line::from(vec![Span::styled(
                                    fold_text,
                                    ratatui::style::Style::default().fg(styles::MUTED()),
                                )]));
                            }
                        }
                    } else {
                        lines.push(
                            Line::from("").style(ratatui::style::Style::default().bg(styles::BG())),
                        );
                    }
                }
                logical_line += 1;
                continue;
            }

            // ── Compute aligned row height ─────────────────────────────────────────
            let left_h = cell_wrap_height(
                row.left.as_ref(),
                wrap_lines,
                split_wrap_width,
                app.config.display.tab_width,
            );
            let right_h = cell_wrap_height(
                row.right.as_ref(),
                wrap_lines,
                split_wrap_width,
                app.config.display.tab_width,
            );
            let row_height = left_h.max(right_h);
            let this_h = match side {
                SplitSide::Old => left_h,
                SplitSide::New => right_h,
            };

            // Background for blank cells / continuation rows on this side.
            let blank_bg = match cell {
                Some(c) => match c.line.line_type {
                    LineType::Add => styles::ADD_BG(),
                    LineType::Delete => styles::DEL_BG(),
                    LineType::Context | LineType::Fold(_) => styles::BG(),
                },
                None => match other_cell {
                    Some(oc) => match oc.line.line_type {
                        LineType::Add => styles::ADD_BG(),
                        LineType::Delete => styles::DEL_BG(),
                        _ => styles::BG(),
                    },
                    None => styles::BG(),
                },
            };

            // ── Render this side's cell ────────────────────────────────────────────
            if let Some(c) = cell {
                let diff_line = c.line;
                let line_idx = c.line_idx;
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
                        LineType::Fold(_) => unreachable!(),
                    }
                } else {
                    match diff_line.line_type {
                        LineType::Add => ("+", styles::add_style()),
                        LineType::Delete => ("-", styles::del_style()),
                        LineType::Context => (" ", styles::default_style()),
                        LineType::Fold(_) => unreachable!(),
                    }
                };

                let gutter_style = if is_selected_line {
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT())
                        .bg(styles::LINE_CURSOR_BG())
                } else {
                    match diff_line.line_type {
                        LineType::Add => ratatui::style::Style::default()
                            .fg(styles::DIM())
                            .bg(styles::ADD_BG()),
                        LineType::Delete => ratatui::style::Style::default()
                            .fg(styles::DIM())
                            .bg(styles::DEL_BG()),
                        LineType::Context => ratatui::style::Style::default().fg(styles::DIM()),
                        LineType::Fold(_) => unreachable!(),
                    }
                };

                if wrap_lines && !diff_line.content.is_empty() {
                    let content = expand_tabs(&diff_line.content, app.config.display.tab_width);
                    let segments = word_wrap(&content, split_wrap_width.max(1));
                    for (seg_idx, segment) in segments.iter().enumerate() {
                        if logical_line + seg_idx >= render_start
                            && logical_line + seg_idx < render_end
                        {
                            let mut spans: Vec<Span<'static>> = if seg_idx == 0 {
                                vec![
                                    Span::styled(format!("{} \u{2502}", num_str), gutter_style),
                                    Span::styled(prefix, base_style),
                                ]
                            } else {
                                vec![
                                    Span::styled(BLANK_SPLIT_GUTTER, gutter_style),
                                    Span::styled(" ", base_style),
                                ]
                            };
                            let highlighted: Vec<Span<'static>> = hl
                                .highlight_line(segment, &file.path, base_style)
                                .into_iter()
                                .map(|s| Span::styled(s.content.into_owned(), s.style))
                                .collect();
                            spans.extend(highlighted);
                            lines.push(Line::from(spans).style(base_style));
                        }
                    }
                } else if logical_line >= render_start && logical_line < render_end {
                    let mut spans = vec![
                        Span::styled(format!("{} \u{2502}", num_str), gutter_style),
                        Span::styled(prefix, base_style),
                    ];
                    if diff_line.content.is_empty() {
                        spans.push(Span::styled("", base_style));
                    } else {
                        let content = expand_tabs(&diff_line.content, app.config.display.tab_width);
                        let highlighted: Vec<Span<'static>> = hl
                            .highlight_line(&content, &file.path, base_style)
                            .into_iter()
                            .map(|s| Span::styled(s.content.into_owned(), s.style))
                            .collect();
                        spans.extend(highlighted);
                    }
                    lines.push(Line::from(spans).style(base_style));
                }
            } else {
                // Blank placeholder — other side has content here.
                if logical_line >= render_start && logical_line < render_end {
                    lines.push(
                        Line::from(Span::styled(
                            "",
                            ratatui::style::Style::default().bg(blank_bg),
                        ))
                        .style(ratatui::style::Style::default().bg(blank_bg)),
                    );
                }
            }

            // ── Blank continuation rows to match the taller side ──────────────────
            for extra in this_h..row_height {
                let ll = logical_line + extra;
                if ll >= render_start && ll < render_end {
                    lines.push(Line::from("").style(ratatui::style::Style::default().bg(blank_bg)));
                }
            }
            logical_line += row_height;

            // ── Inline line comments — anchored to the focused side's line number ──
            // New side renders; Old side emits matching blank padding.
            let comment_line_num = match side {
                SplitSide::New => row
                    .right
                    .as_ref()
                    .and_then(|c| c.line.new_num)
                    .or_else(|| row.left.as_ref().and_then(|c| c.line.old_num)),
                SplitSide::Old => row
                    .right
                    .as_ref()
                    .and_then(|c| c.line.new_num)
                    .or_else(|| row.left.as_ref().and_then(|c| c.line.old_num)),
            };
            if let Some(line_num) = comment_line_num {
                for comment in line_comments_at(&hunk_anchors, line_num) {
                    if !comment_layer_visible(tab, comment) {
                        continue;
                    }

                    if side == SplitSide::New {
                        let is_focused = tab.focused_comment_id.as_deref() == Some(comment.id());
                        let pre_len = lines.len();
                        render_comment_lines(&mut lines, comment, inner.width, true, is_focused);
                        let n = lines.len() - pre_len;
                        if logical_line < render_start || logical_line >= render_end {
                            lines.truncate(pre_len);
                        }
                        logical_line += n;
                    } else {
                        let mut tmp: Vec<Line> = Vec::new();
                        render_comment_lines(&mut tmp, comment, inner.width, true, false);
                        let n = tmp.len();
                        for k in 0..n {
                            if logical_line + k >= render_start && logical_line + k < render_end {
                                lines.push(
                                    Line::from("")
                                        .style(ratatui::style::Style::default().bg(styles::BG())),
                                );
                            }
                        }
                        logical_line += n;
                    }

                    let replies = tab.ai.replies_to(comment.id());
                    for reply in &replies {
                        if side == SplitSide::New {
                            let is_focused = tab.focused_comment_id.as_deref() == Some(reply.id());
                            let pre_len = lines.len();
                            render_reply_lines(&mut lines, reply, inner.width, true, is_focused);
                            let n = lines.len() - pre_len;
                            if logical_line < render_start || logical_line >= render_end {
                                lines.truncate(pre_len);
                            }
                            logical_line += n;
                        } else {
                            let mut tmp: Vec<Line> = Vec::new();
                            render_reply_lines(&mut tmp, reply, inner.width, true, false);
                            let n = tmp.len();
                            for k in 0..n {
                                if logical_line + k >= render_start && logical_line + k < render_end
                                {
                                    lines.push(
                                        Line::from("").style(
                                            ratatui::style::Style::default().bg(styles::BG()),
                                        ),
                                    );
                                }
                            }
                            logical_line += n;
                        }
                    }
                }
            }

            // ── Inline line findings — anchored to new_num (right/New cell) ───────
            let finding_new_num = row.right.as_ref().and_then(|c| c.line.new_num);
            if let Some(new_line_num) = finding_new_num {
                if tab.layers.show_ai_findings {
                    let line_findings = line_findings_for_mode(
                        &tab.ai,
                        tab.mode,
                        &file.path,
                        hunk_idx,
                        new_line_num,
                    );
                    let file_stale = tab.ai.is_file_stale(&file.path);
                    for finding in &line_findings {
                        if side == SplitSide::New {
                            let is_focused = tab.focused_finding_id.as_deref() == Some(&finding.id);
                            let pre_len = lines.len();
                            render_finding_banner(
                                &mut lines,
                                finding,
                                inner.width,
                                file_stale,
                                is_focused,
                            );
                            let n = lines.len() - pre_len;
                            if logical_line < render_start || logical_line >= render_end {
                                lines.truncate(pre_len);
                            }
                            logical_line += n;
                        } else {
                            let mut tmp: Vec<Line> = Vec::new();
                            render_finding_banner(
                                &mut tmp,
                                finding,
                                inner.width,
                                file_stale,
                                false,
                            );
                            let n = tmp.len();
                            for k in 0..n {
                                if logical_line + k >= render_start && logical_line + k < render_end
                                {
                                    lines.push(
                                        Line::from("").style(
                                            ratatui::style::Style::default().bg(styles::BG()),
                                        ),
                                    );
                                }
                            }
                            logical_line += n;
                        }
                    }
                }
            }
        }

        // AI finding banners — hunk-level; New side renders, Old side pads.
        if tab.layers.show_ai_findings {
            let file_stale = tab.ai.is_file_stale(&file.path);
            let total_hunks = file.hunks.len();
            let findings = hunk_findings_for_mode(
                &tab.ai,
                tab.mode,
                &file.path,
                hunk.new_start,
                hunk.new_count,
                hunk_idx,
                total_hunks,
            );
            for finding in &findings {
                if side == SplitSide::New {
                    let is_focused = tab.focused_finding_id.as_deref() == Some(&finding.id);
                    let pre_len = lines.len();
                    render_finding_banner(&mut lines, finding, inner.width, file_stale, is_focused);
                    let n = lines.len() - pre_len;
                    if logical_line < render_start || logical_line >= render_end {
                        lines.truncate(pre_len);
                    }
                    logical_line += n;
                } else {
                    let mut tmp: Vec<Line> = Vec::new();
                    render_finding_banner(&mut tmp, finding, inner.width, file_stale, false);
                    let n = tmp.len();
                    for k in 0..n {
                        if logical_line + k >= render_start && logical_line + k < render_end {
                            lines.push(
                                Line::from("")
                                    .style(ratatui::style::Style::default().bg(styles::BG())),
                            );
                        }
                    }
                    logical_line += n;
                }
            }
        }

        // Gap indicator or blank line between hunks
        if logical_line >= render_start && logical_line < render_end {
            let gap = if hunk_idx + 1 < file.hunks.len() {
                let next = &file.hunks[hunk_idx + 1];
                next.old_start
                    .saturating_sub(hunk.old_start + hunk.old_count)
            } else {
                0
            };
            if gap > 0 {
                lines.push(Line::from(Span::styled(
                    format!("  ··· {} lines hidden (+/- to expand) ···", gap),
                    ratatui::style::Style::default().fg(styles::MUTED()),
                )));
            } else {
                lines.push(Line::from(""));
            }
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
    let visible_scroll = if use_viewport {
        let scroll_into_rendered = scroll.saturating_sub(render_start) as u16;
        (scroll_into_rendered, effective_h_scroll)
    } else {
        (tab.active_diff_scroll(), effective_h_scroll)
    };

    // Pre-slice to visible rows — same fix as unified render path.
    let inner_height = inner.height as usize;
    let scroll_y = visible_scroll.0 as usize;
    let visible_end = (scroll_y + inner_height).min(lines.len());
    let mut visible_lines: Vec<Line> = if scroll_y < lines.len() {
        lines.drain(scroll_y..visible_end).collect()
    } else {
        Vec::new()
    };
    let bg_line = Line::from("").style(ratatui::style::Style::default().bg(styles::BG()));
    while visible_lines.len() < inner_height {
        visible_lines.push(bg_line.clone());
    }
    let paragraph = Paragraph::new(visible_lines).scroll((0, effective_h_scroll));

    f.render_widget(paragraph, inner);

    // Sticky file path header for the New side: when the file header at logical_line=0 has
    // scrolled off the top of the inner viewport, overlay a 1-row sticky header so the user
    // always knows which file they're reviewing. Old side shows a blank line at logical_line=0
    // so no sticky header is needed there.
    if side == SplitSide::New && scroll > 0 {
        let sticky_bg = styles::PANEL();
        let mut sticky_spans: Vec<Span> = vec![
            Span::styled(
                format!("  {} ", file.status.symbol()),
                styles::status_style(&file.status),
            ),
            Span::styled(
                file.path.clone(),
                ratatui::style::Style::default()
                    .fg(styles::BRIGHT())
                    .bg(sticky_bg),
            ),
            Span::styled(
                format!("  +{} -{}", file.adds, file.dels),
                ratatui::style::Style::default()
                    .fg(styles::DIM())
                    .bg(sticky_bg),
            ),
        ];
        // Pad to fill the full inner width
        let sticky_len: usize = sticky_spans.iter().map(|s| s.content.chars().count()).sum();
        let sticky_remaining = (inner.width as usize).saturating_sub(sticky_len);
        sticky_spans.push(Span::styled(
            " ".repeat(sticky_remaining),
            ratatui::style::Style::default().bg(sticky_bg),
        ));
        let sticky_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(Line::from(sticky_spans)), sticky_area);
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
        let commit = match history.commits.get(history.selected_commit) {
            Some(c) => c,
            None => {
                render_history_empty(f, area, "No commit selected");
                return;
            }
        };
        render_history_empty(f, area, &format!("Empty commit: {}", commit.short_hash));
        return;
    }

    let commit = match history.commits.get(history.selected_commit) {
        Some(c) => c,
        None => {
            render_history_empty(f, area, "No commit selected");
            return;
        }
    };
    let title = format!(" {} · {} ", commit.short_hash, commit.subject);
    let total_files = history.commit_files.len();

    let block = Block::default()
        .title(Span::styled(
            title,
            ratatui::style::Style::default().fg(styles::BRIGHT()),
        ))
        .title_alignment(ratatui::layout::Alignment::Left)
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG()))
        .padding(Padding::new(0, 1, 0, 0));

    // Viewport-based rendering, same idea as the unified render path: line
    // positions are computed arithmetically and only the visible window is
    // materialized, so a commit touching hundreds of files doesn't allocate
    // (or syntax-highlight) thousands of Lines per frame.
    let inner_height = block.inner(area).height as usize;
    let scroll_y = history.diff_scroll as usize;
    let render_end = scroll_y + inner_height;

    let mut visible_lines: Vec<Line> = Vec::with_capacity(inner_height);
    // Absolute index of the next logical line; only lines whose index falls
    // inside [scroll_y, render_end) are built.
    let mut cursor: usize = 0;
    // Track the absolute line index where each file header starts (for the
    // sticky header below).
    let mut file_header_line_indices: Vec<usize> = Vec::new();

    macro_rules! emit {
        ($line:expr $(,)?) => {{
            if cursor >= scroll_y && cursor < render_end {
                visible_lines.push($line);
            }
            cursor += 1;
        }};
    }

    // Render each file as a section
    for (file_idx, file) in history.commit_files.iter().enumerate() {
        let is_current_file = file_idx == history.selected_file;

        // Each file occupies: header + blank, then per hunk: header + lines + gap/blank.
        let file_line_count: usize =
            2 + file.hunks.iter().map(|h| 2 + h.lines.len()).sum::<usize>();
        if cursor + file_line_count <= scroll_y || cursor >= render_end {
            file_header_line_indices.push(cursor);
            cursor += file_line_count;
            continue;
        }

        // File header
        let file_header_bg = if is_current_file {
            styles::HUNK_BG()
        } else {
            styles::BG()
        };

        let mut header_spans = vec![
            Span::styled(
                if is_current_file { " ▶ " } else { "   " },
                ratatui::style::Style::default()
                    .fg(if is_current_file {
                        styles::CYAN()
                    } else {
                        styles::DIM()
                    })
                    .bg(file_header_bg),
            ),
            Span::styled(
                format!("{} ", file.status.symbol()),
                match &file.status {
                    er_engine::git::FileStatus::Added => ratatui::style::Style::default()
                        .fg(styles::GREEN())
                        .bg(file_header_bg),
                    er_engine::git::FileStatus::Deleted => ratatui::style::Style::default()
                        .fg(styles::RED())
                        .bg(file_header_bg),
                    _ => ratatui::style::Style::default()
                        .fg(styles::YELLOW())
                        .bg(file_header_bg),
                },
            ),
            Span::styled(
                &file.path,
                ratatui::style::Style::default()
                    .fg(if is_current_file {
                        styles::BRIGHT()
                    } else {
                        styles::TEXT()
                    })
                    .bg(file_header_bg),
            ),
            Span::styled(
                format!("  +{} -{}", file.adds, file.dels),
                ratatui::style::Style::default()
                    .fg(styles::DIM())
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

        file_header_line_indices.push(cursor);
        emit!(Line::from(header_spans));
        emit!(Line::from(""));

        // Render hunks for this file
        for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
            let hunk_line_count = 2 + hunk.lines.len();
            if cursor + hunk_line_count <= scroll_y || cursor >= render_end {
                cursor += hunk_line_count;
                continue;
            }
            let is_current_hunk = is_current_file && hunk_idx == history.current_hunk;

            // Hunk header
            let marker = if is_current_hunk { "▶" } else { " " };
            emit!(Line::from(vec![
                Span::styled(
                    format!(" {} ", marker),
                    if is_current_hunk {
                        ratatui::style::Style::default()
                            .fg(styles::CYAN())
                            .bg(styles::HUNK_BG())
                    } else {
                        ratatui::style::Style::default()
                            .fg(styles::DIM())
                            .bg(styles::HUNK_BG())
                    },
                ),
                Span::styled(&hunk.header, styles::hunk_header_style()),
            ])
            .style(styles::hunk_header_style()),);

            // Hunk lines
            for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
                // Skip per-line formatting and highlighting outside the window.
                if cursor < scroll_y || cursor >= render_end {
                    cursor += 1;
                    continue;
                }
                if let LineType::Fold(hidden) = diff_line.line_type {
                    let fold_text = format!(" ··· {} lines ···", hidden);
                    let fold_style = ratatui::style::Style::default().fg(styles::MUTED());
                    emit!(Line::from(vec![Span::styled(fold_text, fold_style)]));
                    continue;
                }

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
                        LineType::Fold(_) => unreachable!(),
                    }
                } else {
                    match diff_line.line_type {
                        LineType::Add => ("+", styles::add_style()),
                        LineType::Delete => ("-", styles::del_style()),
                        LineType::Context => (" ", styles::default_style()),
                        LineType::Fold(_) => unreachable!(),
                    }
                };

                let gutter_style = if is_selected_line {
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT())
                        .bg(styles::LINE_CURSOR_BG())
                } else {
                    match diff_line.line_type {
                        LineType::Add => ratatui::style::Style::default()
                            .fg(styles::DIM())
                            .bg(styles::ADD_BG()),
                        LineType::Delete => ratatui::style::Style::default()
                            .fg(styles::DIM())
                            .bg(styles::DEL_BG()),
                        LineType::Context => ratatui::style::Style::default().fg(styles::DIM()),
                        LineType::Fold(_) => unreachable!(),
                    }
                };

                let mut spans = vec![
                    Span::styled(format!("{} {} │", old_num, new_num), gutter_style),
                    Span::styled(prefix, base_style),
                ];

                if diff_line.content.is_empty() {
                    spans.push(Span::styled("", base_style));
                } else {
                    let content = expand_tabs(&diff_line.content, app.config.display.tab_width);
                    let highlighted: Vec<Span<'static>> = hl
                        .highlight_line(&content, &file.path, base_style)
                        .into_iter()
                        .map(|s| Span::styled(s.content.into_owned(), s.style))
                        .collect();
                    spans.extend(highlighted);
                }

                emit!(Line::from(spans).style(base_style));
            }

            // Gap indicator or blank line between hunks
            let gap = if hunk_idx + 1 < file.hunks.len() {
                let next = &file.hunks[hunk_idx + 1];
                next.old_start
                    .saturating_sub(hunk.old_start + hunk.old_count)
            } else {
                0
            };
            if gap > 0 {
                emit!(Line::from(Span::styled(
                    format!("  ··· {} lines hidden ···", gap),
                    ratatui::style::Style::default().fg(styles::MUTED()),
                )));
            } else {
                emit!(Line::from(""));
            }
        }
    }

    let bg_line = Line::from("").style(ratatui::style::Style::default().bg(styles::BG()));
    while visible_lines.len() < inner_height {
        visible_lines.push(bg_line.clone());
    }
    let paragraph = Paragraph::new(visible_lines)
        .block(block)
        .scroll((0, history.h_scroll));

    f.render_widget(Clear, area);
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
        // (i.e., the scroll position is past the header line itself). The bounds
        // check guards against commit_files changing length between the header
        // index build and this read.
        let header_line = file_header_line_indices[topmost_file_idx];
        if let Some(file) = history
            .commit_files
            .get(topmost_file_idx)
            .filter(|_| scroll > header_line)
        {
            let sticky_bg = styles::PANEL();

            let mut sticky_spans = vec![
                Span::styled(
                    format!("{} ", file.status.symbol()),
                    match &file.status {
                        er_engine::git::FileStatus::Added => ratatui::style::Style::default()
                            .fg(styles::GREEN())
                            .bg(sticky_bg),
                        er_engine::git::FileStatus::Deleted => ratatui::style::Style::default()
                            .fg(styles::RED())
                            .bg(sticky_bg),
                        _ => ratatui::style::Style::default()
                            .fg(styles::YELLOW())
                            .bg(sticky_bg),
                    },
                ),
                Span::styled(
                    &file.path,
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT())
                        .bg(sticky_bg),
                ),
                Span::styled(
                    format!("  +{} -{}", file.adds, file.dels),
                    ratatui::style::Style::default()
                        .fg(styles::DIM())
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
                y: area.y + 1,
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
            ratatui::style::Style::default().fg(styles::MUTED()),
        )));
        f.render_widget(indicator, indicator_area);
    }
}

/// Render the Tour walkthrough diff: the branch diff reordered/grouped by
/// pillar. Files are concatenated like History mode; the current pillar's title
/// and description are pinned at the top of the viewport as a sticky header.
fn render_tour_diff(f: &mut Frame, area: Rect, app: &App, hl: &mut Highlighter) {
    let tab = app.tab();
    let tour = match tab.tour.as_ref() {
        Some(t) => t,
        None => {
            render_history_empty(f, area, "No tour available — run /er-tour to generate one");
            return;
        }
    };
    if tour.files.is_empty() {
        render_history_empty(f, area, "Tour has no files in this diff");
        return;
    }

    let current_pillar = tour.pillars.get(tour.selected_pillar);
    let pillar_title = current_pillar.map(|p| p.title.clone()).unwrap_or_default();
    let pillar_desc = current_pillar
        .map(|p| p.description.clone())
        .unwrap_or_default();

    // Sticky pillar header: title row + wrapped description (capped height).
    let desc_width = (area.width as usize).saturating_sub(2).max(10);
    let desc_lines = word_wrap(&pillar_desc, desc_width);
    let max_desc_rows = 4usize;
    let shown_desc = desc_lines.len().min(max_desc_rows);
    // header rows = title (1) + desc rows + separator (1)
    let header_rows = 1 + shown_desc + 1;

    let title = format!(
        " TOUR · pillar {}/{} ",
        tour.selected_pillar + 1,
        tour.pillars.len().max(1)
    );

    let block = Block::default()
        .title(Span::styled(
            title,
            ratatui::style::Style::default().fg(styles::BRIGHT()),
        ))
        .title_alignment(ratatui::layout::Alignment::Left)
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG()))
        .padding(Padding::new(0, 1, 0, 0));

    let inner_height = block.inner(area).height as usize;
    // Reserve the top rows for the sticky pillar header.
    let diff_height = inner_height.saturating_sub(header_rows);
    let scroll_y = tour.diff_scroll as usize;
    let render_end = scroll_y + diff_height;

    let mut visible_lines: Vec<Line> = Vec::with_capacity(diff_height);
    let mut cursor: usize = 0;

    macro_rules! emit {
        ($line:expr $(,)?) => {{
            if cursor >= scroll_y && cursor < render_end {
                visible_lines.push($line);
            }
            cursor += 1;
        }};
    }

    for (file_idx, file) in tour.files.iter().enumerate() {
        let is_current_file = file_idx == tour.selected_file;
        let file_line_count: usize =
            2 + file.hunks.iter().map(|h| 2 + h.lines.len()).sum::<usize>();
        if cursor + file_line_count <= scroll_y || cursor >= render_end {
            cursor += file_line_count;
            continue;
        }

        let file_header_bg = if is_current_file {
            styles::HUNK_BG()
        } else {
            styles::BG()
        };
        let reviewed = tab.reviewed.contains_key(&file.path);
        let mut header_spans = vec![
            Span::styled(
                if is_current_file { " ▶ " } else { "   " },
                ratatui::style::Style::default()
                    .fg(if is_current_file {
                        styles::CYAN()
                    } else {
                        styles::DIM()
                    })
                    .bg(file_header_bg),
            ),
            Span::styled(
                format!("{} ", file.status.symbol()),
                match &file.status {
                    er_engine::git::FileStatus::Added => ratatui::style::Style::default()
                        .fg(styles::GREEN())
                        .bg(file_header_bg),
                    er_engine::git::FileStatus::Deleted => ratatui::style::Style::default()
                        .fg(styles::RED())
                        .bg(file_header_bg),
                    _ => ratatui::style::Style::default()
                        .fg(styles::YELLOW())
                        .bg(file_header_bg),
                },
            ),
            Span::styled(
                &file.path,
                ratatui::style::Style::default()
                    .fg(if is_current_file {
                        styles::BRIGHT()
                    } else {
                        styles::TEXT()
                    })
                    .bg(file_header_bg),
            ),
            Span::styled(
                format!("  +{} -{}", file.adds, file.dels),
                ratatui::style::Style::default()
                    .fg(styles::DIM())
                    .bg(file_header_bg),
            ),
        ];
        // Mark co-located related files (tests/styles/…) with a "↳" so the
        // nesting shown in the pillar list is also visible in the diff body.
        if tour.file_is_related.get(file_idx).copied().unwrap_or(false) {
            header_spans.insert(
                1,
                Span::styled(
                    "↳ ",
                    ratatui::style::Style::default()
                        .fg(styles::DIM())
                        .bg(file_header_bg),
                ),
            );
        }
        if reviewed {
            header_spans.push(Span::styled(
                "  ✓ reviewed",
                ratatui::style::Style::default()
                    .fg(styles::GREEN())
                    .bg(file_header_bg),
            ));
        }
        let header_len: usize = header_spans.iter().map(|s| s.content.chars().count()).sum();
        let remaining = (area.width as usize).saturating_sub(header_len);
        header_spans.push(Span::styled(
            " ".repeat(remaining),
            ratatui::style::Style::default().bg(file_header_bg),
        ));
        emit!(Line::from(header_spans));
        emit!(Line::from(""));

        for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
            let hunk_line_count = 2 + hunk.lines.len();
            if cursor + hunk_line_count <= scroll_y || cursor >= render_end {
                cursor += hunk_line_count;
                continue;
            }
            let is_current_hunk = is_current_file && hunk_idx == tour.current_hunk;
            let marker = if is_current_hunk { "▶" } else { " " };
            emit!(Line::from(vec![
                Span::styled(
                    format!(" {} ", marker),
                    if is_current_hunk {
                        ratatui::style::Style::default()
                            .fg(styles::CYAN())
                            .bg(styles::HUNK_BG())
                    } else {
                        ratatui::style::Style::default()
                            .fg(styles::DIM())
                            .bg(styles::HUNK_BG())
                    },
                ),
                Span::styled(&hunk.header, styles::hunk_header_style()),
            ])
            .style(styles::hunk_header_style()));

            for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
                if cursor < scroll_y || cursor >= render_end {
                    cursor += 1;
                    continue;
                }
                if let LineType::Fold(hidden) = diff_line.line_type {
                    let fold_text = format!(" ··· {} lines ···", hidden);
                    let fold_style = ratatui::style::Style::default().fg(styles::MUTED());
                    emit!(Line::from(vec![Span::styled(fold_text, fold_style)]));
                    continue;
                }
                let is_selected_line = is_current_hunk && tour.current_line == Some(line_idx);
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
                        LineType::Fold(_) => unreachable!(),
                    }
                } else {
                    match diff_line.line_type {
                        LineType::Add => ("+", styles::add_style()),
                        LineType::Delete => ("-", styles::del_style()),
                        LineType::Context => (" ", styles::default_style()),
                        LineType::Fold(_) => unreachable!(),
                    }
                };
                let gutter_style = if is_selected_line {
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT())
                        .bg(styles::LINE_CURSOR_BG())
                } else {
                    match diff_line.line_type {
                        LineType::Add => ratatui::style::Style::default()
                            .fg(styles::DIM())
                            .bg(styles::ADD_BG()),
                        LineType::Delete => ratatui::style::Style::default()
                            .fg(styles::DIM())
                            .bg(styles::DEL_BG()),
                        LineType::Context => ratatui::style::Style::default().fg(styles::DIM()),
                        LineType::Fold(_) => unreachable!(),
                    }
                };
                let mut spans = vec![
                    Span::styled(format!("{} {} │", old_num, new_num), gutter_style),
                    Span::styled(prefix, base_style),
                ];
                if diff_line.content.is_empty() {
                    spans.push(Span::styled("", base_style));
                } else {
                    let content = expand_tabs(&diff_line.content, app.config.display.tab_width);
                    let highlighted: Vec<Span<'static>> = hl
                        .highlight_line(&content, &file.path, base_style)
                        .into_iter()
                        .map(|s| Span::styled(s.content.into_owned(), s.style))
                        .collect();
                    spans.extend(highlighted);
                }
                emit!(Line::from(spans).style(base_style));
            }

            let gap = if hunk_idx + 1 < file.hunks.len() {
                let next = &file.hunks[hunk_idx + 1];
                next.old_start
                    .saturating_sub(hunk.old_start + hunk.old_count)
            } else {
                0
            };
            if gap > 0 {
                emit!(Line::from(Span::styled(
                    format!("  ··· {} lines hidden ···", gap),
                    ratatui::style::Style::default().fg(styles::MUTED()),
                )));
            } else {
                emit!(Line::from(""));
            }
        }
    }

    let bg_line = Line::from("").style(ratatui::style::Style::default().bg(styles::BG()));
    while visible_lines.len() < diff_height {
        visible_lines.push(bg_line.clone());
    }

    // Diff area sits below the sticky pillar header.
    let diff_area = Rect {
        x: area.x,
        y: area.y + header_rows as u16,
        width: area.width,
        height: area.height.saturating_sub(header_rows as u16),
    };
    let paragraph = Paragraph::new(visible_lines)
        .block(
            Block::default()
                .borders(Borders::NONE)
                .style(ratatui::style::Style::default().bg(styles::BG()))
                .padding(Padding::new(0, 1, 0, 0)),
        )
        .scroll((0, tour.h_scroll));
    f.render_widget(Clear, area);
    f.render_widget(paragraph, diff_area);

    // Sticky pillar header overlay (title + description) pinned at the very top.
    let header_bg = styles::PANEL();
    let mut header_lines: Vec<Line> = Vec::with_capacity(header_rows);
    header_lines.push(Line::from(Span::styled(
        format!(" {} ", pillar_title),
        ratatui::style::Style::default()
            .fg(styles::BRIGHT())
            .bg(header_bg),
    )));
    for d in desc_lines.iter().take(max_desc_rows) {
        header_lines.push(Line::from(Span::styled(
            format!(" {}", d),
            ratatui::style::Style::default()
                .fg(styles::TEXT())
                .bg(header_bg),
        )));
    }
    header_lines.push(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        ratatui::style::Style::default().fg(styles::BORDER()),
    )));
    let header_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: header_rows as u16,
    };
    f.render_widget(
        Paragraph::new(header_lines).style(ratatui::style::Style::default().bg(header_bg)),
        header_area,
    );

    // File indicator overlay in the top-right corner.
    let indicator_text = format!("File {}/{}", tour.selected_file + 1, tour.files.len());
    let indicator_width = indicator_text.len() + 3;
    let indicator_area = Rect {
        x: area.x + area.width.saturating_sub(indicator_width as u16 + 1),
        y: area.y,
        width: indicator_width as u16,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            indicator_text,
            ratatui::style::Style::default()
                .fg(styles::MUTED())
                .bg(styles::PANEL()),
        ))),
        indicator_area,
    );
}

/// Render empty state for history mode
fn render_history_empty(f: &mut Frame, area: Rect, message: &str) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG()));

    let mut empty_lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", message),
            ratatui::style::Style::default().fg(styles::MUTED()),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Switch modes with [1] [2] [3]",
            ratatui::style::Style::default().fg(styles::DIM()),
        )),
    ];
    pad_lines_to_fill(&mut empty_lines, 0, area.height);
    let text = Paragraph::new(empty_lines).block(block);

    f.render_widget(Clear, area);
    f.render_widget(text, area);
}

/// Render a compacted file summary
fn render_compacted(f: &mut Frame, area: Rect, file: &er_engine::git::DiffFile) {
    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", file.path),
            ratatui::style::Style::default().fg(styles::BRIGHT()),
        ))
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG()))
        .padding(Padding::new(0, 1, 0, 0));

    let hunks_label = if file.raw_hunk_count > 0 {
        format!("  {} hunks", file.raw_hunk_count)
    } else {
        String::new()
    };

    let mut compacted_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  \u{1f4e6} ",
                ratatui::style::Style::default().fg(styles::MUTED()),
            ),
            Span::styled(
                &file.path,
                ratatui::style::Style::default().fg(styles::TEXT()),
            ),
            Span::styled(
                format!("  +{} \u{2212}{}{}", file.adds, file.dels, hunks_label),
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  (compacted \u{2014} press Enter to expand)",
            ratatui::style::Style::default().fg(styles::MUTED()),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Lock files, generated code, and large diffs are",
            ratatui::style::Style::default().fg(styles::DIM()),
        )),
        Line::from(Span::styled(
            "  compacted automatically to save memory.",
            ratatui::style::Style::default().fg(styles::DIM()),
        )),
    ];
    pad_lines_to_fill(&mut compacted_lines, 0, area.height);
    let text = Paragraph::new(compacted_lines).block(block);

    f.render_widget(Clear, area);
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
    let ctype = comment.comment_type();
    let is_question = ctype == CommentType::Question;
    let is_note = ctype == CommentType::Note;
    let is_stale = comment.is_stale();
    let anchor = comment.anchor_status();
    let is_lost = anchor == "lost";

    let bg = if focused {
        styles::COMMENT_FOCUS_BG()
    } else if inline {
        styles::INLINE_COMMENT_BG()
    } else {
        styles::COMMENT_BG()
    };

    // Questions and notes use yellow, GitHub comments use cyan
    let accent = if is_stale {
        styles::STALE()
    } else if is_question || is_note {
        styles::YELLOW()
    } else {
        styles::CYAN()
    };

    let icon = if is_note {
        "📝"
    } else if is_question {
        "❓"
    } else {
        "💬"
    };
    let author = comment.author();

    if inline {
        // ── GitHub-style inline block ──

        // Top label: "  ╭─ 💬 R33 ────────────────────────────────"
        let line_label = if let Some(ln) = comment.line_start() {
            format!("  \u{256d}\u{2500} {} R{}  ", icon, ln)
        } else {
            format!("  \u{256d}\u{2500} {}  ", icon)
        };
        let label_cols = Span::raw(&line_label).width();
        let fill_len = (width as usize).saturating_sub(label_cols);
        lines.push(
            Line::from(vec![
                Span::styled(
                    line_label,
                    ratatui::style::Style::default().fg(accent).bg(bg),
                ),
                Span::styled(
                    "\u{2500}".repeat(fill_len),
                    ratatui::style::Style::default().fg(styles::BORDER()).bg(bg),
                ),
            ])
            .style(ratatui::style::Style::default().bg(bg)),
        );

        // Author line: "  │  VilfredSikker  14:30:00  [badges]"
        let bar = if focused {
            "  \u{25b8}\u{2502}  "
        } else {
            "  \u{2502}  "
        };
        let mut author_spans = vec![
            Span::styled(
                bar,
                ratatui::style::Style::default()
                    .fg(if focused { styles::PURPLE() } else { accent })
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
        let ts = comment.timestamp();
        if !ts.is_empty() {
            let time_part = ts.split('T').nth(1).unwrap_or("").trim_end_matches('Z');
            author_spans.push(Span::styled(
                format!("  {}", time_part),
                ratatui::style::Style::default().fg(styles::DIM()).bg(bg),
            ));
        }
        if anchor == "relocated" {
            author_spans.push(Span::styled(
                "  \u{21aa} moved",
                ratatui::style::Style::default()
                    .fg(styles::RELOCATED_INDICATOR())
                    .bg(bg),
            ));
        } else if is_lost {
            author_spans.push(Span::styled(
                "  ? lost",
                ratatui::style::Style::default()
                    .fg(styles::LOST_INDICATOR())
                    .bg(bg),
            ));
        }
        if is_stale {
            author_spans.push(Span::styled(
                "  \u{26a0} stale",
                ratatui::style::Style::default().fg(styles::STALE()).bg(bg),
            ));
        }
        if comment.is_resolved() {
            author_spans.push(Span::styled(
                "  \u{2713} resolved",
                ratatui::style::Style::default().fg(styles::GREEN()).bg(bg),
            ));
        }
        if comment.comment_type() == CommentType::GitHubComment {
            if comment.is_synced() {
                author_spans.push(Span::styled(
                    "  \u{2191} synced",
                    ratatui::style::Style::default().fg(styles::GREEN()).bg(bg),
                ));
            } else {
                author_spans.push(Span::styled(
                    "  \u{2191} local",
                    ratatui::style::Style::default().fg(styles::DIM()).bg(bg),
                ));
            }
        }
        if focused {
            author_spans.push(Span::styled(
                "  \u{25c6}",
                ratatui::style::Style::default()
                    .fg(styles::PURPLE())
                    .bg(bg)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ));
        }
        lines.push(Line::from(author_spans).style(ratatui::style::Style::default().bg(bg)));

        // Text lines: "  │   text..."
        let bar_style = ratatui::style::Style::default().fg(accent).bg(bg);
        let indent_str = "  \u{2502}  ";
        let max_len = (width as usize).saturating_sub(indent_str.len() + 2);
        let text = comment.text();
        let text_fg = if is_stale || is_lost {
            styles::DIM()
        } else {
            styles::TEXT()
        };
        for wrapped in word_wrap(text, max_len) {
            lines.push(
                Line::from(vec![
                    Span::styled(indent_str, bar_style),
                    Span::styled(
                        format!("  {}", wrapped),
                        ratatui::style::Style::default().fg(text_fg).bg(bg),
                    ),
                ])
                .style(ratatui::style::Style::default().bg(bg)),
            );
        }

        // Bottom border: "  ╰──────────────────────────────────────────"
        let bottom_fill = (width as usize).saturating_sub(3);
        lines.push(
            Line::from(vec![
                Span::styled(
                    "  \u{2570}",
                    ratatui::style::Style::default().fg(accent).bg(bg),
                ),
                Span::styled(
                    "\u{2500}".repeat(bottom_fill),
                    ratatui::style::Style::default().fg(styles::BORDER()).bg(bg),
                ),
            ])
            .style(ratatui::style::Style::default().bg(bg)),
        );
        return;
    }

    // ── Non-inline (panel / hunk-level) style ──
    let mut header_spans = vec![
        Span::styled(
            if focused {
                format!("\u{25b8} {} ", icon)
            } else {
                format!("  {} ", icon)
            },
            ratatui::style::Style::default()
                .fg(if focused { styles::PURPLE() } else { accent })
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
            ratatui::style::Style::default().fg(styles::DIM()).bg(bg),
        ));
    }

    // Relocated/lost anchor indicators
    if anchor == "relocated" {
        header_spans.push(Span::styled(
            "  \u{21aa} moved",
            ratatui::style::Style::default()
                .fg(styles::RELOCATED_INDICATOR())
                .bg(bg),
        ));
    } else if is_lost {
        header_spans.push(Span::styled(
            "  ? lost",
            ratatui::style::Style::default()
                .fg(styles::LOST_INDICATOR())
                .bg(bg),
        ));
    }

    // Stale indicator
    if is_stale {
        header_spans.push(Span::styled(
            "  \u{26a0} stale",
            ratatui::style::Style::default().fg(styles::STALE()).bg(bg),
        ));
    }

    // Resolved indicator
    if comment.is_resolved() {
        header_spans.push(Span::styled(
            "  \u{2713} resolved",
            ratatui::style::Style::default().fg(styles::GREEN()).bg(bg),
        ));
    }

    // Synced indicator (GitHub comments only)
    if comment.is_synced() {
        header_spans.push(Span::styled(
            "  \u{2191} synced",
            ratatui::style::Style::default().fg(styles::GREEN()).bg(bg),
        ));
    } else if comment.comment_type() == CommentType::GitHubComment {
        header_spans.push(Span::styled(
            "  \u{2191} local",
            ratatui::style::Style::default().fg(styles::DIM()).bg(bg),
        ));
    }

    // Focus indicator
    if focused {
        header_spans.push(Span::styled(
            "  \u{25c6} focused",
            ratatui::style::Style::default()
                .fg(styles::PURPLE())
                .bg(bg)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
    }

    lines.push(Line::from(header_spans).style(ratatui::style::Style::default().bg(bg)));

    // Comment text
    let max_len = width.saturating_sub(8) as usize;
    let text = comment.text();
    let text_fg = if is_stale || is_lost {
        styles::DIM()
    } else {
        styles::TEXT()
    };
    for wrapped in word_wrap(text, max_len) {
        lines.push(
            Line::from(vec![Span::styled(
                format!("      {}", wrapped),
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
        styles::COMMENT_FOCUS_BG()
    } else if inline {
        styles::INLINE_COMMENT_BG()
    } else {
        styles::COMMENT_BG()
    };

    let is_question = reply.comment_type() == CommentType::Question;
    let accent = if is_question {
        styles::YELLOW()
    } else {
        styles::CYAN()
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
            ratatui::style::Style::default().fg(styles::DIM()).bg(bg),
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
            ratatui::style::Style::default().fg(styles::DIM()).bg(bg),
        ));
    }

    if reply.is_synced() {
        header_spans.push(Span::styled(
            "  ↑ synced",
            ratatui::style::Style::default().fg(styles::GREEN()).bg(bg),
        ));
    } else if reply.comment_type() == CommentType::GitHubComment {
        header_spans.push(Span::styled(
            "  ↑ local",
            ratatui::style::Style::default().fg(styles::DIM()).bg(bg),
        ));
    }

    if focused {
        header_spans.push(Span::styled(
            "  ◆",
            ratatui::style::Style::default().fg(styles::PURPLE()).bg(bg),
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
                ratatui::style::Style::default().fg(styles::TEXT()).bg(bg),
            )])
            .style(ratatui::style::Style::default().bg(bg)),
        );
    }
}

/// Render an AI finding banner (title + description + suggestion)
fn render_finding_banner(
    lines: &mut Vec<Line<'_>>,
    finding: &Finding,
    width: u16,
    file_stale: bool,
    focused: bool,
) {
    let bg = if focused {
        styles::FINDING_FOCUS_BG()
    } else {
        styles::FINDING_BG()
    };

    let severity_style = if file_stale {
        styles::stale_style()
    } else {
        match finding.severity {
            RiskLevel::High => styles::risk_high(),
            RiskLevel::Medium => styles::risk_medium(),
            RiskLevel::Low => styles::risk_low(),
            RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE()),
        }
    };

    let stale_tag = if file_stale { " [stale]" } else { "" };

    let mut title_spans = vec![
        Span::styled(format!("  {} ", finding.severity.symbol()), severity_style),
        Span::styled(
            format!("[{}]", finding.category),
            ratatui::style::Style::default().fg(styles::DIM()).bg(bg),
        ),
        Span::styled(
            format!(" {}{}", finding.title, stale_tag),
            ratatui::style::Style::default().fg(styles::ORANGE()).bg(bg),
        ),
    ];
    if focused {
        title_spans.push(Span::styled(
            "  ◆ focused",
            ratatui::style::Style::default()
                .fg(styles::PURPLE())
                .bg(bg)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
    }

    lines.push(Line::from(title_spans).style(ratatui::style::Style::default().bg(bg)));

    if !finding.description.is_empty() {
        let desc = finding.description.lines().next().unwrap_or("");
        let max_len = width.saturating_sub(6) as usize;
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
                ratatui::style::Style::default().fg(styles::MUTED()).bg(bg),
            )])
            .style(ratatui::style::Style::default().bg(bg)),
        );
    }

    if !finding.suggestion.is_empty() {
        let sug = finding.suggestion.lines().next().unwrap_or("");
        let max_len = width.saturating_sub(8) as usize;
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
                ratatui::style::Style::default().fg(styles::GREEN()).bg(bg),
            )])
            .style(ratatui::style::Style::default().bg(bg)),
        );
    }
}

/// Render an empty state when no file is selected
fn render_empty(f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG()));

    let mut empty_lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "  No files changed",
            ratatui::style::Style::default().fg(styles::MUTED()),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Switch modes with [1] [2] [3]",
            ratatui::style::Style::default().fg(styles::DIM()),
        )),
    ];
    pad_lines_to_fill(&mut empty_lines, 0, area.height);
    let text = Paragraph::new(empty_lines).block(block);

    f.render_widget(Clear, area);
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
            ratatui::style::Style::default().fg(styles::WATCHED_TEXT()),
        ),
        Span::styled(
            format!("  ({})", status_label),
            ratatui::style::Style::default().fg(styles::WATCHED_MUTED()),
        ),
    ]));

    // Size info
    let size_str = format_size(size);
    lines.push(Line::from(Span::styled(
        format!("  Size: {}", size_str),
        ratatui::style::Style::default().fg(styles::WATCHED_MUTED()),
    )));

    lines.push(Line::from(""));

    // Check for snapshot diff mode
    let use_snapshot = tab.watched_config.diff_mode == "snapshot";

    if use_snapshot {
        // Try to get snapshot diff
        match er_engine::git::diff_watched_file_snapshot(
            repo_root,
            path,
            &tab.er_root.snapshots_dir(),
        ) {
            Ok(Some(raw)) if raw.is_empty() => {
                lines.push(Line::from(Span::styled(
                    "  No changes since snapshot",
                    ratatui::style::Style::default().fg(styles::MUTED()),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Press s to update snapshot",
                    ratatui::style::Style::default().fg(styles::DIM()),
                )));
            }
            Ok(Some(raw)) => {
                // Parse and render the diff
                let parsed = er_engine::git::parse_diff(&raw);
                if let Some(diff_file) = parsed.into_iter().next() {
                    lines.push(Line::from(Span::styled(
                        "  diff vs snapshot",
                        ratatui::style::Style::default().fg(styles::WATCHED_MUTED()),
                    )));
                    lines.push(Line::from(""));

                    // Render hunk data — use owned strings to avoid lifetime issues
                    for hunk in &diff_file.hunks {
                        lines.push(
                            Line::from(Span::styled(
                                format!("  {}", hunk.header),
                                styles::hunk_header_style(),
                            ))
                            .style(styles::hunk_header_style()),
                        );

                        for diff_line in &hunk.lines {
                            if let LineType::Fold(hidden) = diff_line.line_type {
                                let fold_text = format!(" ··· {} lines ···", hidden);
                                let fold_style =
                                    ratatui::style::Style::default().fg(styles::MUTED());
                                lines.push(Line::from(vec![Span::styled(fold_text, fold_style)]));
                                continue;
                            }

                            let (prefix, base_style) = match diff_line.line_type {
                                LineType::Add => ("+", styles::add_style()),
                                LineType::Delete => ("-", styles::del_style()),
                                LineType::Context => (" ", styles::default_style()),
                                LineType::Fold(_) => unreachable!(),
                            };
                            let gutter_style = match diff_line.line_type {
                                LineType::Add => ratatui::style::Style::default()
                                    .fg(styles::DIM())
                                    .bg(styles::ADD_BG()),
                                LineType::Delete => ratatui::style::Style::default()
                                    .fg(styles::DIM())
                                    .bg(styles::DEL_BG()),
                                LineType::Context => {
                                    ratatui::style::Style::default().fg(styles::DIM())
                                }
                                LineType::Fold(_) => unreachable!(),
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
                                Span::styled(
                                    expand_tabs(&diff_line.content, app.config.display.tab_width),
                                    base_style,
                                ),
                            ];
                            lines.push(Line::from(spans).style(base_style));
                        }
                        lines.push(Line::from(""));
                    }
                }
                lines.push(Line::from(Span::styled(
                    "  Press s to update snapshot",
                    ratatui::style::Style::default().fg(styles::DIM()),
                )));
            }
            Ok(None) => {
                // New file — snapshot just created
                lines.push(Line::from(Span::styled(
                    "  Snapshot saved (first view)",
                    ratatui::style::Style::default().fg(styles::GREEN()),
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
            ratatui::style::Style::default().fg(styles::WATCHED_TEXT()),
        ))
        .title_alignment(ratatui::layout::Alignment::Left)
        .borders(Borders::NONE)
        .style(ratatui::style::Style::default().bg(styles::BG()))
        .padding(Padding::new(0, 1, 0, 0));

    // Pre-slice to visible rows — same fix as unified render path.
    let inner_height = block.inner(area).height as usize;
    let scroll_y = tab.diff_scroll as usize;
    let visible_end = (scroll_y + inner_height).min(lines.len());
    let mut visible_lines: Vec<Line> = if scroll_y < lines.len() {
        lines.drain(scroll_y..visible_end).collect()
    } else {
        Vec::new()
    };
    let bg_line = Line::from("").style(ratatui::style::Style::default().bg(styles::BG()));
    while visible_lines.len() < inner_height {
        visible_lines.push(bg_line.clone());
    }
    let paragraph = Paragraph::new(visible_lines)
        .block(block)
        .scroll((0, tab.h_scroll));

    f.render_widget(Clear, area);
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
            ratatui::style::Style::default().fg(styles::MUTED()),
        )));
        return;
    }

    match er_engine::git::read_watched_file_content(repo_root, path) {
        Ok(Some(content)) => {
            let total_lines = content.lines().count();
            if total_lines > 10_000 {
                lines.push(Line::from(Span::styled(
                    format!("  Large file ({} lines) — content truncated", total_lines),
                    ratatui::style::Style::default().fg(styles::MUTED()),
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
                    ratatui::style::Style::default().fg(styles::MUTED()),
                )));
            }
        }
        Ok(None) => {
            // Binary file
            lines.push(Line::from(Span::styled(
                format!("  Binary file ({:.1} KB)", size as f64 / 1024.0),
                ratatui::style::Style::default().fg(styles::MUTED()),
            )));
        }
        Err(e) => {
            lines.push(Line::from(Span::styled(
                format!("  Error reading file: {}", e),
                ratatui::style::Style::default().fg(styles::RED()),
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
mod finding_dispatch_tests {
    use super::*;
    use er_engine::ai::AiState;

    // One file review with a line-anchored finding (line 30, hunk 1) and a
    // hunk-level finding (no line anchor, hunk 2).
    fn ai_with_findings() -> AiState {
        let json = r#"{
            "version": 1,
            "diff_hash": "h",
            "files": {
                "src/a.rs": {
                    "risk": "medium",
                    "findings": [
                        {"id":"f-line","severity":"medium","title":"line finding","hunk_index":1,"line_start":30},
                        {"id":"f-hunk","severity":"low","title":"hunk finding","hunk_index":2}
                    ]
                }
            }
        }"#;
        let mut ai = AiState::default();
        ai.review = Some(serde_json::from_str(json).expect("fixture parses"));
        ai
    }

    fn ids(findings: &[&Finding]) -> Vec<String> {
        findings.iter().map(|f| f.id.clone()).collect()
    }

    // Regression: before the fix, PrDiff returned `vec![]` for every finding, so
    // PR-review findings never rendered inline in the TUI (local `--pr` or remote
    // `--remote`) even though the file-tree count showed them. PrDiff must surface
    // line-anchored findings by line — ignoring hunk_index, since a PR / remote
    // `gh pr diff` may not share the review's hunk indexing.
    #[test]
    fn prdiff_surfaces_line_finding_ignoring_hunk_index() {
        let ai = ai_with_findings();
        // Query the WRONG hunk (0) for the finding anchored to hunk 1.
        let found = line_findings_for_mode(&ai, DiffMode::PrDiff, "src/a.rs", 0, 30);
        assert_eq!(ids(&found), vec!["f-line".to_string()]);
    }

    // Branch mode keeps exact hunk matching — the review was generated against
    // this same branch diff, so hunk indices align. A mismatched hunk must NOT
    // surface the finding; this is what makes the PrDiff branch a real difference.
    #[test]
    fn branch_requires_matching_hunk_for_line_finding() {
        let ai = ai_with_findings();
        let wrong_hunk = line_findings_for_mode(&ai, DiffMode::Branch, "src/a.rs", 0, 30);
        assert!(wrong_hunk.is_empty(), "branch must not match across hunks");
        let right_hunk = line_findings_for_mode(&ai, DiffMode::Branch, "src/a.rs", 1, 30);
        assert_eq!(ids(&right_hunk), vec!["f-line".to_string()]);
    }

    // Hunk-level (non-line-anchored) findings surface in PrDiff too.
    #[test]
    fn prdiff_surfaces_hunk_level_finding() {
        let ai = ai_with_findings();
        let found = hunk_findings_for_mode(&ai, DiffMode::PrDiff, "src/a.rs", 100, 5, 2, 3);
        assert_eq!(ids(&found), vec!["f-hunk".to_string()]);
    }

    // Modes that view a different diff than any review (History/Conflicts/Hidden/
    // Tour) still show no inline findings.
    #[test]
    fn non_review_modes_hide_findings() {
        let ai = ai_with_findings();
        for mode in [
            DiffMode::History,
            DiffMode::Conflicts,
            DiffMode::Hidden,
            DiffMode::Tour,
        ] {
            assert!(line_findings_for_mode(&ai, mode, "src/a.rs", 1, 30).is_empty());
            assert!(hunk_findings_for_mode(&ai, mode, "src/a.rs", 100, 5, 2, 3).is_empty());
        }
    }
}

/// j/k auto-expands tiny files to full-file context, then `fold_context_lines`
/// hides the long context runs. Line-range matching must use the @@ header
/// span (`hunk.new_count`), not the visible Context+Add count, or a GitHub
/// comment on an add after the fold disappears until the next refresh.
#[cfg(test)]
mod github_comment_fold_tests {
    use super::*;
    use er_engine::ai::{ErGitHubComments, GitHubReviewComment};
    use er_engine::git::{DiffFile, DiffHunk, DiffLine, FileStatus};

    fn line(lt: LineType, content: &str, old: Option<usize>, new: Option<usize>) -> DiffLine {
        DiffLine {
            line_type: lt,
            content: content.to_string(),
            old_num: old,
            new_num: new,
        }
    }

    fn visible_new_count(hunk: &DiffHunk) -> usize {
        hunk.lines
            .iter()
            .filter(|l| matches!(l.line_type, LineType::Context | LineType::Add))
            .count()
    }

    fn folded_full_file_hunk() -> DiffHunk {
        DiffHunk {
            header: "@@ -1,300 +1,310 @@".to_string(),
            old_start: 1,
            old_count: 300,
            new_start: 1,
            new_count: 310,
            lines: vec![
                line(LineType::Context, "keep-start-1", Some(1), Some(1)),
                line(LineType::Context, "keep-start-2", Some(2), Some(2)),
                line(LineType::Context, "keep-start-3", Some(3), Some(3)),
                line(LineType::Fold(280), "", None, None),
                line(LineType::Context, "near-1", Some(284), Some(284)),
                line(LineType::Context, "near-2", Some(285), Some(285)),
                line(LineType::Context, "near-3", Some(286), Some(286)),
                line(LineType::Add, "<Banner type=\"warning\">", None, Some(287)),
                line(LineType::Add, "deprecated now", None, Some(288)),
            ],
        }
    }

    fn gh_comment_on_banner() -> GitHubReviewComment {
        GitHubReviewComment {
            id: "gh-1".to_string(),
            timestamp: String::new(),
            file: "page.svelte".to_string(),
            hunk_index: Some(0),
            line_start: Some(288),
            line_end: None,
            line_content: "deprecated now".to_string(),
            comment: "technically it's being removed in 30 days".to_string(),
            in_reply_to: None,
            resolved: false,
            source: "github".to_string(),
            github_id: Some(1),
            author: "martin-kr".to_string(),
            synced: true,
            outdated: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: "@@ -180,20 +180,36 @@".to_string(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".to_string(),
        }
    }

    #[test]
    fn visible_new_count_excludes_folded_span_containing_the_comment_line() {
        let hunk = folded_full_file_hunk();
        let visible = visible_new_count(&hunk);
        // Exact values: 3 Context + 3 Context + 2 Add visible lines; comment at 288.
        assert_eq!(visible, 8);
        assert_eq!(hunk.new_start + visible, 9);
    }

    #[test]
    fn comments_for_hunk_resolved_keeps_comment_after_context_fold() {
        let hunk = folded_full_file_hunk();
        let file = DiffFile {
            path: "page.svelte".to_string(),
            status: FileStatus::Modified,
            hunks: vec![hunk],
            adds: 2,
            dels: 0,
            compacted: false,
            raw_hunk_count: 1,
        };
        let mut app = App::new_for_test(vec![file]);
        app.tab_mut().ai.github_comments = Some(ErGitHubComments {
            version: 1,
            diff_hash: "h".to_string(),
            github: None,
            comments: vec![gh_comment_on_banner()],
        });

        let tab = app.tab();
        let path = tab.files[0].path.clone();
        let hunk = tab.files[0].hunks[0].clone();
        let found = comments_for_hunk_resolved(tab, &path, 0, &hunk);
        assert_eq!(
            found.len(),
            1,
            "GitHub comment on an add after a folded context run must still attach"
        );
        assert_eq!(found[0].id(), "gh-1");
        assert_eq!(found[0].line_start(), Some(288));
    }
}

#[cfg(test)]
mod build_split_rows_tests {
    use super::*;
    use er_engine::git::{DiffHunk, DiffLine};

    fn make_line(lt: LineType, content: &str, old: Option<usize>, new: Option<usize>) -> DiffLine {
        DiffLine {
            line_type: lt,
            content: content.to_string(),
            old_num: old,
            new_num: new,
        }
    }

    fn make_hunk(lines: Vec<DiffLine>) -> DiffHunk {
        DiffHunk {
            header: String::new(),
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1,
            lines,
        }
    }

    #[test]
    fn build_split_rows_context_both_sides() {
        let hunk = make_hunk(vec![make_line(LineType::Context, "ctx", Some(1), Some(1))]);
        let rows = build_split_rows(&hunk);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].left.is_some());
        assert!(rows[0].right.is_some());
        assert_eq!(rows[0].left.as_ref().unwrap().line_idx, 0);
        assert_eq!(rows[0].right.as_ref().unwrap().line_idx, 0);
    }

    #[test]
    fn build_split_rows_equal_del_add_pair_by_index() {
        let hunk = make_hunk(vec![
            make_line(LineType::Delete, "del0", Some(1), None),
            make_line(LineType::Delete, "del1", Some(2), None),
            make_line(LineType::Add, "add0", None, Some(1)),
            make_line(LineType::Add, "add1", None, Some(2)),
        ]);
        let rows = build_split_rows(&hunk);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].left.as_ref().unwrap().line_idx, 0); // del0
        assert_eq!(rows[0].right.as_ref().unwrap().line_idx, 2); // add0
        assert_eq!(rows[1].left.as_ref().unwrap().line_idx, 1); // del1
        assert_eq!(rows[1].right.as_ref().unwrap().line_idx, 3); // add1
    }

    #[test]
    fn build_split_rows_extra_adds_become_right_only() {
        let hunk = make_hunk(vec![
            make_line(LineType::Delete, "del0", Some(1), None),
            make_line(LineType::Add, "add0", None, Some(1)),
            make_line(LineType::Add, "add1", None, Some(2)),
        ]);
        let rows = build_split_rows(&hunk);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].left.is_some());
        assert!(rows[0].right.is_some());
        assert!(rows[1].left.is_none());
        assert!(rows[1].right.is_some());
    }

    #[test]
    fn build_split_rows_extra_dels_become_left_only() {
        let hunk = make_hunk(vec![
            make_line(LineType::Delete, "del0", Some(1), None),
            make_line(LineType::Delete, "del1", Some(2), None),
            make_line(LineType::Add, "add0", None, Some(1)),
        ]);
        let rows = build_split_rows(&hunk);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].left.is_some());
        assert!(rows[0].right.is_some());
        assert!(rows[1].left.is_some());
        assert!(rows[1].right.is_none());
    }

    #[test]
    fn build_split_rows_standalone_add_right_only() {
        let hunk = make_hunk(vec![
            make_line(LineType::Context, "ctx", Some(1), Some(1)),
            make_line(LineType::Add, "add0", None, Some(2)),
        ]);
        let rows = build_split_rows(&hunk);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].left.is_some());
        assert!(rows[0].right.is_some());
        assert!(rows[1].left.is_none());
        assert!(rows[1].right.is_some());
    }

    #[test]
    fn build_split_rows_standalone_del_left_only() {
        let hunk = make_hunk(vec![
            make_line(LineType::Delete, "del0", Some(1), None),
            make_line(LineType::Context, "ctx", Some(2), Some(1)),
        ]);
        let rows = build_split_rows(&hunk);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].left.is_some());
        assert!(rows[0].right.is_none());
        assert!(rows[1].left.is_some());
        assert!(rows[1].right.is_some());
    }

    #[test]
    fn build_split_rows_fold_both_sides() {
        let hunk = make_hunk(vec![make_line(LineType::Fold(5), "", None, None)]);
        let rows = build_split_rows(&hunk);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].left.is_some());
        assert!(rows[0].right.is_some());
        assert_eq!(rows[0].left.as_ref().unwrap().line_idx, 0);
        assert_eq!(rows[0].right.as_ref().unwrap().line_idx, 0);
    }

    #[test]
    fn inline_comment_skips_delete_when_add_has_same_new_num() {
        let hunk = make_hunk(vec![
            make_line(LineType::Delete, "old", Some(5), None),
            make_line(LineType::Add, "new", None, Some(5)),
        ]);
        assert!(!should_render_inline_line_comment(&hunk.lines[0], &hunk, 5));
        assert!(should_render_inline_line_comment(&hunk.lines[1], &hunk, 5));
    }

    #[test]
    fn inline_comment_renders_on_delete_when_no_matching_new_num() {
        let hunk = make_hunk(vec![make_line(LineType::Delete, "gone", Some(10), None)]);
        assert!(should_render_inline_line_comment(&hunk.lines[0], &hunk, 10));
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

// ── Shared helpers for the render tests below ──────────────────────────────
//
// `tempfile` is not a dev-dependency of `er-tui`, so the watched-file tests
// roll a minimal throwaway-directory guard instead of adding one.

#[cfg(test)]
mod test_support {
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use ratatui::{Frame, Terminal};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Render into an off-screen terminal and hand back the painted buffer.
    pub fn draw(width: u16, height: u16, render: impl FnOnce(&mut Frame, Rect)) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        let area = Rect::new(0, 0, width, height);
        terminal
            .draw(|frame| render(frame, area))
            .expect("draw frame");
        terminal.backend().buffer().clone()
    }

    pub fn buffer_rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf.cell((x, y)).map_or(" ", |cell| cell.symbol()))
                    .collect::<String>()
            })
            .collect()
    }

    pub fn buffer_text(buf: &Buffer) -> String {
        buffer_rows(buf).join("\n")
    }

    /// Column (counted in characters, not bytes) where `needle` first appears.
    pub fn col_of(buf: &Buffer, needle: &str) -> Option<usize> {
        buffer_rows(buf)
            .iter()
            .find_map(|row| row.find(needle).map(|idx| row[..idx].chars().count()))
    }

    /// Flatten a built line back into the text a terminal would show.
    pub fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    pub fn leading_spaces(text: &str) -> usize {
        text.chars().take_while(|c| *c == ' ').count()
    }

    static TEMP_SEQ: AtomicUsize = AtomicUsize::new(0);

    pub struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        pub fn new(tag: &str) -> Self {
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "er-diff-view-{}-{}-{}",
                tag,
                std::process::id(),
                seq
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        pub fn root(&self) -> String {
            self.path.to_string_lossy().into_owned()
        }

        pub fn write(&self, rel: &str, contents: &str) {
            self.write_bytes(rel, contents.as_bytes());
        }

        pub fn write_bytes(&self, rel: &str, contents: &[u8]) {
            let full = self.path.join(rel);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create parent dir");
            }
            std::fs::write(full, contents).expect("write temp file");
        }

        pub fn size_of(&self, rel: &str) -> u64 {
            std::fs::metadata(self.path.join(rel))
                .expect("stat temp file")
                .len()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// `cell_wrap_height` decides how many terminal rows one split-view cell takes.
/// Both panes advance by `max(left, right)`, so a wrong height here silently
/// de-synchronises the two sides of the diff.
#[cfg(test)]
mod cell_wrap_height_tests {
    use super::*;
    use er_engine::git::DiffLine;

    fn diff_line(line_type: LineType, content: &str) -> DiffLine {
        DiffLine {
            line_type,
            content: content.to_string(),
            old_num: Some(1),
            new_num: Some(1),
        }
    }

    #[test]
    fn missing_cell_occupies_exactly_one_row() {
        // A one-sided row (add with no matching delete) still needs a phantom
        // row on the other pane, or the panes drift apart.
        assert_eq!(cell_wrap_height(None, true, 20, 4), 1);
    }

    #[test]
    fn fold_row_stays_one_row_even_when_its_content_would_wrap() {
        let line = diff_line(LineType::Fold(12), "aaaa bbbb cccc dddd eeee ffff");
        let cell = SplitCell {
            line_idx: 0,
            line: &line,
        };
        assert_eq!(cell_wrap_height(Some(&cell), true, 8, 4), 1);
    }

    #[test]
    fn wrapped_cell_height_matches_the_word_wrap_segment_count() {
        let content = "alpha beta gamma delta";
        let line = diff_line(LineType::Add, content);
        let cell = SplitCell {
            line_idx: 0,
            line: &line,
        };
        // "alpha beta" (10 cols) then "gamma delta" (11) — two segments at width 12.
        // Literal, not `word_wrap(content, 12).len()`: re-deriving the expected
        // value with the function's own helper would pass for any wrap width.
        assert_eq!(cell_wrap_height(Some(&cell), true, 12, 4), 2);
    }

    #[test]
    fn wrap_disabled_keeps_a_long_line_on_one_row() {
        let line = diff_line(LineType::Add, "alpha beta gamma delta");
        let cell = SplitCell {
            line_idx: 0,
            line: &line,
        };
        assert_eq!(cell_wrap_height(Some(&cell), false, 12, 4), 1);
    }

    #[test]
    fn empty_content_never_wraps() {
        let line = diff_line(LineType::Context, "");
        let cell = SplitCell {
            line_idx: 0,
            line: &line,
        };
        assert_eq!(cell_wrap_height(Some(&cell), true, 8, 4), 1);
    }

    #[test]
    fn tabs_are_expanded_before_the_wrap_is_measured() {
        let line = diff_line(LineType::Context, "\tword1 word2");
        let cell = SplitCell {
            line_idx: 0,
            line: &line,
        };
        // tab_width 1 → " word1 word2" (12 cols) fits in 14.
        assert_eq!(cell_wrap_height(Some(&cell), true, 14, 1), 1);
        // tab_width 8 → 8 spaces of indent pushes "word2" onto a second row.
        assert_eq!(cell_wrap_height(Some(&cell), true, 14, 8), 2);
    }

    #[test]
    fn zero_wrap_width_falls_back_to_one_column_instead_of_no_wrapping() {
        let line = diff_line(LineType::Add, "abcd");
        let cell = SplitCell {
            line_idx: 0,
            line: &line,
        };
        // Without the `.max(1)` guard this would hit word_wrap's "0 disables
        // wrapping" path and report a single row for a line that cannot fit.
        assert_eq!(word_wrap("abcd", 0).len(), 1);
        assert_eq!(cell_wrap_height(Some(&cell), true, 0, 4), 4);
    }
}

/// Reply rendering: the `↳` header (icon, author, time, sync marker, focus
/// diamond) plus the wrapped body, at two indent depths (inline vs panel).
#[cfg(test)]
mod render_reply_lines_tests {
    use super::test_support::{leading_spaces, line_text};
    use super::*;
    use er_engine::ai::{GitHubReviewComment, ReviewQuestion};
    use ratatui::style::Modifier;

    fn question(id: &str, author: &str, timestamp: &str, text: &str) -> ReviewQuestion {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "file": "src/lib.rs",
            "hunk_index": 0,
            "line_start": 12,
            "text": text,
            "author": author,
            "timestamp": timestamp,
        }))
        .expect("question fixture parses")
    }

    fn github_reply(id: &str, synced: bool) -> GitHubReviewComment {
        GitHubReviewComment {
            id: id.to_string(),
            timestamp: String::new(),
            file: "src/lib.rs".to_string(),
            hunk_index: Some(0),
            line_start: Some(12),
            line_end: None,
            line_content: "let x = 1;".to_string(),
            comment: "please rename".to_string(),
            in_reply_to: Some("gh-parent".to_string()),
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "octocat".to_string(),
            synced,
            outdated: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: "@@ -1,3 +1,3 @@".to_string(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".to_string(),
        }
    }

    #[test]
    fn question_reply_header_uses_the_question_icon_and_a_bold_author() {
        let q = question("q-1", "reviewer", "", "Looks off to me");
        let mut lines: Vec<Line> = Vec::new();
        render_reply_lines(&mut lines, &CommentRef::Question(&q), 60, false, false);

        let header = line_text(&lines[0]);
        assert!(
            header.starts_with("    \u{21b3} \u{2753} "),
            "got {header:?}"
        );
        assert_eq!(lines[0].spans[1].content.as_ref(), "reviewer");
        assert!(lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn note_reply_uses_the_comment_icon_not_the_question_icon() {
        // Notes reuse `ReviewQuestion`, but only the `Question` variant gets the
        // ❓ icon — a note reply renders like a comment.
        let n = question("n-1", "You", "", "hand this to an agent");
        let mut lines: Vec<Line> = Vec::new();
        render_reply_lines(&mut lines, &CommentRef::Note(&n), 60, false, false);

        let header = line_text(&lines[0]);
        assert!(header.contains('\u{1f4ac}'), "got {header:?}");
        assert!(!header.contains('\u{2753}'), "got {header:?}");
    }

    #[test]
    fn unsynced_github_reply_is_marked_local_and_a_synced_one_is_marked_synced() {
        let local = github_reply("gh-r1", false);
        let mut local_lines: Vec<Line> = Vec::new();
        render_reply_lines(
            &mut local_lines,
            &CommentRef::GitHubComment(&local),
            60,
            false,
            false,
        );
        let local_header = line_text(&local_lines[0]);
        assert!(
            local_header.contains("\u{2191} local"),
            "got {local_header:?}"
        );
        assert!(!local_header.contains("\u{2191} synced"));

        let pushed = github_reply("gh-r2", true);
        let mut pushed_lines: Vec<Line> = Vec::new();
        render_reply_lines(
            &mut pushed_lines,
            &CommentRef::GitHubComment(&pushed),
            60,
            false,
            false,
        );
        let pushed_header = line_text(&pushed_lines[0]);
        assert!(
            pushed_header.contains("\u{2191} synced"),
            "got {pushed_header:?}"
        );
    }

    #[test]
    fn question_reply_header_carries_no_sync_marker_at_all() {
        // Questions are private, so neither the "local" nor the "synced" badge
        // applies — the header is just the prefix and the author.
        let q = question("q-2", "You", "", "why?");
        let mut lines: Vec<Line> = Vec::new();
        render_reply_lines(&mut lines, &CommentRef::Question(&q), 60, false, false);

        assert_eq!(lines[0].spans.len(), 2);
        let header = line_text(&lines[0]);
        assert!(!header.contains('\u{2191}'), "got {header:?}");
    }

    #[test]
    fn reply_header_shows_only_the_time_part_of_the_timestamp() {
        let q = question("q-3", "You", "2024-05-01T12:34:56Z", "when?");
        let mut lines: Vec<Line> = Vec::new();
        render_reply_lines(&mut lines, &CommentRef::Question(&q), 60, false, false);

        let header = line_text(&lines[0]);
        assert!(header.contains("12:34:56"), "got {header:?}");
        assert!(!header.contains("2024-05-01"), "got {header:?}");
        assert!(!header.contains('Z'), "trailing Z is trimmed: {header:?}");
    }

    #[test]
    fn focused_reply_header_ends_with_the_focus_diamond() {
        let q = question("q-4", "You", "", "focus me");
        let mut lines: Vec<Line> = Vec::new();
        render_reply_lines(&mut lines, &CommentRef::Question(&q), 60, false, true);

        assert!(line_text(&lines[0]).ends_with("  \u{25c6}"));
    }

    #[test]
    fn inline_replies_indent_two_columns_deeper_than_panel_replies() {
        let reply = github_reply("gh-r3", false);

        let mut inline_lines: Vec<Line> = Vec::new();
        render_reply_lines(
            &mut inline_lines,
            &CommentRef::GitHubComment(&reply),
            60,
            true,
            false,
        );
        let mut panel_lines: Vec<Line> = Vec::new();
        render_reply_lines(
            &mut panel_lines,
            &CommentRef::GitHubComment(&reply),
            60,
            false,
            false,
        );

        assert_eq!(leading_spaces(&line_text(&inline_lines[0])), 7);
        assert_eq!(leading_spaces(&line_text(&panel_lines[0])), 4);
        assert_eq!(leading_spaces(&line_text(&inline_lines[1])), 12);
        assert_eq!(leading_spaces(&line_text(&panel_lines[1])), 10);
    }

    #[test]
    fn reply_body_wraps_to_the_width_left_after_the_indent() {
        let text = "alpha beta gamma delta epsilon zeta";
        let q = question("q-5", "You", "", text);
        let mut lines: Vec<Line> = Vec::new();
        // width 30 − indent 10 = 20 columns of text.
        render_reply_lines(&mut lines, &CommentRef::Question(&q), 30, false, false);

        // Literals, not `word_wrap(text, 20)`: re-deriving with the same helper
        // would pass whatever width the function actually wrapped at. At the
        // full width 30 the body would be ["alpha beta gamma delta epsilon",
        // "zeta"], so these strings pin the `width − indent` subtraction.
        assert_eq!(lines.len(), 3);
        assert_eq!(line_text(&lines[1]).trim(), "alpha beta gamma");
        assert_eq!(line_text(&lines[2]).trim(), "delta epsilon zeta");
    }
}

/// Finding banners: severity symbol, category, stale tag, focus marker, and the
/// first-line-only description/suggestion rows with their two truncation budgets.
#[cfg(test)]
mod render_finding_banner_tests {
    use super::test_support::line_text;
    use super::*;
    use ratatui::style::Modifier;

    fn finding(value: serde_json::Value) -> Finding {
        serde_json::from_value(value).expect("finding fixture parses")
    }

    fn plain_finding() -> Finding {
        finding(serde_json::json!({
            "id": "f-1",
            "severity": "high",
            "category": "correctness",
            "title": "Null deref",
        }))
    }

    #[test]
    fn banner_without_body_is_a_single_symbol_category_title_row() {
        let f = plain_finding();
        let mut lines: Vec<Line> = Vec::new();
        render_finding_banner(&mut lines, &f, 80, false, false);

        assert_eq!(lines.len(), 1, "no description or suggestion → title only");
        assert_eq!(line_text(&lines[0]), "  \u{25cf} [correctness] Null deref");
    }

    #[test]
    fn info_severity_renders_the_hollow_symbol() {
        let f = finding(serde_json::json!({
            "id": "f-2",
            "severity": "info",
            "category": "style",
            "title": "Naming nit",
        }));
        let mut lines: Vec<Line> = Vec::new();
        render_finding_banner(&mut lines, &f, 80, false, false);

        assert_eq!(line_text(&lines[0]), "  \u{25cb} [style] Naming nit");
    }

    #[test]
    fn stale_file_appends_a_stale_tag_to_the_title() {
        let f = plain_finding();
        let mut lines: Vec<Line> = Vec::new();
        render_finding_banner(&mut lines, &f, 80, true, false);

        assert!(line_text(&lines[0]).ends_with("Null deref [stale]"));
    }

    #[test]
    fn focused_banner_appends_a_bold_focus_marker() {
        let f = plain_finding();
        let mut lines: Vec<Line> = Vec::new();
        render_finding_banner(&mut lines, &f, 80, false, true);

        let marker = lines[0].spans.last().expect("focus span");
        assert_eq!(marker.content.as_ref(), "  \u{25c6} focused");
        assert!(marker.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn description_and_suggestion_render_only_their_first_line() {
        let f = finding(serde_json::json!({
            "id": "f-3",
            "severity": "medium",
            "category": "logic",
            "title": "Off by one",
            "description": "loop overruns\nsecond paragraph",
            "suggestion": "use ..= here\nsecond paragraph",
        }));
        let mut lines: Vec<Line> = Vec::new();
        render_finding_banner(&mut lines, &f, 80, false, false);

        assert_eq!(lines.len(), 3);
        assert_eq!(line_text(&lines[1]), "    loop overruns");
        assert_eq!(line_text(&lines[2]), "    \u{2192} use ..= here");
    }

    #[test]
    fn description_and_suggestion_truncate_to_their_own_width_budgets() {
        let f = finding(serde_json::json!({
            "id": "f-4",
            "severity": "low",
            "category": "perf",
            "title": "Slow path",
            "description": "d".repeat(100),
            "suggestion": "s".repeat(100),
        }));
        let mut lines: Vec<Line> = Vec::new();
        render_finding_banner(&mut lines, &f, 40, false, false);

        // description budget = width − 6, suggestion budget = width − 8.
        let desc = line_text(&lines[1]);
        let desc_body = desc.strip_prefix("    ").expect("description indent");
        assert_eq!(desc_body.chars().count(), 34);
        assert!(desc_body.ends_with('\u{2026}'));

        let sug = line_text(&lines[2]);
        let sug_body = sug
            .strip_prefix("    \u{2192} ")
            .expect("suggestion indent");
        assert_eq!(sug_body.chars().count(), 32);
        assert!(sug_body.ends_with('\u{2026}'));
    }
}

/// Watched-file content mode: numbered source rows plus the size/binary/error
/// short circuits.
#[cfg(test)]
mod render_watched_content_lines_tests {
    use super::test_support::{line_text, TempDir};
    use super::*;

    #[test]
    fn content_lines_are_numbered_from_one_with_a_gutter() {
        let dir = TempDir::new("content");
        dir.write("notes.md", "alpha\nbeta\n");
        let mut lines: Vec<Line> = Vec::new();
        render_watched_content_lines(&mut lines, &dir.root(), "notes.md", dir.size_of("notes.md"));

        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "    1 \u{2502}alpha");
        assert_eq!(line_text(&lines[1]), "    2 \u{2502}beta");
    }

    #[test]
    fn oversized_file_reports_its_size_without_reading_the_file() {
        let dir = TempDir::new("huge");
        // Nothing on disk: if the size guard did not short-circuit before the
        // read, this would render a read error instead.
        let mut lines: Vec<Line> = Vec::new();
        render_watched_content_lines(&mut lines, &dir.root(), "absent.bin", 11 * 1024 * 1024);

        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "  Binary or large file (11.0 MB)");
    }

    #[test]
    fn binary_file_reports_its_size_instead_of_content() {
        let dir = TempDir::new("binary");
        // A NUL byte in the first 8KB is what marks the file binary; the size
        // in the label comes from the watcher's recorded size, not the file.
        dir.write_bytes("blob.bin", &[b'a', 0, b'b']);
        let mut lines: Vec<Line> = Vec::new();
        render_watched_content_lines(&mut lines, &dir.root(), "blob.bin", 2048);

        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "  Binary file (2.0 KB)");
    }

    #[test]
    fn missing_file_reports_the_read_error_with_the_path() {
        let dir = TempDir::new("missing");
        let mut lines: Vec<Line> = Vec::new();
        render_watched_content_lines(&mut lines, &dir.root(), "gone.txt", 10);

        assert_eq!(lines.len(), 1);
        let text = line_text(&lines[0]);
        assert!(text.starts_with("  Error reading file:"), "got {text:?}");
        assert!(text.contains("gone.txt"), "got {text:?}");
    }

    #[test]
    fn file_over_ten_thousand_lines_is_truncated_with_a_tail_note() {
        let dir = TempDir::new("large");
        let body: String = (1..=10_002).map(|i| format!("line{i}\n")).collect();
        dir.write("big.txt", &body);
        let mut lines: Vec<Line> = Vec::new();
        render_watched_content_lines(&mut lines, &dir.root(), "big.txt", dir.size_of("big.txt"));

        assert_eq!(
            line_text(&lines[0]),
            "  Large file (10002 lines) \u{2014} content truncated"
        );
        // warning + blank + 10_000 rendered rows + blank + tail note
        assert_eq!(lines.len(), 2 + 10_000 + 2);
        assert_eq!(line_text(&lines[2]), "    1 \u{2502}line1");
        assert_eq!(line_text(&lines[lines.len() - 1]), "  ... 2 more lines");
    }
}

/// The watched-file panel: header, size row, content mode, and all three
/// snapshot-mode outcomes (first view, unchanged, changed) plus the error
/// fallback.
#[cfg(test)]
mod render_watched_tests {
    use super::test_support::{buffer_text, draw, TempDir};
    use super::*;
    use er_engine::ErRoot;

    fn watched_app(dir: &TempDir, diff_mode: &str) -> App {
        let mut app = App::new_for_test(vec![]);
        let tab = app.tab_mut();
        tab.repo_root = dir.root();
        tab.er_root = ErRoot::RepoLocal(dir.root());
        tab.watched_config.diff_mode = diff_mode.to_string();
        app
    }

    #[test]
    fn header_warns_when_the_watched_file_is_not_gitignored() {
        let dir = TempDir::new("wnotignored");
        dir.write("agent.log", "hello\n");
        let mut app = watched_app(&dir, "content");
        app.tab_mut()
            .watched_not_ignored
            .push("agent.log".to_string());

        let buf = draw(80, 12, |f, area| {
            render_watched(f, area, &app, "agent.log", 6);
        });
        let text = buffer_text(&buf);

        assert!(
            text.contains("watched \u{b7} \u{26a0} not in .gitignore"),
            "{text}"
        );
        assert!(!text.contains("not tracked by git"), "{text}");
    }

    #[test]
    fn header_says_untracked_when_the_file_is_gitignored() {
        let dir = TempDir::new("wignored");
        dir.write("agent.log", "hello\n");
        let app = watched_app(&dir, "content");

        let buf = draw(80, 12, |f, area| {
            render_watched(f, area, &app, "agent.log", 6);
        });
        let text = buffer_text(&buf);

        assert!(text.contains("watched \u{b7} not tracked by git"), "{text}");
        assert!(!text.contains("not in .gitignore"), "{text}");
    }

    #[test]
    fn content_mode_shows_the_size_row_and_the_file_body() {
        let dir = TempDir::new("wcontent");
        dir.write("agent.log", "hello\n");
        let app = watched_app(&dir, "content");

        let buf = draw(80, 12, |f, area| {
            render_watched(f, area, &app, "agent.log", 6);
        });
        let text = buffer_text(&buf);

        assert!(text.contains("\u{25c9} agent.log"), "{text}");
        assert!(text.contains("Size: 6 B"), "{text}");
        assert!(text.contains("hello"), "{text}");
    }

    #[test]
    fn scrolling_drops_the_header_rows_from_the_viewport() {
        let dir = TempDir::new("wscroll");
        dir.write("agent.log", "hello\n");
        let mut app = watched_app(&dir, "content");
        // Header, size row, and blank spacer occupy the first three lines.
        app.tab_mut().diff_scroll = 3;

        let buf = draw(80, 12, |f, area| {
            render_watched(f, area, &app, "agent.log", 6);
        });
        let text = buffer_text(&buf);

        assert!(
            !text.contains('\u{25c9}'),
            "header must scroll away: {text}"
        );
        assert!(!text.contains("Size: 6 B"), "{text}");
        assert!(text.contains("hello"), "content stays visible: {text}");
    }

    #[test]
    fn snapshot_mode_first_view_saves_a_snapshot_and_still_shows_content() {
        let dir = TempDir::new("wsnapfirst");
        dir.write("agent.log", "one\ntwo\n");
        let app = watched_app(&dir, "snapshot");

        let buf = draw(80, 14, |f, area| {
            render_watched(f, area, &app, "agent.log", 8);
        });
        let text = buffer_text(&buf);

        assert!(text.contains("Snapshot saved (first view)"), "{text}");
        assert!(
            dir.path().join(".er/snapshots/agent.log").exists(),
            "the first view must persist a snapshot"
        );
        assert!(text.contains("one"), "falls through to content: {text}");
    }

    #[test]
    fn snapshot_mode_reports_no_changes_when_the_file_still_matches() {
        let dir = TempDir::new("wsnapsame");
        dir.write("agent.log", "one\ntwo\n");
        let app = watched_app(&dir, "snapshot");

        // First render seeds the snapshot; the second diffs against it.
        draw(80, 14, |f, area| {
            render_watched(f, area, &app, "agent.log", 8);
        });
        let buf = draw(80, 14, |f, area| {
            render_watched(f, area, &app, "agent.log", 8);
        });
        let text = buffer_text(&buf);

        assert!(text.contains("No changes since snapshot"), "{text}");
        assert!(text.contains("Press s to update snapshot"), "{text}");
    }

    #[test]
    fn snapshot_mode_renders_the_diff_after_the_file_changes() {
        let dir = TempDir::new("wsnapdiff");
        dir.write("agent.log", "one\ntwo\n");
        let app = watched_app(&dir, "snapshot");

        draw(80, 20, |f, area| {
            render_watched(f, area, &app, "agent.log", 8);
        });
        dir.write("agent.log", "one\nTWO\n");
        let buf = draw(80, 20, |f, area| {
            render_watched(f, area, &app, "agent.log", 8);
        });
        let text = buffer_text(&buf);

        assert!(text.contains("diff vs snapshot"), "{text}");
        assert!(text.contains("@@"), "hunk header is rendered: {text}");
        assert!(text.contains("-two"), "deleted line is rendered: {text}");
        assert!(text.contains("+TWO"), "added line is rendered: {text}");
        assert!(text.contains(" one"), "context line is rendered: {text}");
    }

    #[test]
    fn snapshot_failure_falls_back_to_content_mode() {
        let dir = TempDir::new("wsnaperr");
        let app = watched_app(&dir, "snapshot");

        // `..` escapes the repo root, so the snapshot path never resolves and
        // the snapshot branch errors out into the content-mode fallback.
        let buf = draw(80, 10, |f, area| {
            render_watched(f, area, &app, "../escape.txt", 12);
        });
        let text = buffer_text(&buf);

        assert!(!text.contains("Snapshot saved"), "{text}");
        assert!(text.contains("Error reading file"), "{text}");
    }
}

/// Split view: the 50/50 pane routing and every condition that sends it back to
/// the unified renderer.
#[cfg(test)]
mod render_split_tests {
    use super::test_support::{buffer_text, col_of, draw, TempDir};
    use super::*;
    use er_engine::git::{DiffFile, DiffHunk, DiffLine, FileStatus, WatchedFile};
    use er_engine::ErRoot;
    use std::time::SystemTime;

    fn split_file() -> DiffFile {
        DiffFile {
            path: "src/lib.txt".to_string(),
            status: FileStatus::Modified,
            hunks: vec![DiffHunk {
                header: "@@ -1,2 +1,2 @@".to_string(),
                old_start: 1,
                old_count: 2,
                new_start: 1,
                new_count: 2,
                lines: vec![
                    DiffLine {
                        line_type: LineType::Context,
                        content: "alpha".to_string(),
                        old_num: Some(1),
                        new_num: Some(1),
                    },
                    DiffLine {
                        line_type: LineType::Delete,
                        content: "OLDLINE".to_string(),
                        old_num: Some(2),
                        new_num: None,
                    },
                    DiffLine {
                        line_type: LineType::Add,
                        content: "NEWLINE".to_string(),
                        old_num: None,
                        new_num: Some(2),
                    },
                ],
            }],
            adds: 1,
            dels: 1,
            compacted: false,
            raw_hunk_count: 1,
        }
    }

    #[test]
    fn split_view_puts_the_deleted_line_left_and_the_added_line_right() {
        let app = App::new_for_test(vec![split_file()]);
        let config = ErConfig::default();
        let mut hl = Highlighter::new();

        let buf = draw(100, 20, |f, area| {
            render_split(f, area, &app, &mut hl, &config);
        });
        let text = buffer_text(&buf);

        assert!(text.contains(" Old "), "{text}");
        assert!(text.contains(" New "), "{text}");
        let old_col = col_of(&buf, "OLDLINE").expect("deleted line rendered");
        let new_col = col_of(&buf, "NEWLINE").expect("added line rendered");
        assert!(
            old_col < 50,
            "deleted line belongs left, got column {old_col}"
        );
        assert!(
            new_col >= 50,
            "added line belongs right, got column {new_col}"
        );
    }

    #[test]
    fn area_narrower_than_sixty_columns_falls_back_to_the_unified_view() {
        let app = App::new_for_test(vec![split_file()]);
        let config = ErConfig::default();
        let mut hl = Highlighter::new();

        let buf = draw(50, 20, |f, area| {
            render_split(f, area, &app, &mut hl, &config);
        });
        let text = buffer_text(&buf);

        assert!(!text.contains(" Old "), "no split panes at 50 cols: {text}");
        assert!(text.contains("OLDLINE"), "{text}");
        assert!(text.contains("NEWLINE"), "{text}");
    }

    #[test]
    fn compacted_file_falls_back_to_the_unified_summary() {
        let mut file = split_file();
        file.compacted = true;
        file.hunks.clear();
        let app = App::new_for_test(vec![file]);
        let config = ErConfig::default();
        let mut hl = Highlighter::new();

        let buf = draw(100, 20, |f, area| {
            render_split(f, area, &app, &mut hl, &config);
        });
        let text = buffer_text(&buf);

        assert!(
            text.contains("(compacted \u{2014} press Enter to expand)"),
            "{text}"
        );
        assert!(!text.contains(" Old "), "{text}");
    }

    #[test]
    fn no_selected_file_falls_back_to_the_empty_state() {
        let app = App::new_for_test(vec![]);
        let config = ErConfig::default();
        let mut hl = Highlighter::new();

        let buf = draw(100, 20, |f, area| {
            render_split(f, area, &app, &mut hl, &config);
        });
        let text = buffer_text(&buf);

        assert!(text.contains("No files changed"), "{text}");
        assert!(!text.contains(" Old "), "{text}");
    }

    #[test]
    fn selected_watched_file_falls_back_to_the_watched_view() {
        let dir = TempDir::new("splitwatched");
        dir.write("agent.log", "watched body\n");
        let mut app = App::new_for_test(vec![split_file()]);
        {
            let tab = app.tab_mut();
            tab.repo_root = dir.root();
            tab.er_root = ErRoot::RepoLocal(dir.root());
            tab.watched_config.diff_mode = "content".to_string();
            tab.watched_files = vec![WatchedFile {
                path: "agent.log".to_string(),
                modified: SystemTime::now(),
                size: 13,
            }];
            tab.selected_watched = Some(0);
        }
        let config = ErConfig::default();
        let mut hl = Highlighter::new();

        let buf = draw(100, 20, |f, area| {
            render_split(f, area, &app, &mut hl, &config);
        });
        let text = buffer_text(&buf);

        assert!(text.contains("\u{25c9} agent.log"), "{text}");
        assert!(text.contains("watched body"), "{text}");
        assert!(
            !text.contains(" Old "),
            "watched view is never split: {text}"
        );
    }
}
