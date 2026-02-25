use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};

use crate::ai::agent::{AgentContext, AgentState, MessageRole};
use crate::ai::{PanelTab, RiskLevel};
use crate::app::{App, InputMode};
use super::styles;

/// Render the AI side panel (right side, in SidePanel view mode)
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();

    // Split: tab bar (1 line) + content
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(area);

    render_tab_bar(f, chunks[0], tab.ai.panel_tab);

    match tab.ai.panel_tab {
        PanelTab::Review => render_review_content(f, chunks[1], app),
        PanelTab::Agent => render_agent_content(f, chunks[1], app),
    }
}

// ── Tab Bar ──

fn render_tab_bar(f: &mut Frame, area: Rect, active: PanelTab) {
    let review_style = if active == PanelTab::Review {
        styles::tab_active_style()
    } else {
        styles::tab_inactive_style()
    };
    let agent_style = if active == PanelTab::Agent {
        styles::tab_active_style()
    } else {
        styles::tab_inactive_style()
    };

    let line = Line::from(vec![
        Span::styled(" ", Style::default().bg(styles::SURFACE)),
        Span::styled(" Review ", review_style.bg(styles::SURFACE)),
        Span::styled("  ", Style::default().fg(styles::MUTED).bg(styles::SURFACE)),
        Span::styled(" Agent ", agent_style.bg(styles::SURFACE)),
        Span::styled(
            " ".repeat(area.width.saturating_sub(20) as usize),
            Style::default().bg(styles::SURFACE),
        ),
    ]);

    let bar = Paragraph::new(vec![line])
        .style(Style::default().bg(styles::SURFACE));
    f.render_widget(bar, area);
}

// ── Review Tab (existing logic, extracted) ──

fn render_review_content(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let ai_stale = tab.ai.is_stale;

    let mut lines: Vec<Line> = Vec::new();

    let file = tab.selected_diff_file();
    let file_path = file.map(|f| f.path.as_str());
    let fr = file_path.and_then(|p| tab.ai.file_review(p));

    if let (Some(path), Some(fr)) = (file_path, fr) {
        let risk_style = if ai_stale {
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
            RiskLevel::High => "HIGH RISK",
            RiskLevel::Medium => "MEDIUM RISK",
            RiskLevel::Low => "LOW RISK",
            RiskLevel::Info => "INFO",
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", fr.risk.symbol()), risk_style),
            Span::styled(risk_label, risk_style),
        ]));

        if !fr.risk_reason.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!(" {}", fr.risk_reason), Style::default().fg(styles::DIM)),
            ]));
        }

        lines.push(Line::from(""));

        // Summary
        if !fr.summary.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    " Summary",
                    Style::default().fg(styles::BRIGHT).add_modifier(Modifier::BOLD),
                ),
            ]));
            let max_w = area.width.saturating_sub(3) as usize;
            for wrapped in word_wrap(&fr.summary, max_w) {
                lines.push(Line::from(vec![
                    Span::styled(format!(" {}", wrapped), Style::default().fg(styles::TEXT)),
                ]));
            }
            lines.push(Line::from(""));
        }

        // Findings
        if !fr.findings.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" Findings ({})", fr.findings.len()),
                    Style::default().fg(styles::BRIGHT).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));

            for finding in &fr.findings {
                let sev_style = if ai_stale {
                    styles::stale_style()
                } else {
                    match finding.severity {
                        RiskLevel::High => styles::risk_high(),
                        RiskLevel::Medium => styles::risk_medium(),
                        RiskLevel::Low => styles::risk_low(),
                        RiskLevel::Info => Style::default().fg(styles::BLUE),
                    }
                };

                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", finding.severity.symbol()), sev_style),
                    Span::styled(
                        format!("[{}] ", finding.category),
                        Style::default().fg(styles::DIM),
                    ),
                    Span::styled(&finding.title, Style::default().fg(styles::ORANGE)),
                ]));

                if let Some(hi) = finding.hunk_index {
                    let line_ref = finding
                        .line_start
                        .map(|l| format!(" L{}", l))
                        .unwrap_or_default();
                    lines.push(Line::from(vec![Span::styled(
                        format!("   hunk #{}{}", hi + 1, line_ref),
                        Style::default().fg(styles::MUTED),
                    )]));
                }

                if !finding.description.is_empty() {
                    let max_w = area.width.saturating_sub(5) as usize;
                    for wrapped in word_wrap(&finding.description, max_w) {
                        lines.push(Line::from(vec![Span::styled(
                            format!("   {}", wrapped),
                            Style::default().fg(styles::TEXT),
                        )]));
                    }
                }

                if !finding.suggestion.is_empty() {
                    let max_w = area.width.saturating_sub(7) as usize;
                    for wrapped in word_wrap(&finding.suggestion, max_w) {
                        lines.push(Line::from(vec![Span::styled(
                            format!("   > {}", wrapped),
                            Style::default().fg(styles::GREEN),
                        )]));
                    }
                }

                lines.push(Line::from(""));
            }
        }

        // Human comments
        if let Some(fb) = &tab.ai.feedback {
            let file_comments: Vec<_> = fb.comments.iter().filter(|c| c.file == path).collect();
            if !file_comments.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    format!(" Comments ({})", file_comments.len()),
                    Style::default()
                        .fg(styles::CYAN)
                        .add_modifier(Modifier::BOLD),
                )]));
                lines.push(Line::from(""));

                for comment in file_comments {
                    let target = comment
                        .hunk_index
                        .map(|hi| {
                            comment
                                .line_start
                                .map(|l| format!("h{}:L{}", hi + 1, l))
                                .unwrap_or_else(|| format!("h{}", hi + 1))
                        })
                        .unwrap_or_else(|| "file".to_string());

                    lines.push(Line::from(vec![
                        Span::raw(" "),
                        Span::styled(target, Style::default().fg(styles::DIM)),
                    ]));

                    let max_w = area.width.saturating_sub(5) as usize;
                    for wrapped in word_wrap(&comment.comment, max_w) {
                        lines.push(Line::from(vec![Span::styled(
                            format!("   {}", wrapped),
                            Style::default().fg(styles::TEXT),
                        )]));
                    }
                    lines.push(Line::from(""));
                }
            }
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            " No AI data for this file",
            Style::default().fg(styles::MUTED),
        )]));
    }

    let stale_tag = if ai_stale { " [stale]" } else { "" };
    let title = format!(" Review{} ", stale_tag);

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(styles::PURPLE)))
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(styles::BORDER))
        .style(Style::default().bg(styles::SURFACE))
        .padding(Padding::new(0, 1, 0, 0));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((tab.diff_scroll, 0));

    f.render_widget(paragraph, area);
}

// ── Agent Tab ──

fn render_agent_content(f: &mut Frame, area: Rect, app: &App) {
    let agent = &app.tab().ai.agent;
    let is_prompt_mode = app.input_mode == InputMode::AgentPrompt;

    // Split: context badge | conversation | prompt input
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .split(area);

    render_context_badge(f, chunks[0], &agent.context);
    render_conversation(f, chunks[1], agent);
    render_prompt_input(f, chunks[2], &agent.input, agent.is_running, is_prompt_mode);
}

fn render_context_badge(f: &mut Frame, area: Rect, ctx: &AgentContext) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(ref file) = ctx.file {
        let mut breadcrumb = file.clone();
        if let Some(idx) = ctx.hunk_index {
            breadcrumb.push_str(&format!(" > hunk#{}", idx + 1));
        }
        if let Some(ln) = ctx.line_number {
            breadcrumb.push_str(&format!(" > L{}", ln));
        }
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {}", breadcrumb),
                Style::default().fg(styles::TEXT),
            ),
        ]));

        if let Some(ref title) = ctx.finding_title {
            let sev = ctx.finding_severity.as_deref().unwrap_or("");
            let sev_color = match sev {
                "High" => styles::RED,
                "Medium" => styles::ORANGE,
                "Low" => styles::YELLOW,
                _ => styles::BLUE,
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" [{sev}] {title}"),
                    Style::default().fg(sev_color),
                ),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                " Navigate to a file to set context",
                Style::default().fg(styles::MUTED),
            ),
        ]));
    }

    let block = Block::default()
        .borders(Borders::LEFT | Borders::BOTTOM)
        .border_style(Style::default().fg(styles::BORDER))
        .style(Style::default().bg(styles::AGENT_BADGE_BG))
        .padding(Padding::new(0, 1, 0, 0));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

fn render_conversation(f: &mut Frame, area: Rect, agent: &AgentState) {
    let mut lines: Vec<Line> = Vec::new();
    let max_w = area.width.saturating_sub(4) as usize;

    if agent.messages.is_empty() && !agent.is_running {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            " Ask about this code...",
            Style::default().fg(styles::MUTED),
        )]));
    } else {
        for msg in &agent.messages {
            let (prefix, prefix_style) = match msg.role {
                MessageRole::User => ("you: ", styles::agent_user_style()),
                MessageRole::Agent => ("agent: ", styles::agent_response_style()),
                MessageRole::System => ("", styles::agent_system_style()),
            };

            let text_style = match msg.role {
                MessageRole::System => styles::agent_system_style(),
                _ => Style::default().fg(styles::TEXT),
            };

            // First line with prefix
            let wrapped = word_wrap(&msg.text, max_w.saturating_sub(prefix.len()));
            if let Some((first, rest)) = wrapped.split_first() {
                lines.push(Line::from(vec![
                    Span::styled(format!(" {}", prefix), prefix_style),
                    Span::styled(first.clone(), text_style),
                ]));
                for line in rest {
                    let indent = " ".repeat(prefix.len() + 1);
                    lines.push(Line::from(vec![
                        Span::styled(format!("{}{}", indent, line), text_style),
                    ]));
                }
            }
            lines.push(Line::from(""));
        }
    }

    // Streaming partial response
    if agent.is_running && !agent.partial_response.is_empty() {
        let wrapped = word_wrap(&agent.partial_response, max_w.saturating_sub(8));
        if let Some((first, rest)) = wrapped.split_first() {
            lines.push(Line::from(vec![
                Span::styled(" agent: ", styles::agent_response_style()),
                Span::styled(first.clone(), Style::default().fg(styles::TEXT)),
            ]));
            for line in rest {
                lines.push(Line::from(vec![Span::styled(
                    format!("        {}", line),
                    Style::default().fg(styles::TEXT),
                )]));
            }
        }
        // Block cursor at end
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled(
                " \u{2588}",
                Style::default().fg(styles::GREEN),
            ));
        }
    } else if agent.is_running {
        lines.push(Line::from(vec![
            Span::styled(" agent: ", styles::agent_response_style()),
            Span::styled("\u{2588}", Style::default().fg(styles::GREEN)),
        ]));
    }

    // Auto-scroll: compute scroll to show bottom
    let total_lines = lines.len() as u16;
    let visible_h = area.height.saturating_sub(2); // account for block borders
    let scroll = if total_lines > visible_h {
        total_lines - visible_h
    } else {
        agent.scroll
    };

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(styles::BORDER))
        .style(Style::default().bg(styles::SURFACE))
        .padding(Padding::new(0, 1, 0, 0));

    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    f.render_widget(paragraph, area);
}

fn render_prompt_input(
    f: &mut Frame,
    area: Rect,
    input: &str,
    is_running: bool,
    is_focused: bool,
) {
    let line = if is_running {
        Line::from(vec![Span::styled(
            " ... running",
            Style::default()
                .fg(styles::YELLOW)
                .add_modifier(Modifier::ITALIC),
        )])
    } else if input.is_empty() && !is_focused {
        Line::from(vec![Span::styled(
            " > Ask about this code...",
            Style::default().fg(styles::MUTED),
        )])
    } else {
        Line::from(vec![
            Span::styled(" > ", Style::default().fg(styles::PURPLE)),
            Span::styled(input, styles::agent_prompt_style()),
            Span::styled(
                if is_focused { "\u{2588}" } else { "" },
                Style::default().fg(styles::TEXT),
            ),
        ])
    };

    let block = Block::default()
        .borders(Borders::LEFT | Borders::TOP)
        .border_style(Style::default().fg(styles::BORDER))
        .style(Style::default().bg(styles::SURFACE));

    let paragraph = Paragraph::new(vec![line]).block(block);
    f.render_widget(paragraph, area);
}

// ── Helpers ──

fn word_wrap(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    for line in text.lines() {
        if line.len() <= max_width {
            result.push(line.to_string());
        } else {
            let mut current = String::new();
            for word in line.split_whitespace() {
                if current.is_empty() {
                    current = word.to_string();
                } else if current.len() + 1 + word.len() <= max_width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    result.push(current);
                    current = word.to_string();
                }
            }
            if !current.is_empty() {
                result.push(current);
            }
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}
