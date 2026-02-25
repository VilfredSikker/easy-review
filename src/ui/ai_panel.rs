use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Padding},
    Frame,
};

use crate::ai::RiskLevel;
use crate::app::App;
use super::styles;
use super::utils::word_wrap;

/// Render the AI side panel (right side, in SidePanel view mode)
/// Shows findings, risk, and summary for the currently selected file
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let ai_stale = tab.ai.is_stale;

    let mut lines: Vec<Line> = Vec::new();

    // Get current file's AI data
    let file = tab.selected_diff_file();
    let file_path = file.map(|f| f.path.as_str());
    let file_stale = file_path.map_or(ai_stale, |p| tab.ai.is_file_stale(p));

    let fr = file_path.and_then(|p| tab.ai.file_review(p));

    // ── File risk header ──
    if let (Some(path), Some(fr)) = (file_path, fr) {
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
            RiskLevel::High => "HIGH RISK",
            RiskLevel::Medium => "MEDIUM RISK",
            RiskLevel::Low => "LOW RISK",
            RiskLevel::Info => "INFO",
        };

        // File name + risk
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", fr.risk.symbol()),
                risk_style,
            ),
            Span::styled(
                risk_label,
                risk_style,
            ),
        ]));

        if !fr.risk_reason.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {}", fr.risk_reason),
                    ratatui::style::Style::default().fg(styles::DIM),
                ),
            ]));
        }

        lines.push(Line::from(""));

        // Summary
        if !fr.summary.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    " Summary",
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]));
            // Word-wrap the summary to fit the panel
            let max_w = area.width.saturating_sub(3) as usize;
            for wrapped in word_wrap(&fr.summary, max_w) {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {}", wrapped),
                        ratatui::style::Style::default().fg(styles::TEXT),
                    ),
                ]));
            }
            lines.push(Line::from(""));
        }

        // Findings list
        if !fr.findings.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" Findings ({})", fr.findings.len()),
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));

            for finding in &fr.findings {
                let sev_style = if file_stale {
                    styles::stale_style()
                } else {
                    match finding.severity {
                        RiskLevel::High => styles::risk_high(),
                        RiskLevel::Medium => styles::risk_medium(),
                        RiskLevel::Low => styles::risk_low(),
                        RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE),
                    }
                };

                // Finding header: severity icon + category + title
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {} ", finding.severity.symbol()),
                        sev_style,
                    ),
                    Span::styled(
                        format!("[{}] ", finding.category),
                        ratatui::style::Style::default().fg(styles::DIM),
                    ),
                    Span::styled(
                        &finding.title,
                        ratatui::style::Style::default().fg(styles::ORANGE),
                    ),
                ]));

                // Hunk reference
                if let Some(hi) = finding.hunk_index {
                    let line_ref = finding.line_start
                        .map(|l| format!(" L{}", l))
                        .unwrap_or_default();
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("   hunk #{}{}", hi + 1, line_ref),
                            ratatui::style::Style::default().fg(styles::MUTED),
                        ),
                    ]));
                }

                // Description (word-wrapped)
                if !finding.description.is_empty() {
                    let max_w = area.width.saturating_sub(5) as usize;
                    for wrapped in word_wrap(&finding.description, max_w) {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("   {}", wrapped),
                                ratatui::style::Style::default().fg(styles::TEXT),
                            ),
                        ]));
                    }
                }

                // Suggestion
                if !finding.suggestion.is_empty() {
                    let max_w = area.width.saturating_sub(7) as usize;
                    for wrapped in word_wrap(&finding.suggestion, max_w) {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("   → {}", wrapped),
                                ratatui::style::Style::default().fg(styles::GREEN),
                            ),
                        ]));
                    }
                }

                lines.push(Line::from(""));
            }
        }

        // Human comments for this file
        if let Some(fb) = &tab.ai.feedback {
            let file_comments: Vec<_> = fb.comments.iter()
                .filter(|c| c.file == path && c.in_reply_to.is_none())
                .collect();
            if !file_comments.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" Comments ({})", file_comments.len()),
                        ratatui::style::Style::default()
                            .fg(styles::CYAN)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(""));

                for comment in &file_comments {
                    let target = comment.hunk_index
                        .map(|hi| {
                            comment.line_start
                                .map(|l| format!("h{}:L{}", hi + 1, l))
                                .unwrap_or_else(|| format!("h{}", hi + 1))
                        })
                        .unwrap_or_else(|| "file".to_string());

                    let author = if comment.author.is_empty() { "You" } else { &comment.author };

                    let mut header_spans = vec![
                        Span::styled(
                            " 💬 ",
                            styles::comment_style(),
                        ),
                        Span::styled(
                            author.to_string(),
                            ratatui::style::Style::default()
                                .fg(styles::CYAN)
                                .add_modifier(ratatui::style::Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("  {}", target),
                            ratatui::style::Style::default().fg(styles::DIM),
                        ),
                    ];

                    if comment.synced {
                        header_spans.push(Span::styled(
                            "  ↑ synced",
                            ratatui::style::Style::default().fg(styles::GREEN),
                        ));
                    }

                    lines.push(Line::from(header_spans));

                    let max_w = area.width.saturating_sub(5) as usize;
                    for wrapped in word_wrap(&comment.comment, max_w) {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("   {}", wrapped),
                                ratatui::style::Style::default().fg(styles::TEXT),
                            ),
                        ]));
                    }

                    // Render replies
                    let replies = tab.ai.replies_to(&comment.id);
                    for reply in &replies {
                        let reply_author = if reply.author.is_empty() { "You" } else { &reply.author };
                        lines.push(Line::from(vec![
                            Span::styled(
                                "   ↳ 💬 ",
                                ratatui::style::Style::default().fg(styles::DIM),
                            ),
                            Span::styled(
                                reply_author.to_string(),
                                ratatui::style::Style::default()
                                    .fg(styles::CYAN)
                                    .add_modifier(ratatui::style::Modifier::BOLD),
                            ),
                        ]));
                        for wrapped in word_wrap(&reply.comment, max_w.saturating_sub(4)) {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("       {}", wrapped),
                                    ratatui::style::Style::default().fg(styles::TEXT),
                                ),
                            ]));
                        }
                    }

                    lines.push(Line::from(""));
                }
            }
        }
    } else {
        // No file selected or no AI data for this file
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                " No AI data for this file",
                ratatui::style::Style::default().fg(styles::MUTED),
            ),
        ]));
    }

    let stale_tag = if file_stale {
        " [stale]"
    } else if ai_stale {
        " [stale]"
    } else {
        ""
    };
    let title = format!(" AI Panel{} ", stale_tag);
    let title_style = if stale_tag.is_empty() {
        ratatui::style::Style::default().fg(styles::PURPLE)
    } else {
        styles::stale_style()
    };

    let block = Block::default()
        .title(Span::styled(title, title_style))
        .borders(Borders::LEFT)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER))
        .style(ratatui::style::Style::default().bg(styles::SURFACE))
        .padding(Padding::new(0, 1, 0, 0));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((tab.ai_panel_scroll, 0));

    f.render_widget(paragraph, area);
}

