use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};

use crate::ai::{CommentRef, CommentType, PanelContent, ReviewFocus, RiskLevel};
use crate::app::App;
use super::styles;
use super::utils::word_wrap;

/// Render the context panel (right side, when tab.panel is Some)
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let panel_content = match &tab.panel {
        Some(c) => *c,
        None => return,
    };

    render_panel(f, area, app, panel_content);
}

fn render_panel(f: &mut Frame, area: Rect, app: &App, content: PanelContent) {
    let tab = app.tab();
    let mut lines: Vec<Line> = Vec::new();

    // Title bar: [File] [AI] [PR] with active one highlighted
    let file_style = if content == PanelContent::FileDetail {
        Style::default().fg(styles::PURPLE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::DIM)
    };
    let ai_style = if content == PanelContent::AiSummary {
        Style::default().fg(styles::PURPLE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::DIM)
    };
    let pr_style = if content == PanelContent::PrOverview {
        Style::default().fg(styles::PURPLE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::DIM)
    };

    lines.push(Line::from(vec![
        Span::styled(" [", Style::default().fg(styles::MUTED)),
        Span::styled("File", file_style),
        Span::styled("] [", Style::default().fg(styles::MUTED)),
        Span::styled("AI", ai_style),
        Span::styled("] [", Style::default().fg(styles::MUTED)),
        Span::styled("PR", pr_style),
        Span::styled("]", Style::default().fg(styles::MUTED)),
    ]));
    lines.push(Line::from(vec![Span::styled(
        "─".repeat(area.width.saturating_sub(2) as usize),
        Style::default().fg(styles::BORDER),
    )]));

    // Content area
    match content {
        PanelContent::FileDetail => render_file_detail(&mut lines, area, tab),
        PanelContent::AiSummary => render_ai_summary(&mut lines, area, tab),
        PanelContent::PrOverview => render_pr_overview(&mut lines),
    }

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(styles::BORDER))
        .style(Style::default().bg(styles::SURFACE))
        .padding(Padding::new(0, 1, 0, 0));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((tab.panel_scroll, 0));

    f.render_widget(paragraph, area);
}

// ── FileDetail ──

fn render_file_detail<'a>(lines: &mut Vec<Line<'a>>, area: Rect, tab: &'a crate::app::TabState) {
    let ai_stale = tab.ai.is_stale;

    let file = tab.selected_diff_file();
    let file_path = file.map(|f| f.path.as_str());
    let file_stale = file_path.map_or(ai_stale, |p| tab.ai.is_file_stale(p));

    let Some(path) = file_path else {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            " No file selected",
            Style::default().fg(styles::MUTED),
        )]));
        return;
    };

    // File path header
    lines.push(Line::from(vec![Span::styled(
        format!(" {}", path),
        Style::default().fg(styles::TEXT).add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));

    // AI file review
    if tab.layers.show_ai_findings {
        if let Some(fr) = tab.ai.file_review(path) {
            let risk_style = if file_stale {
                styles::stale_style()
            } else {
                match fr.risk {
                    RiskLevel::High => styles::risk_high(),
                    RiskLevel::Medium => styles::risk_medium(),
                    RiskLevel::Low => styles::risk_low(),
                    RiskLevel::Info => Style::default().fg(styles::BLUE),
                }
            };
            let risk_label = match fr.risk {
                RiskLevel::High => "[HIGH]",
                RiskLevel::Medium => "[MED]",
                RiskLevel::Low => "[LOW]",
                RiskLevel::Info => "[INFO]",
            };

            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", fr.risk.symbol()), risk_style),
                Span::styled(risk_label, risk_style),
            ]));

            if !fr.risk_reason.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    format!(" {}", fr.risk_reason),
                    Style::default().fg(styles::DIM),
                )]));
            }

            if !fr.summary.is_empty() {
                lines.push(Line::from(""));
                let max_w = area.width.saturating_sub(3) as usize;
                for wrapped in word_wrap(&fr.summary, max_w) {
                    lines.push(Line::from(vec![Span::styled(
                        format!(" {}", wrapped),
                        Style::default().fg(styles::TEXT),
                    )]));
                }
            }

            lines.push(Line::from(""));
        }
    }

    // Comments section — collect all top-level comments for this file across all hunks
    let selected_file = tab.files.get(tab.selected_file);
    let hunk_count = selected_file.map(|f| f.hunks.len()).unwrap_or(0);

    let mut file_comments: Vec<CommentRef> = Vec::new();
    for hi in 0..hunk_count {
        for cr in tab.ai.comments_for_hunk(path, hi) {
            if cr.in_reply_to().is_none() {
                file_comments.push(cr);
            }
        }
    }

    if file_comments.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " No comments for this file",
            Style::default().fg(styles::MUTED),
        )]));
        return;
    }

    let q_count = file_comments
        .iter()
        .filter(|c| c.comment_type() == CommentType::Question)
        .count();
    let gh_count = file_comments
        .iter()
        .filter(|c| c.comment_type() == CommentType::GitHubComment)
        .count();

    let header = if q_count > 0 && gh_count > 0 {
        format!(" Questions ({}) + Comments ({})", q_count, gh_count)
    } else if q_count > 0 {
        format!(" Questions ({})", q_count)
    } else {
        format!(" Comments ({})", gh_count)
    };

    lines.push(Line::from(vec![Span::styled(
        header,
        Style::default()
            .fg(styles::CYAN)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));

    let max_w = area.width.saturating_sub(5) as usize;

    for comment in &file_comments {
        let is_question = comment.comment_type() == CommentType::Question;
        let accent = if comment.is_stale() {
            styles::STALE
        } else if is_question {
            styles::YELLOW
        } else {
            styles::CYAN
        };
        let bullet = "◆";

        let target = comment
            .hunk_index()
            .map(|hi| {
                comment
                    .line_start()
                    .map(|l| format!("h{}:L{}", hi + 1, l))
                    .unwrap_or_else(|| format!("h{}", hi + 1))
            })
            .unwrap_or_else(|| "file".to_string());

        let author = comment.author();

        let mut header_spans = vec![
            Span::styled(
                format!(" {} ", bullet),
                Style::default().fg(accent),
            ),
            Span::styled(
                author.to_string(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", target),
                Style::default().fg(styles::DIM),
            ),
        ];

        if comment.is_stale() {
            header_spans.push(Span::styled(
                "  ⚠ stale",
                Style::default().fg(styles::STALE),
            ));
        }

        if comment.is_synced() {
            header_spans.push(Span::styled(
                "  ↑ synced",
                Style::default().fg(styles::GREEN),
            ));
        }

        // Reply indicator
        let replies = tab.ai.replies_to(comment.id());
        if !replies.is_empty() {
            header_spans.push(Span::styled(
                format!("  ↳ {}", replies.len()),
                Style::default().fg(styles::DIM),
            ));
        }

        lines.push(Line::from(header_spans));

        let text_fg = if comment.is_stale() {
            styles::DIM
        } else {
            styles::TEXT
        };
        for wrapped in word_wrap(comment.text(), max_w) {
            lines.push(Line::from(vec![Span::styled(
                format!("   {}", wrapped),
                Style::default().fg(text_fg),
            )]));
        }

        // Render replies
        for reply in &replies {
            let reply_author = reply.author();
            lines.push(Line::from(vec![
                Span::styled("   ↳ ", Style::default().fg(styles::DIM)),
                Span::styled(
                    reply_author.to_string(),
                    Style::default()
                        .fg(styles::CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            for wrapped in word_wrap(reply.text(), max_w.saturating_sub(4)) {
                lines.push(Line::from(vec![Span::styled(
                    format!("       {}", wrapped),
                    Style::default().fg(styles::TEXT),
                )]));
            }
        }

        lines.push(Line::from(""));
    }
}

// ── AiSummary ──

fn render_ai_summary<'a>(lines: &mut Vec<Line<'a>>, area: Rect, tab: &'a crate::app::TabState) {
    let ai_stale = tab.ai.is_stale;
    let stale_tag = if ai_stale { " [stale]" } else { "" };

    // Overall summary
    lines.push(Line::from(vec![Span::styled(
        format!(" AI Review Summary{}", stale_tag),
        Style::default()
            .fg(styles::PURPLE)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));

    if let Some(ref summary) = tab.ai.summary {
        let max_w = area.width.saturating_sub(4) as usize;
        for line in summary.lines() {
            if line.is_empty() {
                lines.push(Line::from(""));
            } else if line.starts_with('#') {
                let text = line.trim_start_matches('#').trim();
                lines.push(Line::from(vec![Span::styled(
                    format!(" {}", text),
                    Style::default()
                        .fg(styles::BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )]));
            } else {
                for wrapped in word_wrap(line, max_w) {
                    lines.push(Line::from(vec![Span::styled(
                        format!(" {}", wrapped),
                        Style::default().fg(styles::TEXT),
                    )]));
                }
            }
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            " No .er-summary.md found",
            Style::default().fg(styles::MUTED),
        )]));
    }

    lines.push(Line::from(""));

    // File risk overview
    let is_files_focused = tab.review_focus == ReviewFocus::Files;
    let files_header_style = if is_files_focused {
        Style::default()
            .fg(styles::BRIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::BORDER)
    };
    lines.push(Line::from(vec![Span::styled(
        " ─── File Risk Overview ───",
        files_header_style,
    )]));
    lines.push(Line::from(""));

    if let Some(ref review) = tab.ai.review {
        let mut file_entries: Vec<_> = review.files.iter().collect();
        file_entries.sort_by(|a, b| {
            let risk_ord = |r: &RiskLevel| match r {
                RiskLevel::High => 0,
                RiskLevel::Medium => 1,
                RiskLevel::Low => 2,
                RiskLevel::Info => 3,
            };
            risk_ord(&a.1.risk)
                .cmp(&risk_ord(&b.1.risk))
                .then_with(|| a.0.cmp(b.0))
        });

        let cursor = tab.review_cursor;
        for (idx, (path, fr)) in file_entries.iter().enumerate() {
            let is_selected = is_files_focused && idx == cursor;
            let per_file_stale = tab.ai.is_file_stale(path);

            let risk_style = if per_file_stale {
                styles::stale_style()
            } else {
                match fr.risk {
                    RiskLevel::High => styles::risk_high(),
                    RiskLevel::Medium => styles::risk_medium(),
                    RiskLevel::Low => styles::risk_low(),
                    RiskLevel::Info => Style::default().fg(styles::BLUE),
                }
            };

            let bg = if is_selected {
                styles::LINE_CURSOR_BG
            } else {
                styles::BG
            };
            let path_style = if is_selected {
                Style::default()
                    .fg(styles::BRIGHT)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(styles::BRIGHT)
            };

            let prefix = if is_selected { "▸" } else { " " };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}{} ", prefix, fr.risk.symbol()),
                    if is_selected { risk_style.bg(bg) } else { risk_style },
                ),
                Span::styled(*path, path_style),
            ]));
        }

        let total = tab.ai.total_findings();
        if total > 0 {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                format!(" {} total findings across all files", total),
                Style::default().fg(styles::MUTED),
            )]));
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            " No .er-review.json found",
            Style::default().fg(styles::MUTED),
        )]));
    }

    lines.push(Line::from(""));

    // Checklist
    let is_checklist_focused = tab.review_focus == ReviewFocus::Checklist;
    let checklist_header_style = if is_checklist_focused {
        Style::default()
            .fg(styles::CYAN)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(styles::CYAN).add_modifier(Modifier::BOLD)
    };
    lines.push(Line::from(vec![Span::styled(
        " ─── Review Checklist ───",
        checklist_header_style,
    )]));
    lines.push(Line::from(""));

    if let Some(ref checklist) = tab.ai.checklist {
        let cursor = tab.review_cursor;
        for (idx, item) in checklist.items.iter().enumerate() {
            let is_selected = is_checklist_focused && idx == cursor;
            let bg = if is_selected {
                styles::LINE_CURSOR_BG
            } else {
                styles::SURFACE
            };

            let check = if item.checked { "✓" } else { "○" };
            let check_style = if item.checked {
                Style::default().fg(styles::GREEN).bg(bg)
            } else {
                Style::default().fg(styles::DIM).bg(bg)
            };

            let prefix = if is_selected { "▸" } else { " " };
            let text_style = if is_selected {
                Style::default()
                    .fg(styles::BRIGHT)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD)
            } else if item.checked {
                Style::default().fg(styles::MUTED)
            } else {
                Style::default().fg(styles::TEXT)
            };

            lines.push(Line::from(vec![
                Span::styled(format!("{}{} ", prefix, check), check_style),
                Span::styled(&item.text, text_style),
            ]));

            let mut meta_parts: Vec<String> = Vec::new();
            if !item.category.is_empty() {
                meta_parts.push(item.category.clone());
            }
            if !item.related_files.is_empty() {
                let files: Vec<&str> = item
                    .related_files
                    .iter()
                    .map(|f| f.rsplit('/').next().unwrap_or(f.as_str()))
                    .collect();
                meta_parts.push(files.join(", "));
            }
            if !meta_parts.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    format!("   {}", meta_parts.join(" · ")),
                    Style::default().fg(styles::MUTED).bg(bg),
                )]));
            }

            lines.push(Line::from(""));
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            " No .er-checklist.json found",
            Style::default().fg(styles::MUTED),
        )]));
    }
}

// ── PrOverview ──

fn render_pr_overview(lines: &mut Vec<Line<'_>>) {
    lines.push(Line::from(vec![Span::styled(
        " PR Overview",
        Style::default()
            .fg(styles::PURPLE)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        " No PR data loaded",
        Style::default().fg(styles::DIM),
    )]));
}
