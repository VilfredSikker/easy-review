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

// PR check conclusion display helpers
fn check_icon(conclusion: Option<&str>) -> (&'static str, ratatui::style::Color) {
    match conclusion {
        Some("success") => ("✓", styles::GREEN),
        Some("failure") | Some("cancelled") | Some("timed_out") => ("✗", styles::RED_TEXT),
        Some("skipped") => ("–", styles::MUTED),
        _ => ("○", styles::DIM),
    }
}

fn review_state_style(state: &str) -> (&'static str, ratatui::style::Color) {
    match state {
        "APPROVED" => ("✓ approved", styles::GREEN),
        "CHANGES_REQUESTED" => ("✗ changes requested", styles::RED_TEXT),
        "COMMENTED" => ("◆ commented", styles::CYAN),
        "DISMISSED" => ("– dismissed", styles::MUTED),
        _ => ("○ pending", styles::DIM),
    }
}

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

    // Title bar: [File] [AI] [PR] — AI and PR only shown when available
    let has_ai = tab.ai.has_data();
    let has_pr = tab.pr_data.is_some();

    let file_style = if content == PanelContent::FileDetail {
        Style::default().fg(styles::PURPLE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::DIM)
    };

    let mut tab_spans = vec![
        Span::styled(" [", Style::default().fg(styles::MUTED)),
        Span::styled("File", file_style),
        Span::styled("]", Style::default().fg(styles::MUTED)),
    ];

    if has_ai {
        let ai_style = if content == PanelContent::AiSummary {
            Style::default().fg(styles::PURPLE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(styles::DIM)
        };
        tab_spans.push(Span::styled(" [", Style::default().fg(styles::MUTED)));
        tab_spans.push(Span::styled("AI", ai_style));
        tab_spans.push(Span::styled("]", Style::default().fg(styles::MUTED)));
    }

    if has_pr {
        let pr_style = if content == PanelContent::PrOverview {
            Style::default().fg(styles::PURPLE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(styles::DIM)
        };
        tab_spans.push(Span::styled(" [", Style::default().fg(styles::MUTED)));
        tab_spans.push(Span::styled("PR", pr_style));
        tab_spans.push(Span::styled("]", Style::default().fg(styles::MUTED)));
    }

    if tab.symbol_refs.is_some() {
        let refs_style = if content == PanelContent::SymbolRefs {
            Style::default().fg(styles::PURPLE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(styles::DIM)
        };
        tab_spans.push(Span::styled(" [", Style::default().fg(styles::MUTED)));
        tab_spans.push(Span::styled("Refs", refs_style));
        tab_spans.push(Span::styled("]", Style::default().fg(styles::MUTED)));
    }

    lines.push(Line::from(tab_spans));
    lines.push(Line::from(vec![Span::styled(
        "─".repeat(area.width.saturating_sub(2) as usize),
        Style::default().fg(styles::BORDER),
    )]));

    // Content area
    match content {
        PanelContent::FileDetail => render_file_detail(&mut lines, area, tab),
        PanelContent::AiSummary => render_ai_summary(&mut lines, area, tab),
        PanelContent::PrOverview => render_pr_overview(&mut lines, area, tab),
        PanelContent::SymbolRefs => render_symbol_refs(&mut lines, area, tab),
    }

    let border_style = if tab.panel_focus {
        Style::default().fg(styles::PURPLE)
    } else {
        Style::default().fg(styles::BORDER)
    };

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(border_style)
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

fn render_pr_overview<'a>(lines: &mut Vec<Line<'a>>, area: Rect, tab: &'a crate::app::TabState) {
    lines.push(Line::from(vec![Span::styled(
        " PR Overview",
        Style::default()
            .fg(styles::PURPLE)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(""));

    let Some(pr) = tab.pr_data.as_ref() else {
        lines.push(Line::from(vec![Span::styled(
            " No PR data loaded",
            Style::default().fg(styles::DIM),
        )]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            " Open a branch with an active PR",
            Style::default().fg(styles::MUTED),
        )]));
        return;
    };

    // PR number + state
    let state_color = match pr.state.as_str() {
        "OPEN" => styles::GREEN,
        "CLOSED" => styles::RED_TEXT,
        "MERGED" => styles::PURPLE,
        _ => styles::MUTED,
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!(" #{} ", pr.number),
            Style::default().fg(styles::CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            pr.state.to_lowercase(),
            Style::default().fg(state_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  @{}", pr.author),
            Style::default().fg(styles::DIM),
        ),
    ]));
    lines.push(Line::from(""));

    // Title (word-wrapped)
    let max_w = area.width.saturating_sub(3) as usize;
    for wrapped in word_wrap(&pr.title, max_w) {
        lines.push(Line::from(vec![Span::styled(
            format!(" {}", wrapped),
            Style::default().fg(styles::BRIGHT).add_modifier(Modifier::BOLD),
        )]));
    }
    lines.push(Line::from(""));

    // Branch info
    lines.push(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(&pr.head_branch, Style::default().fg(styles::CYAN)),
        Span::styled(" → ", Style::default().fg(styles::DIM)),
        Span::styled(&pr.base_branch, Style::default().fg(styles::TEXT)),
    ]));
    lines.push(Line::from(""));

    // Body (first few lines, truncated)
    if !pr.body.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " ─── Description ───",
            Style::default().fg(styles::BORDER),
        )]));
        lines.push(Line::from(""));
        let mut shown = 0;
        for line in pr.body.lines() {
            if shown >= 8 {
                lines.push(Line::from(vec![Span::styled(
                    " [scroll for more]",
                    Style::default().fg(styles::MUTED),
                )]));
                break;
            }
            if line.is_empty() {
                lines.push(Line::from(""));
            } else {
                for wrapped in word_wrap(line, max_w) {
                    lines.push(Line::from(vec![Span::styled(
                        format!(" {}", wrapped),
                        Style::default().fg(styles::TEXT),
                    )]));
                    shown += 1;
                }
            }
        }
        lines.push(Line::from(""));
    }

    // CI checks
    if !pr.checks.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " ─── CI Checks ───",
            Style::default().fg(styles::BORDER),
        )]));
        lines.push(Line::from(""));
        for check in &pr.checks {
            let (icon, color) = check_icon(check.conclusion.as_deref());
            let status_text = if check.status == "completed" {
                check.conclusion.as_deref().unwrap_or("unknown").to_string()
            } else {
                check.status.clone()
            };
            let check_name = if check.name.chars().count() > max_w.saturating_sub(12) {
                format!("{}…", check.name.chars().take(max_w.saturating_sub(13)).collect::<String>())
            } else {
                check.name.clone()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", icon),
                    Style::default().fg(color),
                ),
                Span::styled(
                    check_name,
                    Style::default().fg(styles::TEXT),
                ),
                Span::styled(
                    format!("  {}", status_text),
                    Style::default().fg(styles::MUTED),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Reviewers
    if !pr.reviewers.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " ─── Reviewers ───",
            Style::default().fg(styles::BORDER),
        )]));
        lines.push(Line::from(""));
        // Deduplicate: keep latest review state per reviewer
        let mut seen: std::collections::HashMap<&str, &crate::github::ReviewerStatus> = std::collections::HashMap::new();
        for r in &pr.reviewers {
            seen.insert(&r.login, r);
        }
        let mut sorted_reviewers: Vec<&crate::github::ReviewerStatus> = seen.values().copied().collect();
        sorted_reviewers.sort_by(|a, b| a.login.cmp(&b.login));
        for reviewer in sorted_reviewers {
            let (label, color) = review_state_style(&reviewer.state);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" @{}  ", reviewer.login),
                    Style::default().fg(styles::TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(label, Style::default().fg(color)),
            ]));
        }
        lines.push(Line::from(""));
    }
}

// ── SymbolRefs ──

fn render_symbol_refs<'a>(lines: &mut Vec<Line<'a>>, area: Rect, tab: &'a crate::app::TabState) {
    let state = match tab.symbol_refs.as_ref() {
        Some(s) => s,
        None => {
            lines.push(Line::from(vec![Span::styled(
                " No symbol references",
                Style::default().fg(styles::MUTED),
            )]));
            return;
        }
    };

    // Header: symbol name
    lines.push(Line::from(vec![
        Span::styled(" Symbol: ", Style::default().fg(styles::DIM)),
        Span::styled(
            &*state.symbol,
            Style::default().fg(styles::BLUE).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    let max_w = area.width.saturating_sub(4) as usize;
    let mut entry_idx: usize = 0;

    // In-diff section
    if !state.in_diff.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(" In this diff ({})", state.in_diff.len()),
            Style::default().fg(styles::CYAN).add_modifier(Modifier::BOLD),
        )]));

        for entry in &state.in_diff {
            let is_selected = entry_idx == state.cursor && tab.panel_focus;
            let loc = format!(" {}:{}", entry.file, entry.line_num);
            let content = entry.line_content.trim();
            let truncated = if content.chars().count() > max_w {
                format!("{}…", content.chars().take(max_w.saturating_sub(1)).collect::<String>())
            } else {
                content.to_string()
            };

            let style = if is_selected {
                Style::default().fg(styles::TEXT).bg(styles::PANEL)
            } else {
                Style::default().fg(styles::TEXT)
            };
            let loc_style = if is_selected {
                Style::default().fg(styles::BLUE).bg(styles::PANEL).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(styles::BLUE)
            };

            lines.push(Line::from(vec![Span::styled(loc, loc_style)]));
            lines.push(Line::from(vec![Span::styled(
                format!("   {}", truncated),
                style,
            )]));

            entry_idx += 1;
        }
        lines.push(Line::from(""));
    }

    // External section
    if !state.external.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(" Other files ({})", state.external.len()),
            Style::default().fg(styles::DIM).add_modifier(Modifier::BOLD),
        )]));

        for entry in &state.external {
            let is_selected = entry_idx == state.cursor && tab.panel_focus;
            let loc = format!(" {}:{}", entry.file, entry.line_num);
            let content = entry.line_content.trim();
            let truncated = if content.chars().count() > max_w {
                format!("{}…", content.chars().take(max_w.saturating_sub(1)).collect::<String>())
            } else {
                content.to_string()
            };

            let style = if is_selected {
                Style::default().fg(styles::DIM).bg(styles::PANEL)
            } else {
                Style::default().fg(styles::DIM)
            };
            let loc_style = if is_selected {
                Style::default().fg(styles::MUTED).bg(styles::PANEL).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(styles::MUTED)
            };

            lines.push(Line::from(vec![Span::styled(loc, loc_style)]));
            lines.push(Line::from(vec![Span::styled(
                format!("   {}", truncated),
                style,
            )]));

            entry_idx += 1;
        }
    }

    if state.in_diff.is_empty() && state.external.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " No references found",
            Style::default().fg(styles::MUTED),
        )]));
    }
}
