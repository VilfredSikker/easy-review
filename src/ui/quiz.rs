use crate::app::{App, QuizAnswer, QuizInputMode};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::styles;
use super::utils::word_wrap;

/// Render the left quiz question list panel.
pub fn render_quiz_list(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let quiz = match &tab.quiz {
        Some(q) => q,
        None => {
            let block = Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(styles::BORDER()));
            f.render_widget(block, area);
            return;
        }
    };

    let visible_indices = tab.quiz_visible_indices();
    let total = quiz.questions.len();
    let answered = quiz.answers.len();

    let mut lines: Vec<Line> = Vec::new();

    // Header
    let filter_label = match quiz.filter_level {
        None => "All".to_string(),
        Some(l) => format!("L{}", l),
    };
    lines.push(Line::from(vec![
        Span::styled(
            " Questions ",
            Style::default()
                .fg(styles::BRIGHT())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(
            format!("[{}] ", filter_label),
            Style::default().fg(styles::DIM()),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!(
            " \u{2500}{}",
            "\u{2500}".repeat(area.width.saturating_sub(2) as usize)
        ),
        Style::default().fg(styles::BORDER()),
    )));

    let inner_width = area.width.saturating_sub(2) as usize;

    for &idx in &visible_indices {
        let q = &quiz.questions[idx];
        let is_current = idx == quiz.current;

        // Status marker
        let (status_str, status_style) = if let Some(answer) = quiz.answers.get(&q.id) {
            match answer {
                QuizAnswer::Choice(c) => {
                    let correct = q
                        .options
                        .as_ref()
                        .and_then(|opts| opts.iter().find(|o| o.label == *c))
                        .map(|o| o.is_correct)
                        .unwrap_or(false);
                    if correct {
                        ("\u{2713}", Style::default().fg(styles::GREEN()))
                    } else {
                        ("\u{2717}", Style::default().fg(styles::RED()))
                    }
                }
                QuizAnswer::Freeform(_) => ("\u{2713}", Style::default().fg(styles::CYAN())),
            }
        } else if is_current {
            ("\u{25B6}", Style::default().fg(styles::BLUE()))
        } else {
            (" ", Style::default().fg(styles::MUTED()))
        };

        // Level badge color
        let level_style = match q.level {
            1 => Style::default().fg(styles::BLUE()),
            2 => Style::default().fg(styles::ORANGE()),
            _ => Style::default().fg(styles::RED()),
        };
        let level_badge = format!("[L{}]", q.level);

        // Category (3 chars abbreviated)
        let cat_abbr = if q.category.is_empty() {
            "   ".to_string()
        } else {
            let chars: String = q.category.chars().take(3).collect();
            format!("{:<3}", chars)
        };

        // Question number
        let num_str = format!("{:2}", idx + 1);

        // Background for current
        let bg_style = if is_current {
            Style::default().bg(styles::LINE_CURSOR_BG())
        } else {
            Style::default()
        };

        // Build spans for the line
        let prefix_width = 1 + 1 + 2 + 1 + 4 + 1 + 3 + 1; // status + space + num + space + badge + space + cat + space
        let text_width = inner_width.saturating_sub(prefix_width);
        let truncated_text: String = q.text.chars().take(text_width).collect();

        let line = Line::from(vec![
            Span::styled(" ", bg_style),
            Span::styled(status_str, status_style.patch(bg_style)),
            Span::raw(" "),
            Span::styled(num_str, Style::default().fg(styles::DIM()).patch(bg_style)),
            Span::raw(" "),
            Span::styled(level_badge, level_style.patch(bg_style)),
            Span::raw(" "),
            Span::styled(
                cat_abbr,
                Style::default().fg(styles::MUTED()).patch(bg_style),
            ),
            Span::raw(" "),
            Span::styled(
                truncated_text,
                Style::default().fg(styles::TEXT()).patch(bg_style),
            ),
        ]);
        lines.push(line);
    }

    // Spacer
    while lines.len() < (area.height as usize).saturating_sub(3) {
        lines.push(Line::default());
    }

    // Score and progress at bottom
    let (correct, attempted) = quiz.score;
    let pct = if attempted > 0 {
        (correct * 100) / attempted
    } else {
        0
    };
    lines.push(Line::from(Span::styled(
        format!(
            " \u{2500}{}",
            "\u{2500}".repeat(area.width.saturating_sub(2) as usize)
        ),
        Style::default().fg(styles::BORDER()),
    )));
    lines.push(Line::from(vec![
        Span::styled(" Score: ", Style::default().fg(styles::DIM())),
        Span::styled(
            format!("{}/{}", correct, attempted),
            Style::default().fg(styles::BRIGHT()),
        ),
        Span::styled(format!(" ({}%)", pct), Style::default().fg(styles::CYAN())),
        Span::styled(
            format!("  {}/{}", answered, total),
            Style::default().fg(styles::MUTED()),
        ),
    ]));

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(styles::BORDER()));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // Render with scroll to keep current question visible
    let current_visible_pos = visible_indices
        .iter()
        .position(|&i| i == quiz.current)
        .unwrap_or(0);
    // +2 for header lines
    let content_start = current_visible_pos + 2;
    let scroll = if content_start + 2 >= inner_area.height as usize {
        (content_start + 2 - inner_area.height as usize) as u16
    } else {
        0
    };

    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    f.render_widget(paragraph, inner_area);
}

/// Render the right quiz question/answer panel.
pub fn render_quiz_question(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let quiz = match &tab.quiz {
        Some(q) => q,
        None => {
            let block = Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(styles::BORDER()));
            f.render_widget(block, area);
            return;
        }
    };

    let question = match quiz.questions.get(quiz.current) {
        Some(q) => q,
        None => {
            let block = Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(styles::BORDER()));
            f.render_widget(block, area);
            return;
        }
    };

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(styles::BORDER()));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let width = inner_area.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Header: level + category
    let level_name = match question.level {
        1 => "Comprehension",
        2 => "Analysis",
        3 => "Evaluation",
        _ => "Unknown",
    };
    let level_style = match question.level {
        1 => Style::default().fg(styles::BLUE()),
        2 => Style::default().fg(styles::ORANGE()),
        _ => Style::default().fg(styles::RED()),
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!(" Level {} \u{00B7} {}", question.level, level_name),
            level_style.add_modifier(ratatui::style::Modifier::BOLD),
        ),
        if !question.category.is_empty() {
            Span::styled(
                format!("  {}", question.category),
                Style::default().fg(styles::MUTED()),
            )
        } else {
            Span::raw("")
        },
    ]));
    lines.push(Line::from(Span::styled(
        "\u{2500}".repeat(width),
        Style::default().fg(styles::BORDER()),
    )));
    lines.push(Line::raw(""));

    // Question text (word-wrapped)
    let question_text = format!(" {}", question.text);
    for wrapped_line in word_wrap(&question_text, width.saturating_sub(2)) {
        lines.push(Line::from(Span::styled(
            wrapped_line,
            Style::default().fg(styles::BRIGHT()),
        )));
    }
    lines.push(Line::raw(""));

    let already_answered = quiz.answers.contains_key(&question.id);

    if let Some(ref options) = question.options {
        // Multiple choice
        for opt in options {
            let (bg, fg) = if already_answered {
                if let Some(QuizAnswer::Choice(chosen)) = quiz.answers.get(&question.id) {
                    if opt.label == *chosen {
                        if opt.is_correct {
                            (styles::ADD_BG(), styles::ADD_TEXT())
                        } else {
                            (styles::DEL_BG(), styles::DEL_TEXT())
                        }
                    } else if opt.is_correct {
                        (styles::ADD_BG(), styles::ADD_TEXT())
                    } else {
                        (styles::BG(), styles::MUTED())
                    }
                } else {
                    (styles::BG(), styles::TEXT())
                }
            } else {
                (styles::BG(), styles::TEXT())
            };

            let opt_style = Style::default().fg(fg).bg(bg);
            let opt_text: String = format!("  {}  {}", opt.label, opt.text)
                .chars()
                .take(width)
                .collect();
            lines.push(Line::from(Span::styled(opt_text, opt_style)));
        }
    } else if question.freeform {
        // Freeform question
        lines.push(Line::from(Span::styled(
            " [Freeform Answer]",
            Style::default().fg(styles::CYAN()),
        )));
        lines.push(Line::raw(""));

        if quiz.input_mode == QuizInputMode::AnsweringFreeform {
            // Show text input area
            lines.push(Line::from(Span::styled(
                " Type your answer (Enter to submit, Esc to cancel):",
                Style::default().fg(styles::DIM()),
            )));
            let input_display = format!(" > {}_", quiz.input_buffer);
            for wrapped in word_wrap(&input_display, width.saturating_sub(2)) {
                lines.push(Line::from(Span::styled(
                    wrapped,
                    Style::default().fg(styles::TEXT()).bg(styles::SURFACE()),
                )));
            }
        } else if let Some(QuizAnswer::Freeform(text)) = quiz.answers.get(&question.id) {
            lines.push(Line::from(Span::styled(
                " Your answer:",
                Style::default().fg(styles::DIM()),
            )));
            for wrapped in word_wrap(text, width.saturating_sub(2)) {
                lines.push(Line::from(Span::styled(
                    format!(" {}", wrapped),
                    Style::default().fg(styles::TEXT()),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                " Press Enter to write your answer",
                Style::default().fg(styles::MUTED()),
            )));
        }
    }

    // Explanation (shown after answering or when toggled with 'e')
    if already_answered && quiz.show_explanation && !question.explanation.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "\u{2500}".repeat(width),
            Style::default().fg(styles::BORDER()),
        )));
        lines.push(Line::from(Span::styled(
            " Explanation",
            Style::default()
                .fg(styles::CYAN())
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));
        lines.push(Line::raw(""));
        for wrapped in word_wrap(&question.explanation, width.saturating_sub(2)) {
            lines.push(Line::from(Span::styled(
                format!(" {}", wrapped),
                Style::default().fg(styles::TEXT()),
            )));
        }
    } else if already_answered && !question.explanation.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            " Press 'e' to show explanation",
            Style::default().fg(styles::MUTED()),
        )));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner_area);
}
