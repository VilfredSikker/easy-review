use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, DiffMode, InputMode};
use crate::ai::ViewMode;
use super::styles;

/// Compute the display width of a list of spans
fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// Calculate how many rows the top bar needs
pub fn top_bar_height(app: &App, _width: u16) -> u16 {
    if app.tabs.len() > 1 {
        3 // tabs row + branch info row + modes row
    } else {
        2 // branch info row + modes row
    }
}

/// Render the top status bar
///
/// Multi-tab layout (3 rows):
///   Row 1: Tab 1 │ Tab 2 │ Tab 3
///   Row 2: repo · branch (vs base)
///   Row 3: 1 BRANCH  2 UNSTAGED  3 STAGED          x/y reviewed
///
/// Single-tab layout (2 rows):
///   Row 1: repo · branch (vs base)
///   Row 2: 1 BRANCH  2 UNSTAGED  3 STAGED          x/y reviewed
pub fn render_top_bar(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let bar_width = area.width as usize;

    let mode_style = |mode: DiffMode, current: DiffMode| {
        if mode == current {
            ratatui::style::Style::default()
                .fg(styles::BG)
                .bg(styles::BLUE)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            ratatui::style::Style::default().fg(styles::MUTED)
        }
    };

    let panel_bg = ratatui::style::Style::default().bg(styles::PANEL);
    let has_tabs = app.tabs.len() > 1;

    // Split area into rows
    let row_count = if has_tabs { 3u16 } else { 2u16 };
    let constraints: Vec<ratatui::layout::Constraint> = (0..row_count)
        .map(|_| ratatui::layout::Constraint::Length(1))
        .collect();
    let rows = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut row_idx: usize = 0;

    // ── Tab row (multi-tab only) ──
    if has_tabs {
        let mut tab_spans: Vec<Span> = Vec::new();
        for (i, t) in app.tabs.iter().enumerate() {
            let label = format!(" {} ", t.tab_name());
            if i == app.active_tab {
                tab_spans.push(Span::styled(
                    label,
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT)
                        .bg(styles::BLUE)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ));
            } else {
                tab_spans.push(Span::styled(
                    label,
                    ratatui::style::Style::default().fg(styles::DIM),
                ));
            }
            if i < app.tabs.len() - 1 {
                tab_spans.push(Span::styled(
                    " │ ",
                    ratatui::style::Style::default().fg(styles::BORDER),
                ));
            }
        }
        let tab_bar = Paragraph::new(Line::from(tab_spans)).style(panel_bg);
        f.render_widget(tab_bar, rows[row_idx]);
        row_idx += 1;
    }

    // ── Branch info row: repo · branch (vs base) ──
    let repo_name = tab.tab_name();
    let mut info_spans: Vec<Span> = vec![
        Span::styled(
            format!(" {}", repo_name),
            ratatui::style::Style::default()
                .fg(styles::CYAN)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(" · ", ratatui::style::Style::default().fg(styles::BORDER)),
        Span::styled(
            &tab.current_branch,
            ratatui::style::Style::default().fg(styles::GREEN),
        ),
    ];
    if tab.mode == DiffMode::Branch {
        info_spans.push(Span::styled(
            format!(" (vs {})", tab.base_branch),
            ratatui::style::Style::default().fg(styles::DIM),
        ));
    }
    let info_bar = Paragraph::new(Line::from(info_spans)).style(panel_bg);
    f.render_widget(info_bar, rows[row_idx]);
    row_idx += 1;

    // ── Modes row: modes (left) + reviewed (right) ──
    let mut modes: Vec<Span> = vec![
        Span::raw(" "),
        Span::styled(" 1 ", mode_style(DiffMode::Branch, tab.mode)),
        Span::styled(" BRANCH ", mode_style(DiffMode::Branch, tab.mode)),
        Span::raw(" "),
        Span::styled(" 2 ", mode_style(DiffMode::Unstaged, tab.mode)),
        Span::styled(" UNSTAGED ", mode_style(DiffMode::Unstaged, tab.mode)),
        Span::raw(" "),
        Span::styled(" 3 ", mode_style(DiffMode::Staged, tab.mode)),
        Span::styled(" STAGED ", mode_style(DiffMode::Staged, tab.mode)),
    ];

    let mut right: Vec<Span> = Vec::new();

    // AI view mode + staleness indicator
    if tab.ai.has_data() {
        if tab.ai.is_stale {
            let stale_count = tab.ai.stale_files.len();
            let stale_label = if stale_count > 0 {
                format!("⚠ {} file{} changed", stale_count, if stale_count == 1 { "" } else { "s" })
            } else {
                "⚠ AI stale".to_string()
            };
            right.push(Span::styled(stale_label, styles::stale_style()));
            right.push(Span::raw("  "));
        }
        if tab.ai.view_mode != ViewMode::Default {
            right.push(Span::styled(
                format!("⬡ {}", tab.ai.view_mode.label()),
                ratatui::style::Style::default()
                    .fg(styles::PURPLE)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ));
            right.push(Span::raw("  "));
        }
    }

    let (reviewed, total) = tab.reviewed_count();
    if total > 0 {
        right.push(Span::styled(
            format!("{}/{} reviewed", reviewed, total),
            ratatui::style::Style::default().fg(styles::PURPLE),
        ));
    }
    if app.watching {
        if !right.is_empty() {
            right.push(Span::raw("  "));
        }
        right.push(Span::styled(
            "\u{25cf} WATCHING",
            ratatui::style::Style::default()
                .fg(styles::GREEN)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
    }

    // Memory budget indicator (debug mode)
    if std::env::var("ER_DEBUG").is_ok() {
        let budget = &tab.mem_budget;
        if !right.is_empty() {
            right.push(Span::raw("  "));
        }
        let mut mem_label = format!(
            "MEM: {}K lines  {} files",
            (budget.total_lines + 500) / 1000,
            budget.parsed_files,
        );
        if budget.compacted_files > 0 {
            mem_label.push_str(&format!("  {} compacted", budget.compacted_files));
        }
        if tab.lazy_mode {
            mem_label.push_str("  [lazy]");
        }
        right.push(Span::styled(
            mem_label,
            ratatui::style::Style::default().fg(styles::DIM),
        ));
    }

    if !right.is_empty() {
        right.push(Span::raw(" "));
    }

    let modes_w = spans_width(&modes);
    let right_w = spans_width(&right);
    let gap = bar_width.saturating_sub(modes_w + right_w);
    modes.push(Span::raw(" ".repeat(gap)));
    modes.extend(right);

    let modes_bar = Paragraph::new(Line::from(modes)).style(panel_bg);
    f.render_widget(modes_bar, rows[row_idx]);
}

/// A key-label hint pair, e.g. ("s", " stage ")
struct Hint {
    key: String,
    label: String,
}

impl Hint {
    fn new(key: &str, label: &str) -> Self {
        Self { key: key.to_string(), label: label.to_string() }
    }
    fn width(&self) -> usize {
        self.key.len() + self.label.len()
    }
}

/// Build the hint list for AiReview mode
fn build_ai_review_hints(app: &App) -> Vec<Hint> {
    let tab = app.tab();
    let mut hints = vec![
        Hint::new("j/k", " nav "),
        Hint::new("Tab", " switch column "),
        Hint::new("␣", " toggle check "),
        Hint::new("Enter", " jump to file "),
        Hint::new("v/V", " view "),
        Hint::new("Esc", " default view "),
        Hint::new("q", " quit "),
    ];

    // Show which column is focused
    let focus_label = match tab.ai.review_focus {
        crate::ai::ReviewFocus::Files => " [Files] ",
        crate::ai::ReviewFocus::Checklist => " [Checklist] ",
    };
    hints.push(Hint {
        key: String::new(),
        label: focus_label.to_string(),
    });

    hints
}

/// Build the normal-mode hint list
fn build_hints(app: &App) -> Vec<Hint> {
    let tab = app.tab();

    // Delegate to AiReview-specific hints when in that mode
    if tab.ai.view_mode == ViewMode::AiReview {
        return build_ai_review_hints(app);
    }

    let mut hints = vec![
        Hint::new("j/k", " nav "),
        Hint::new("n/N", " hunks "),
        Hint::new("s", " stage "),
        Hint::new("S", " hunk "),
        Hint::new("␣", " review "),
        Hint::new("u", " unreviewed "),
        Hint::new("y", " yank "),
        Hint::new("/", " search "),
        Hint::new("r", " reload "),
        Hint::new("w", " watch "),
        Hint::new("e", " edit "),
        Hint::new("t", " tree "),
        Hint::new("o", " open "),
        Hint::new("q", " quit "),
    ];

    hints.push(Hint::new("c", " comment "));

    if tab.ai.has_data() {
        hints.push(Hint::new("v/V", " AI view "));
    }

    if app.tabs.len() > 1 {
        hints.push(Hint::new("[/]", " tabs "));
        hints.push(Hint::new("x", " close "));
    }

    // Indicators (not really key+label, but reuse the structure)
    if !tab.search_query.is_empty() {
        hints.push(Hint {
            key: String::new(),
            label: format!(" filter: \"{}\" ", tab.search_query),
        });
    }
    if tab.show_unreviewed_only {
        hints.push(Hint {
            key: String::new(),
            label: " [unreviewed] ".to_string(),
        });
    }

    hints
}

/// Pack hints into rows that fit within `width`, returns vec of Lines
fn pack_hint_lines(hints: &[Hint], width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_w: usize = 1; // leading space

    for hint in hints {
        let hw = hint.width();
        if current_w + hw > width && !current_spans.is_empty() {
            // Wrap to next line
            lines.push(Line::from(current_spans));
            current_spans = Vec::new();
            current_w = 1;
        }
        if current_spans.is_empty() {
            current_spans.push(Span::raw(" "));
        }
        if !hint.key.is_empty() {
            current_spans.push(Span::styled(
                hint.key.clone(),
                styles::key_hint_style(),
            ));
        }
        current_spans.push(Span::styled(
            hint.label.clone(),
            if hint.key.is_empty() {
                // Indicator style (filter, unreviewed)
                if hint.label.contains("filter") {
                    ratatui::style::Style::default().fg(styles::YELLOW)
                } else {
                    ratatui::style::Style::default().fg(styles::PURPLE)
                }
            } else {
                ratatui::style::Style::default().fg(styles::DIM)
            },
        ));
        current_w += hw;
    }
    if !current_spans.is_empty() {
        lines.push(Line::from(current_spans));
    }
    if lines.is_empty() {
        lines.push(Line::from(vec![Span::raw(" ")]));
    }
    lines
}

/// Calculate how many rows the bottom bar needs
pub fn bottom_bar_height(app: &App, width: u16) -> u16 {
    match app.input_mode {
        InputMode::Search | InputMode::Comment => 1,
        InputMode::Normal => {
            let hints = build_hints(app);
            let lines = pack_hint_lines(&hints, width as usize);
            (lines.len() as u16).max(1)
        }
    }
}

/// Render the bottom keybinding hints bar
pub fn render_bottom_bar(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let panel_bg = ratatui::style::Style::default().bg(styles::PANEL);

    match app.input_mode {
        InputMode::Comment => {
            // Show: 💬 file:hunk > comment_input█  Enter send  Esc cancel
            let file_short = tab.comment_file.rsplit('/').next().unwrap_or(&tab.comment_file);
            let target_label = if let Some(ln) = tab.comment_line_num {
                format!("{}:L{}", file_short, ln)
            } else {
                format!("{}:h{}", file_short, tab.comment_hunk + 1)
            };
            let spans = vec![
                Span::styled(" comment ", ratatui::style::Style::default()
                    .fg(styles::BG)
                    .bg(styles::CYAN)
                    .add_modifier(ratatui::style::Modifier::BOLD)),
                Span::styled(
                    format!(" {} ", target_label),
                    ratatui::style::Style::default().fg(styles::DIM),
                ),
                Span::styled(
                    format!("{}", tab.comment_input),
                    ratatui::style::Style::default().fg(styles::TEXT),
                ),
                Span::styled(
                    "█",
                    ratatui::style::Style::default().fg(styles::CYAN),
                ),
                Span::styled("  ", ratatui::style::Style::default()),
                Span::styled("Enter", styles::key_hint_style()),
                Span::styled(" send  ", ratatui::style::Style::default().fg(styles::DIM)),
                Span::styled("Esc", styles::key_hint_style()),
                Span::styled(" cancel", ratatui::style::Style::default().fg(styles::DIM)),
            ];
            let bar = Paragraph::new(Line::from(spans)).style(panel_bg);
            f.render_widget(bar, area);
        }
        InputMode::Search => {
            let spans = vec![
                Span::styled(" /", styles::key_hint_style()),
                Span::styled(
                    format!(" {}", tab.search_query),
                    ratatui::style::Style::default().fg(styles::TEXT),
                ),
                Span::styled(
                    "█",
                    ratatui::style::Style::default().fg(styles::BLUE),
                ),
                Span::styled("  ", ratatui::style::Style::default()),
                Span::styled("Enter", styles::key_hint_style()),
                Span::styled(" confirm  ", ratatui::style::Style::default().fg(styles::DIM)),
                Span::styled("Esc", styles::key_hint_style()),
                Span::styled(" cancel", ratatui::style::Style::default().fg(styles::DIM)),
            ];
            let bar = Paragraph::new(Line::from(spans)).style(panel_bg);
            f.render_widget(bar, area);
        }
        InputMode::Normal => {
            let hints = build_hints(app);
            let lines = pack_hint_lines(&hints, area.width as usize);

            // Split area into rows
            let row_count = lines.len() as u16;
            let constraints: Vec<ratatui::layout::Constraint> = (0..row_count)
                .map(|_| ratatui::layout::Constraint::Length(1))
                .collect();
            let rows = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints(constraints)
                .split(area);

            for (i, line) in lines.into_iter().enumerate() {
                let bar = Paragraph::new(line).style(panel_bg);
                f.render_widget(bar, rows[i]);
            }
        }
    }
}

/// Render watch notification overlay
pub fn render_watch_notification(f: &mut Frame, area: Rect, message: &str) {
    let notif_width = message.len() as u16 + 4;
    let notif_x = area.x + area.width.saturating_sub(notif_width + 2);
    let notif_y = area.y + 2;

    let notif_area = Rect {
        x: notif_x,
        y: notif_y,
        width: notif_width.min(area.width),
        height: 1,
    };

    let notif = Paragraph::new(Line::from(vec![
        Span::styled(
            " ● ",
            ratatui::style::Style::default().fg(styles::GREEN),
        ),
        Span::styled(
            message,
            ratatui::style::Style::default().fg(styles::TEXT),
        ),
        Span::raw(" "),
    ]))
    .style(
        ratatui::style::Style::default()
            .bg(styles::PANEL)
            .fg(styles::TEXT),
    );

    f.render_widget(notif, notif_area);
}
