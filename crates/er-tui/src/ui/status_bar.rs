use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::styles;
use er_engine::ai::PanelContent;
use er_engine::app::{App, ConfirmAction, DiffMode, InputMode};

/// Compute the display width of a list of spans
fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// Calculate how many rows the top bar needs
pub const fn top_bar_height(app: &App, _width: u16) -> u16 {
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
                .fg(styles::BG())
                .bg(styles::BLUE())
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            ratatui::style::Style::default().fg(styles::MUTED())
        }
    };

    let panel_bg = ratatui::style::Style::default().bg(styles::PANEL());
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
                        .fg(styles::BRIGHT())
                        .bg(styles::BLUE())
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ));
            } else {
                tab_spans.push(Span::styled(
                    label,
                    ratatui::style::Style::default().fg(styles::DIM()),
                ));
            }
            if i < app.tabs.len() - 1 {
                tab_spans.push(Span::styled(
                    " │ ",
                    ratatui::style::Style::default().fg(styles::BORDER()),
                ));
            }
        }
        let tab_bar = Paragraph::new(Line::from(tab_spans)).style(panel_bg);
        f.render_widget(tab_bar, rows[row_idx]);
        row_idx += 1;
    }

    // ── Branch info row: repo · branch (vs base) ──
    // For remote tabs, show full owner/repo slug; tab_name() is compact for the tab bar
    let repo_label = tab
        .remote_repo
        .as_deref()
        .unwrap_or(&tab.tab_name())
        .to_string();
    let mut info_spans: Vec<Span> = vec![
        Span::styled(
            format!(" {}", repo_label),
            ratatui::style::Style::default()
                .fg(styles::CYAN())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled(" · ", ratatui::style::Style::default().fg(styles::BORDER())),
        Span::styled(
            &tab.current_branch,
            ratatui::style::Style::default().fg(styles::GREEN()),
        ),
    ];
    if let Some(pr_num) = tab.pr_number {
        info_spans.push(Span::styled(
            format!(" [PR #{}]", pr_num),
            ratatui::style::Style::default()
                .fg(styles::CYAN())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
    }
    if tab.is_remote() {
        info_spans.push(Span::styled(
            " [remote]",
            ratatui::style::Style::default().fg(styles::ORANGE()),
        ));
    }
    if matches!(
        tab.mode,
        DiffMode::Branch | DiffMode::History | DiffMode::Tour
    ) {
        info_spans.push(Span::styled(
            format!(" (vs {})", tab.base_branch),
            ratatui::style::Style::default().fg(styles::DIM()),
        ));
    }
    if tab.mode == DiffMode::Conflicts && tab.merge_active {
        info_spans.push(Span::styled(
            " [merge in progress]",
            ratatui::style::Style::default().fg(styles::ORANGE()),
        ));
    }
    // In History mode, show selected commit info
    if tab.mode == DiffMode::History {
        if let Some(ref history) = tab.history {
            if let Some(commit) = history.commits.get(history.selected_commit) {
                info_spans.push(Span::styled(
                    format!(" · {} · {}", commit.short_hash, commit.relative_date),
                    ratatui::style::Style::default().fg(styles::DIM()),
                ));
            }
        }
    }
    let info_bar = Paragraph::new(Line::from(info_spans)).style(panel_bg);
    f.render_widget(info_bar, rows[row_idx]);
    row_idx += 1;

    // ── Modes row: modes (left) + reviewed (right) ──
    let mut modes: Vec<Span> = vec![Span::raw(" ")];
    let visible = tab.visible_modes(&app.config);
    for (i, &vmode) in visible.iter().enumerate() {
        let num = format!(" {} ", i + 1);
        let label = match vmode {
            DiffMode::Branch => " BRANCH ",
            DiffMode::Unstaged => " UNSTAGED ",
            DiffMode::Staged => {
                if tab.mode == DiffMode::Staged && tab.committed_unpushed {
                    " COMMITTED "
                } else {
                    " STAGED "
                }
            }
            DiffMode::History => " HISTORY ",
            DiffMode::Conflicts => " CONFLICTS ",
            DiffMode::Hidden => " HIDDEN ",
            DiffMode::PrDiff => " PR DIFF ",
            DiffMode::Tour => " TOUR ",
        };
        modes.push(Span::styled(num, mode_style(vmode, tab.mode)));
        modes.push(Span::styled(label, mode_style(vmode, tab.mode)));
        modes.push(Span::raw(" "));
    }
    if tab.sort_by_mtime {
        modes.push(Span::raw(" "));
        modes.push(Span::styled(
            " m RECENT ",
            ratatui::style::Style::default()
                .fg(styles::BG())
                .bg(styles::YELLOW())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
    }

    let mut right: Vec<Span> = Vec::new();

    // Agent command status badges (persistent while running)
    for (name, _label, _is_running) in tab.agent_statuses() {
        let display_name = match name {
            "review" => "Review",
            "questions" => "Questions",
            _ => name,
        };
        right.push(Span::styled(
            format!(" ◐ {} running… ", display_name),
            ratatui::style::Style::default()
                .fg(styles::BG())
                .bg(styles::CYAN())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
        right.push(Span::raw("  "));
    }

    // AI badge + panel label
    if tab.ai.has_data() {
        if tab.ai.is_stale {
            let stale_count = tab.ai.stale_files.len();
            let stale_label = if stale_count > 0 {
                format!(
                    "⚠ {} file{} changed",
                    stale_count,
                    if stale_count == 1 { "" } else { "s" }
                )
            } else {
                "⚠ AI stale".to_string()
            };
            right.push(Span::styled(stale_label, styles::stale_style()));
            right.push(Span::raw("  "));
        }
        if tab.layers.show_ai_findings {
            right.push(Span::styled(
                " AI ON ",
                ratatui::style::Style::default()
                    .fg(styles::BG())
                    .bg(styles::ORANGE())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ));
        } else {
            right.push(Span::styled(
                " AI OFF ",
                ratatui::style::Style::default()
                    .fg(styles::MUTED())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ));
        }
        right.push(Span::raw("  "));
    }
    if app.config.ai_hub.has_presets() {
        right.push(Span::styled(
            format!(" {} ", app.active_ai_selection_label()),
            ratatui::style::Style::default()
                .fg(styles::BG())
                .bg(styles::PURPLE())
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
        right.push(Span::raw("  "));
    }
    if let Some(panel) = tab.panel {
        let panel_label = match panel {
            PanelContent::FileDetail => " File Detail ",
            PanelContent::AiSummary => " AI Summary ",
            PanelContent::PrOverview => " PR Overview ",
            PanelContent::SymbolRefs => " Symbol Refs ",
            PanelContent::AgentLog => " Agent Log ",
        };
        let panel_style = if tab.panel_focus {
            ratatui::style::Style::default()
                .fg(styles::BG())
                .bg(styles::BLUE())
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            ratatui::style::Style::default()
                .fg(styles::BLUE())
                .add_modifier(ratatui::style::Modifier::BOLD)
        };
        right.push(Span::styled(panel_label, panel_style));
        right.push(Span::raw("  "));
    }

    // Show conflict status badge when in Conflicts mode
    if tab.mode == DiffMode::Conflicts {
        let total = tab.files.len();
        let unresolved = tab.unresolved_count;
        if unresolved > 0 {
            // Unresolved conflicts: show unresolved count highlighted in orange
            let unresolved_label = format!(" {} unresolved ", unresolved);
            right.push(Span::styled(
                unresolved_label,
                ratatui::style::Style::default()
                    .fg(styles::BG())
                    .bg(styles::ORANGE())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ));
            // Show total merge file count in dimmer style
            if total > unresolved {
                right.push(Span::styled(
                    format!(" / {} ", total),
                    ratatui::style::Style::default().fg(styles::MUTED()),
                ));
            }
        } else {
            // All conflicts resolved: green MERGE badge
            right.push(Span::styled(
                " MERGE ",
                ratatui::style::Style::default()
                    .fg(styles::BG())
                    .bg(styles::GREEN())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ));
            if total > 0 {
                right.push(Span::styled(
                    format!(" {} files ", total),
                    ratatui::style::Style::default().fg(styles::MUTED()),
                ));
            }
        }
        right.push(Span::raw("  "));
    }

    // Show filtered reviewed count (yellow) then total reviewed count (blue)
    if let Some((f_reviewed, f_total)) = tab.filtered_reviewed_count() {
        right.push(Span::styled(
            format!("{}/{}", f_reviewed, f_total),
            ratatui::style::Style::default().fg(styles::YELLOW()),
        ));
        right.push(Span::styled(
            " · ",
            ratatui::style::Style::default().fg(styles::MUTED()),
        ));
    }
    let (reviewed, total) = tab.reviewed_count();
    if total > 0 {
        right.push(Span::styled(
            format!("{}/{} reviewed", reviewed, total),
            ratatui::style::Style::default().fg(styles::BLUE()),
        ));
    }
    if app.watching {
        if !right.is_empty() {
            right.push(Span::raw("  "));
        }
        right.push(Span::styled(
            "\u{25cf} WATCHING",
            ratatui::style::Style::default()
                .fg(styles::GREEN())
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
            ratatui::style::Style::default().fg(styles::DIM()),
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
        Self {
            key: key.to_string(),
            label: label.to_string(),
        }
    }
    const fn width(&self) -> usize {
        self.key.len() + self.label.len()
    }
}

/// Build the hint list for when the AI Summary panel is focused
fn build_ai_panel_hints(app: &App) -> Vec<Hint> {
    let tab = app.tab();
    let mut hints = vec![
        Hint::new("j/k", " navigate "),
        Hint::new("Tab", " focus "),
        Hint::new("␣", " toggle "),
        Hint::new("Enter", " jump "),
    ];

    // Show which column is focused
    let focus_label = match tab.review_focus {
        er_engine::ai::ReviewFocus::Files => " [Files] ",
        er_engine::ai::ReviewFocus::Checklist => " [Checklist] ",
    };
    hints.push(Hint {
        key: String::new(),
        label: focus_label.to_string(),
    });

    hints
}

/// Build hints for History mode
fn build_tour_hints(app: &App) -> Vec<Hint> {
    let h = &app.config.hints;
    let mut hints = Vec::new();
    if h.navigation {
        hints.push(Hint::new("j/k", " pillars "));
        hints.push(Hint::new("n/N", " files "));
        hints.push(Hint::new("u/d", " scroll "));
        hints.push(Hint::new("↑↓", " lines "));
    }
    hints.push(Hint::new("␣", " reviewed "));
    hints.push(Hint::new("b", " review pillar "));
    hints.push(Hint::new("g", " git "));
    hints
}

fn build_history_hints(app: &App) -> Vec<Hint> {
    let tab = app.tab();
    let h = &app.config.hints;
    let mut hints = Vec::new();

    // Navigation
    if h.navigation {
        hints.push(Hint::new("j/k", " commits "));
        hints.push(Hint::new("n/N", " files "));
        hints.push(Hint::new("↑↓", " lines "));
        hints.push(Hint::new("/", " search "));
    }

    // Context-sensitive: focused comment actions
    if h.comments {
        if let Some(ref fid) = tab.focused_comment_id {
            if let Some(comment) = tab.ai.find_comment(fid) {
                if comment.can_reply() {
                    hints.push(Hint::new("r", " reply "));
                }
                if comment.can_delete() {
                    hints.push(Hint::new("x", " delete "));
                }
            }
        } else if tab.focused_finding_id.is_some() {
            hints.push(Hint::new("r", " reply "));
        }
    }

    // Hub triggers
    hints.push(Hint::new("g", " git "));
    hints.push(Hint::new("a", " ai "));
    hints.push(Hint::new("?", " help "));
    hints.push(Hint::new("^q", " quit "));

    // Verbose hints for history mode
    if h.verbose {
        hints.push(Hint::new("e", " edit "));
        hints.push(Hint::new("p", " panel "));
        hints.push(Hint::new("f", " filter "));
    }

    // Show current file in commit if navigating
    if let Some(ref history) = tab.history {
        if !history.commit_files.is_empty() {
            let file_name = history
                .commit_files
                .get(history.selected_file)
                .map(|f| f.path.rsplit('/').next().unwrap_or(&f.path))
                .unwrap_or("");
            if !file_name.is_empty() {
                hints.push(Hint {
                    key: String::new(),
                    label: format!(" {} ", file_name),
                });
            }
        }
    }

    // Status indicators
    if !tab.filter_expr.is_empty() {
        hints.push(Hint {
            key: "F:".to_string(),
            label: format!(" {} ", tab.filter_expr),
        });
    }

    if !tab.search_query.is_empty() {
        hints.push(Hint {
            key: String::new(),
            label: format!(" search: \"{}\" ", tab.search_query),
        });
    }

    hints
}

/// Build the normal-mode hint list
fn build_hints(app: &App) -> Vec<Hint> {
    let tab = app.tab();
    let h = &app.config.hints;

    // Delegate to AI panel hints when focus is on the AI Summary panel
    if tab.panel_focus && tab.panel == Some(PanelContent::AiSummary) {
        return build_ai_panel_hints(app);
    }

    // History mode has different hints
    if tab.mode == DiffMode::History {
        return build_history_hints(app);
    }

    // Tour mode has its own hints
    if tab.mode == DiffMode::Tour {
        return build_tour_hints(app);
    }

    // Conflicts mode uses same hint structure as normal mode (staging not applicable)
    // — fall through to normal hint building below

    let mut hints: Vec<Hint> = Vec::new();

    if app.has_comment_draft() {
        hints.insert(0, Hint::new("Tab", " resume draft "));
    }

    if tab.panel.is_some() {
        // Context: panel open — show panel controls
        if h.navigation {
            hints.push(Hint::new("j/k", " nav "));
            hints.push(Hint::new("n/N", " hunks "));
        }
        if tab.panel_focus {
            hints.push(Hint::new("Esc", " unfocus "));
        } else {
            hints.push(Hint::new("Tab", " focus panel "));
        }
        hints.push(Hint::new("p", " close panel "));

        // Context-sensitive: focused comment actions (panel open)
        if h.comments {
            if let Some(ref fid) = tab.focused_comment_id {
                if let Some(comment) = tab.ai.find_comment(fid) {
                    if comment.can_reply() {
                        hints.push(Hint::new("r", " reply "));
                    }
                    if comment.can_delete() {
                        hints.push(Hint::new("x", " delete "));
                    }
                }
            } else if tab.focused_finding_id.is_some() {
                hints.push(Hint::new("r", " reply "));
            }
        }

        // Hub triggers — always shown
        hints.push(Hint::new("g", " git "));
        hints.push(Hint::new("a", " ai "));
        hints.push(Hint::new("o", " open "));
        hints.push(Hint::new("v", " verify "));
        hints.push(Hint::new("?", " help "));
        hints.push(Hint::new("^q", " quit "));

        // Verbose hints for panel-open mode
        if h.verbose {
            if tab.mode != DiffMode::Staged {
                hints.push(Hint::new("q", " question "));
                hints.push(Hint::new("c", " comment "));
            }
            hints.push(Hint::new("f", " filter "));
            hints.push(Hint::new("!", " unreviewed "));
            hints.push(Hint::new("</>", " tree w "));
            hints.push(Hint::new("{/}", " panel w "));
        }
    } else {
        // Default normal mode — essential navigation only
        if h.navigation {
            hints.push(Hint::new("j/k", " nav "));
            hints.push(Hint::new("n/N", " hunks "));
            hints.push(Hint::new("+/-", " context "));
            hints.push(Hint::new("␣", " review "));
            hints.push(Hint::new("/", " search "));
        }

        // Staging: the most common workflow keys stay visible
        if h.staging && (tab.mode == DiffMode::Unstaged || tab.mode == DiffMode::Staged) {
            hints.push(Hint::new("s", " stage "));
        }
        if h.staging && tab.mode == DiffMode::Staged {
            hints.push(Hint::new("c", " commit "));
        }

        // Context-sensitive: focused comment actions
        if h.comments {
            if let Some(ref fid) = tab.focused_comment_id {
                if let Some(comment) = tab.ai.find_comment(fid) {
                    if comment.can_reply() {
                        hints.push(Hint::new("r", " reply "));
                    }
                    if comment.can_delete() {
                        hints.push(Hint::new("x", " delete "));
                    }
                }
            } else if tab.focused_finding_id.is_some() {
                hints.push(Hint::new("r", " reply "));
            }
        }

        // Hub triggers — always shown
        hints.push(Hint::new("g", " git "));
        hints.push(Hint::new("a", " ai "));
        hints.push(Hint::new("o", " open "));
        hints.push(Hint::new("v", " verify "));
        hints.push(Hint::new("?", " help "));
        hints.push(Hint::new("^q", " quit "));

        // Verbose hints — additional keybinds beyond hub triggers
        if h.verbose {
            if tab.mode != DiffMode::Staged {
                hints.push(Hint::new("q", " question "));
                hints.push(Hint::new("c", " comment "));
            }
            hints.push(Hint::new("e", " edit "));
            hints.push(Hint::new("p", " panel "));
            hints.push(Hint::new("f", " filter "));
            hints.push(Hint::new("!", " unreviewed "));
            hints.push(Hint::new("U", " next unreviewed "));
            hints.push(Hint::new("m", " recent "));
            if tab.ai.has_data() {
                hints.push(Hint::new("A", " AI toggle "));
            }
            // Show comment navigation hints when current file has comments
            if let Some(file) = tab.files.get(tab.selected_file) {
                let has_comments = tab.ai.file_question_count(&file.path) > 0
                    || tab.ai.file_note_count(&file.path) > 0
                    || tab.ai.file_github_comment_count(&file.path) > 0;
                if has_comments {
                    hints.push(Hint::new("J/K", " comments "));
                }
            }
            if tab.ai.has_questions() || tab.ai.has_notes() {
                hints.push(Hint::new("z", " clear questions & notes "));
            }
            if tab.ai.has_data() {
                hints.push(Hint::new("Z", " clear reviews "));
            }
            hints.push(Hint::new(",", " settings "));
        }
    }

    // Status indicators always shown
    if !tab.filter_expr.is_empty() {
        hints.push(Hint {
            key: "F:".to_string(),
            label: format!(" {} ", tab.filter_expr),
        });
    }
    if !tab.search_query.is_empty() {
        hints.push(Hint {
            key: String::new(),
            label: format!(" search: \"{}\" ", tab.search_query),
        });
    }
    if tab.show_unreviewed_only {
        hints.push(Hint {
            key: String::new(),
            label: " [unreviewed] ".to_string(),
        });
    }
    if tab.show_watched && !tab.watched_files.is_empty() {
        hints.push(Hint {
            key: String::new(),
            label: format!(" [watched: {}] ", tab.watched_files.len()),
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
            current_spans.push(Span::styled(hint.key.clone(), styles::key_hint_style()));
        }
        current_spans.push(Span::styled(
            hint.label.clone(),
            if hint.key.is_empty() {
                // Indicator style (search, unreviewed)
                if hint.label.contains("search") {
                    ratatui::style::Style::default().fg(styles::YELLOW())
                } else {
                    ratatui::style::Style::default().fg(styles::PURPLE())
                }
            } else if hint.key == "F:" {
                // Filter expression indicator — yellow accent
                ratatui::style::Style::default().fg(styles::YELLOW())
            } else {
                ratatui::style::Style::default().fg(styles::DIM())
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
    match &app.input_mode {
        InputMode::Comment => 5,
        InputMode::Search
        | InputMode::Confirm(_)
        | InputMode::Filter
        | InputMode::Commit
        | InputMode::RemoteUrl => 1,
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
    let panel_bg = ratatui::style::Style::default().bg(styles::PANEL());

    match &app.input_mode {
        InputMode::Confirm(action) => {
            let prompt = match action {
                ConfirmAction::DeleteComment { .. } => "Delete comment? (y/n)".to_string(),
                ConfirmAction::DeleteWatchedFile { ref path } => {
                    format!("Delete {}? (y/n)", path)
                }
                ConfirmAction::Push => "Push branch to remote? (y/n)".to_string(),
                ConfirmAction::CleanupQuestions { count } => {
                    format!("Clear {} item(s) (questions & notes)? (y/n)", count)
                }
                ConfirmAction::CleanupReviews { count } => {
                    format!("Clear {} review file(s)? (y/n)", count)
                }
                ConfirmAction::RunAgentReview { .. } => {
                    "Clear previous review before running? (y=clear k=keep Esc=cancel)".to_string()
                }
                ConfirmAction::RunAgentQuestions { .. } => {
                    "Clear previous AI answers before running? (y=clear k=keep Esc=cancel)"
                        .to_string()
                }
                ConfirmAction::RunAgentNotes { .. } => {
                    "Clear previous note replies before running? (y=clear k=keep Esc=cancel)"
                        .to_string()
                }
                ConfirmAction::ApprovePR => "Approve this PR on GitHub? (y/n)".to_string(),
                ConfirmAction::PushComments => {
                    "Push as: (r) Review  (i) Individual  (Esc) Cancel".to_string()
                }
            };
            let spans = vec![
                Span::styled(
                    " ⚠ ",
                    ratatui::style::Style::default()
                        .fg(styles::BG())
                        .bg(styles::YELLOW())
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {} ", prompt),
                    ratatui::style::Style::default().fg(styles::YELLOW()),
                ),
            ];
            let bar = Paragraph::new(Line::from(spans)).style(panel_bg);
            f.render_widget(bar, area);
        }
        InputMode::Comment => {
            let is_question = tab.comment_type == er_engine::ai::CommentType::Question;
            let is_note = tab.comment_type == er_engine::ai::CommentType::Note;
            // Questions and notes share the yellow accent; GitHub comments are cyan.
            let yellow_kind = is_question || is_note;
            let is_reply = tab.comment_reply_to.is_some();
            let is_finding_reply = tab.comment_finding_ref.is_some();
            let (label, accent) = if is_reply {
                (
                    "reply",
                    if yellow_kind {
                        styles::YELLOW()
                    } else {
                        styles::CYAN()
                    },
                )
            } else if is_finding_reply {
                ("response", styles::CYAN())
            } else if is_note {
                ("note", styles::YELLOW())
            } else if is_question {
                ("question", styles::YELLOW())
            } else {
                ("comment", styles::CYAN())
            };
            let file_short = tab
                .comment_file
                .rsplit('/')
                .next()
                .unwrap_or(&tab.comment_file);
            let target_label = if let Some(ln) = tab.comment_line_num {
                format!("{}:L{}", file_short, ln)
            } else {
                format!("{}:h{}", file_short, tab.comment_hunk + 1)
            };

            // Split: 1 row header, rest textarea
            let rows = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Length(1),
                    ratatui::layout::Constraint::Min(1),
                ])
                .split(area);

            // Header row
            let dim = ratatui::style::Style::default().fg(styles::DIM());
            let mut header_spans = vec![
                Span::styled(
                    format!(" {} ", label),
                    ratatui::style::Style::default()
                        .fg(styles::BG())
                        .bg(accent)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {} ", target_label),
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
                Span::styled("  ", ratatui::style::Style::default()),
                Span::styled("Enter", styles::key_hint_style()),
                Span::styled(" send  ", dim),
                Span::styled("S+Enter", styles::key_hint_style()),
                Span::styled(" newline  ", dim),
                Span::styled("Tab", styles::key_hint_style()),
                Span::styled(" pause  ", dim),
            ];
            if app.can_toggle_comment_type() {
                header_spans.push(Span::styled("Ctrl+t", styles::key_hint_style()));
                header_spans.push(Span::styled(" type  ", dim));
            }
            header_spans.push(Span::styled("Esc", styles::key_hint_style()));
            header_spans.push(Span::styled(" cancel", dim));
            let header = Paragraph::new(Line::from(header_spans)).style(panel_bg);
            f.render_widget(header, rows[0]);

            // Textarea
            f.render_widget(&tab.comment_textarea, rows[1]);
        }
        InputMode::Filter => {
            let spans = vec![
                Span::styled(
                    " filter ",
                    ratatui::style::Style::default()
                        .fg(styles::BG())
                        .bg(styles::YELLOW())
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}", tab.filter_input),
                    ratatui::style::Style::default().fg(styles::TEXT()),
                ),
                Span::styled("█", ratatui::style::Style::default().fg(styles::YELLOW())),
                Span::styled("  ", ratatui::style::Style::default()),
                Span::styled("Enter", styles::key_hint_style()),
                Span::styled(
                    " apply  ",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
                Span::styled("Esc", styles::key_hint_style()),
                Span::styled(
                    " cancel",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
            ];
            let bar = Paragraph::new(Line::from(spans)).style(panel_bg);
            f.render_widget(bar, area);
        }
        InputMode::RemoteUrl => {
            let spans = vec![
                Span::styled(
                    " remote ",
                    ratatui::style::Style::default()
                        .fg(styles::BG())
                        .bg(styles::CYAN())
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}", app.remote_url_input),
                    ratatui::style::Style::default().fg(styles::TEXT()),
                ),
                Span::styled("█", ratatui::style::Style::default().fg(styles::CYAN())),
                Span::styled("  ", ratatui::style::Style::default()),
                Span::styled("Enter", styles::key_hint_style()),
                Span::styled(
                    " open  ",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
                Span::styled("Esc", styles::key_hint_style()),
                Span::styled(
                    " cancel",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
            ];
            let bar = Paragraph::new(Line::from(spans)).style(panel_bg);
            f.render_widget(bar, area);
        }
        InputMode::Search => {
            let spans = vec![
                Span::styled(" /", styles::key_hint_style()),
                Span::styled(
                    format!(" {}", tab.search_query),
                    ratatui::style::Style::default().fg(styles::TEXT()),
                ),
                Span::styled("█", ratatui::style::Style::default().fg(styles::BLUE())),
                Span::styled("  ", ratatui::style::Style::default()),
                Span::styled("Enter", styles::key_hint_style()),
                Span::styled(
                    " confirm  ",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
                Span::styled("Esc", styles::key_hint_style()),
                Span::styled(
                    " cancel",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
            ];
            let bar = Paragraph::new(Line::from(spans)).style(panel_bg);
            f.render_widget(bar, area);
        }
        InputMode::Commit => {
            let spans = vec![
                Span::styled(
                    " commit ",
                    ratatui::style::Style::default()
                        .fg(styles::BG())
                        .bg(styles::GREEN())
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}", tab.commit_input),
                    ratatui::style::Style::default().fg(styles::TEXT()),
                ),
                Span::styled("█", ratatui::style::Style::default().fg(styles::GREEN())),
                Span::styled("  ", ratatui::style::Style::default()),
                Span::styled("Enter", styles::key_hint_style()),
                Span::styled(
                    " commit  ",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
                Span::styled("Esc", styles::key_hint_style()),
                Span::styled(
                    " cancel",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ),
            ];
            let bar = Paragraph::new(Line::from(spans)).style(panel_bg);
            f.render_widget(bar, area);
        }
        InputMode::Normal => {
            let hints = build_hints(app);
            let lines = pack_hint_lines(&hints, area.width as usize);

            let row_count = lines.len() as u16;
            let constraints: Vec<ratatui::layout::Constraint> = (0..row_count)
                .map(|_| ratatui::layout::Constraint::Length(1))
                .collect();
            let rows = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints(constraints)
                .split(area);

            // rows has exactly row_count slots; the break below guards against
            // pack_hint_lines producing more lines than were budgeted at layout time.
            for (i, line) in lines.into_iter().enumerate() {
                if i >= rows.len() {
                    break;
                }
                let bar = Paragraph::new(line).style(panel_bg);
                f.render_widget(bar, rows[i]);
            }
        }
    }
}

/// Render watch notification overlay
pub fn render_watch_notification(f: &mut Frame, area: Rect, message: &str) {
    let char_count = message.chars().count().min(u16::MAX as usize - 4);
    let notif_width = char_count as u16 + 4;
    let notif_x = area.x + area.width.saturating_sub(notif_width + 2);
    let notif_y = area.y + 2;

    let notif_area = Rect {
        x: notif_x,
        y: notif_y,
        width: notif_width.min(area.width),
        height: 1,
    };

    let notif = Paragraph::new(Line::from(vec![
        Span::styled(" ● ", ratatui::style::Style::default().fg(styles::GREEN())),
        Span::styled(message, ratatui::style::Style::default().fg(styles::TEXT())),
        Span::raw(" "),
    ]))
    .style(
        ratatui::style::Style::default()
            .bg(styles::PANEL())
            .fg(styles::TEXT()),
    );

    f.render_widget(notif, notif_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── spans_width ──

    #[test]
    fn spans_width_correct_counting() {
        let spans = vec![Span::raw("hello"), Span::raw(" "), Span::raw("world")];
        assert_eq!(spans_width(&spans), 11);
    }

    #[test]
    fn spans_width_empty() {
        let spans: Vec<Span> = vec![];
        assert_eq!(spans_width(&spans), 0);
    }

    // ── pack_hint_lines ──

    #[test]
    fn pack_hint_lines_fits_in_one_line() {
        let hints = vec![Hint::new("j/k", " nav "), Hint::new("n/N", " hunks ")];
        let lines = pack_hint_lines(&hints, 80);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn pack_hint_lines_wraps_to_multiple_lines() {
        let hints = vec![
            Hint::new("j/k", " navigate "),
            Hint::new("n/N", " hunks "),
            Hint::new("^q", " quit "),
            Hint::new("f", " filter "),
            Hint::new("!", " unreviewed "),
            Hint::new(",", " settings "),
        ];
        // Total width of all hints > 20 chars, so they should wrap
        let lines = pack_hint_lines(&hints, 20);
        assert!(lines.len() > 1);
    }

    #[test]
    fn pack_hint_lines_empty_returns_one_line() {
        let hints: Vec<Hint> = vec![];
        let lines = pack_hint_lines(&hints, 80);
        assert_eq!(lines.len(), 1);
    }

    // ── Hint::width ──

    #[test]
    fn hint_width_includes_key_and_label() {
        let hint = Hint::new("j/k", " nav ");
        assert_eq!(hint.width(), 8); // "j/k" (3) + " nav " (5)
    }

    // ── shared fixtures ──

    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn hint_keys(hints: &[Hint]) -> Vec<&str> {
        hints.iter().map(|h| h.key.as_str()).collect()
    }

    fn hint_labels(hints: &[Hint]) -> Vec<&str> {
        hints.iter().map(|h| h.label.as_str()).collect()
    }

    fn history_app() -> App {
        let mut app = App::new_for_test(vec![]);
        app.tab_mut().mode = DiffMode::History;
        app
    }

    fn gh_comment(
        id: &str,
        source: &str,
        author: &str,
        in_reply_to: Option<String>,
    ) -> er_engine::ai::GitHubReviewComment {
        er_engine::ai::GitHubReviewComment {
            id: id.to_string(),
            timestamp: String::new(),
            file: "src/lib.rs".to_string(),
            hunk_index: Some(0),
            line_start: Some(10),
            line_end: None,
            line_content: String::new(),
            comment: "a remark".to_string(),
            in_reply_to,
            resolved: false,
            source: source.to_string(),
            github_id: None,
            author: author.to_string(),
            synced: false,
            outdated: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".to_string(),
        }
    }

    fn app_focused_on_comment(comment: er_engine::ai::GitHubReviewComment) -> App {
        let mut app = history_app();
        let id = comment.id.clone();
        app.tab_mut().ai.github_comments = Some(er_engine::ai::ErGitHubComments {
            version: 1,
            diff_hash: "h".to_string(),
            github: None,
            comments: vec![comment],
        });
        app.tab_mut().focused_comment_id = Some(id);
        app
    }

    // ── build_history_hints ──

    #[test]
    fn build_history_hints_leads_with_commit_navigation_when_navigation_is_enabled() {
        let app = history_app();
        let hints = build_history_hints(&app);

        let keys = hint_keys(&hints);
        assert_eq!(&keys[..4], &["j/k", "n/N", "↑↓", "/"][..]);
        assert_eq!(
            hint_labels(&hints)[0],
            " commits ",
            "History mode navigates commits, not files, with j/k"
        );
    }

    #[test]
    fn build_history_hints_drops_navigation_but_keeps_hub_triggers_when_disabled() {
        let mut app = history_app();
        app.config.hints.navigation = false;
        let hints = build_history_hints(&app);

        let keys = hint_keys(&hints);
        assert!(!keys.contains(&"j/k"), "got: {keys:?}");
        assert_eq!(
            &keys[..],
            &["g", "a", "?", "^q"][..],
            "the hub triggers are unconditional"
        );
    }

    #[test]
    fn build_history_hints_offers_reply_and_delete_for_a_focused_local_comment() {
        let app = app_focused_on_comment(gh_comment("gh-1", "local", "You", None));
        let binding = build_history_hints(&app);
        let keys = hint_keys(&binding);

        assert!(keys.contains(&"r"), "a top-level comment can be replied to");
        assert!(keys.contains(&"x"), "a local comment can be deleted");
    }

    #[test]
    fn build_history_hints_hides_delete_for_someone_elses_github_comment() {
        let app = app_focused_on_comment(gh_comment("gh-2", "github", "martin-kr", None));
        let binding = build_history_hints(&app);
        let keys = hint_keys(&binding);

        assert!(keys.contains(&"r"), "you can still reply");
        assert!(
            !keys.contains(&"x"),
            "another author's GitHub comment is not deletable, got: {keys:?}"
        );
    }

    #[test]
    fn build_history_hints_hides_reply_for_a_focused_reply() {
        let app =
            app_focused_on_comment(gh_comment("gh-3", "local", "You", Some("gh-1".to_string())));
        let binding = build_history_hints(&app);
        let keys = hint_keys(&binding);

        assert!(
            !keys.contains(&"r"),
            "threads are single level, so a reply cannot be replied to, got: {keys:?}"
        );
        assert!(keys.contains(&"x"), "your own reply is still deletable");
    }

    #[test]
    fn build_history_hints_offers_reply_for_a_focused_finding() {
        let mut app = history_app();
        app.tab_mut().focused_finding_id = Some("prof-1".to_string());
        let binding = build_history_hints(&app);
        let keys = hint_keys(&binding);

        assert!(
            keys.contains(&"r"),
            "a focused AI finding can be responded to, got: {keys:?}"
        );
    }

    #[test]
    fn build_history_hints_suppresses_comment_actions_when_comment_hints_are_off() {
        let mut app = history_app();
        app.config.hints.comments = false;
        app.tab_mut().focused_finding_id = Some("prof-1".to_string());
        let binding = build_history_hints(&app);
        let keys = hint_keys(&binding);

        assert!(!keys.contains(&"r"), "got: {keys:?}");
    }

    #[test]
    fn build_history_hints_adds_secondary_keys_only_in_verbose_mode() {
        let mut app = history_app();
        assert!(!hint_keys(&build_history_hints(&app)).contains(&"e"));

        app.config.hints.verbose = true;
        let binding = build_history_hints(&app);
        let keys = hint_keys(&binding);
        assert!(keys.contains(&"e"), "got: {keys:?}");
        assert!(keys.contains(&"p"), "got: {keys:?}");
        assert!(keys.contains(&"f"), "got: {keys:?}");
    }

    #[test]
    fn build_history_hints_surfaces_the_active_filter_expression() {
        let mut app = history_app();
        app.tab_mut().filter_expr = "*.rs".to_string();
        let hints = build_history_hints(&app);

        let filter = hints
            .iter()
            .find(|h| h.key == "F:")
            .expect("filter indicator missing");
        assert_eq!(filter.label, " *.rs ");
    }

    #[test]
    fn build_history_hints_surfaces_the_active_search_query() {
        let mut app = history_app();
        app.tab_mut().search_query = "todo".to_string();
        let hints = build_history_hints(&app);

        assert!(
            hint_labels(&hints)
                .iter()
                .any(|l| l.contains("search: \"todo\"")),
            "got: {:?}",
            hint_labels(&hints)
        );
    }

    #[test]
    fn build_history_hints_omits_status_indicators_when_filter_and_search_are_clear() {
        let app = history_app();
        let hints = build_history_hints(&app);

        assert!(hints.iter().all(|h| h.key != "F:"));
        assert!(hint_labels(&hints).iter().all(|l| !l.contains("search:")));
    }

    // ── render_bottom_bar ──

    fn buffer_text(backend: &TestBackend) -> String {
        let buf = backend.buffer();
        let area = *buf.area();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn draw_bottom_bar(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let area = Rect::new(0, 0, width, height);
        terminal.draw(|f| render_bottom_bar(f, area, app)).unwrap();
        buffer_text(terminal.backend())
    }

    #[test]
    fn render_bottom_bar_confirm_shows_an_action_specific_prompt() {
        let cases: Vec<(ConfirmAction, &str)> = vec![
            (
                ConfirmAction::DeleteComment {
                    comment_id: "gh-1".to_string(),
                },
                "Delete comment? (y/n)",
            ),
            (
                ConfirmAction::DeleteWatchedFile {
                    path: "notes.md".to_string(),
                },
                "Delete notes.md? (y/n)",
            ),
            (ConfirmAction::Push, "Push branch to remote? (y/n)"),
            (
                ConfirmAction::CleanupQuestions { count: 3 },
                "Clear 3 item(s) (questions & notes)? (y/n)",
            ),
            (
                ConfirmAction::CleanupReviews { count: 2 },
                "Clear 2 review file(s)? (y/n)",
            ),
            (
                ConfirmAction::RunAgentReview {
                    clear_previous: true,
                },
                "Clear previous review before running?",
            ),
            (
                ConfirmAction::RunAgentQuestions {
                    clear_previous: false,
                },
                "Clear previous AI answers before running?",
            ),
            (
                ConfirmAction::RunAgentNotes {
                    clear_previous: false,
                },
                "Clear previous note replies before running?",
            ),
            (ConfirmAction::ApprovePR, "Approve this PR on GitHub? (y/n)"),
            (
                ConfirmAction::PushComments,
                "Push as: (r) Review  (i) Individual  (Esc) Cancel",
            ),
        ];

        for (action, expected) in cases {
            let mut app = App::new_for_test(vec![]);
            app.input_mode = InputMode::Confirm(action.clone());
            let text = draw_bottom_bar(&app, 140, 1);
            assert!(
                text.contains(expected),
                "{action:?} must prompt with {expected:?}, got: {text:?}"
            );
        }
    }

    #[test]
    fn render_bottom_bar_confirm_prefixes_the_prompt_with_a_warning_badge() {
        let mut app = App::new_for_test(vec![]);
        app.input_mode = InputMode::Confirm(ConfirmAction::Push);
        let text = draw_bottom_bar(&app, 140, 1);

        assert!(text.starts_with(" ⚠ "), "got: {text:?}");
    }

    #[test]
    fn render_bottom_bar_comment_mode_labels_the_draft_kind() {
        let cases = [
            (er_engine::ai::CommentType::GitHubComment, " comment "),
            (er_engine::ai::CommentType::Question, " question "),
            (er_engine::ai::CommentType::Note, " note "),
        ];
        for (kind, label) in cases {
            let mut app = App::new_for_test(vec![]);
            app.input_mode = InputMode::Comment;
            app.tab_mut().comment_type = kind;
            app.tab_mut().comment_file = "src/app/state/mod.rs".to_string();
            let text = draw_bottom_bar(&app, 160, 4);
            assert!(
                text.contains(label),
                "{kind:?} must render {label:?}, got: {text:?}"
            );
        }
    }

    #[test]
    fn render_bottom_bar_comment_mode_labels_replies_and_finding_responses() {
        let mut reply = App::new_for_test(vec![]);
        reply.input_mode = InputMode::Comment;
        reply.tab_mut().comment_file = "mod.rs".to_string();
        reply.tab_mut().comment_reply_to = Some("gh-1".to_string());
        assert!(
            draw_bottom_bar(&reply, 160, 4).contains(" reply "),
            "a draft with a parent is a reply"
        );

        let mut response = App::new_for_test(vec![]);
        response.input_mode = InputMode::Comment;
        response.tab_mut().comment_file = "mod.rs".to_string();
        response.tab_mut().comment_finding_ref = Some("prof-1".to_string());
        assert!(
            draw_bottom_bar(&response, 160, 4).contains(" response "),
            "a draft against a finding is a response"
        );
    }

    #[test]
    fn render_bottom_bar_reply_pill_inherits_the_accent_of_the_kind_it_replies_to() {
        fn pill_bg(comment_type: er_engine::ai::CommentType) -> ratatui::style::Color {
            let mut app = App::new_for_test(vec![]);
            app.input_mode = InputMode::Comment;
            app.tab_mut().comment_file = "mod.rs".to_string();
            app.tab_mut().comment_type = comment_type;
            app.tab_mut().comment_reply_to = Some("gh-1".to_string());

            let area = Rect::new(0, 0, 160, 4);
            let mut terminal = Terminal::new(TestBackend::new(160, 4)).unwrap();
            terminal.draw(|f| render_bottom_bar(f, area, &app)).unwrap();
            // The " reply " pill starts at column 0, so column 1 is its first letter.
            terminal.backend().buffer()[(1u16, 0u16)].bg
        }

        assert_eq!(
            pill_bg(er_engine::ai::CommentType::Question),
            styles::YELLOW(),
            "a reply to a question keeps the yellow question accent"
        );
        assert_eq!(
            pill_bg(er_engine::ai::CommentType::Note),
            styles::YELLOW(),
            "notes share the yellow accent with questions"
        );
        assert_eq!(
            pill_bg(er_engine::ai::CommentType::GitHubComment),
            styles::CYAN(),
            "a reply to a GitHub comment stays cyan"
        );
    }

    #[test]
    fn render_bottom_bar_comment_mode_targets_a_line_or_falls_back_to_the_hunk() {
        let mut on_line = App::new_for_test(vec![]);
        on_line.input_mode = InputMode::Comment;
        on_line.tab_mut().comment_file = "src/app/state/mod.rs".to_string();
        on_line.tab_mut().comment_line_num = Some(42);
        assert!(
            draw_bottom_bar(&on_line, 160, 4).contains("mod.rs:L42"),
            "a line-anchored draft names the line"
        );

        let mut on_hunk = App::new_for_test(vec![]);
        on_hunk.input_mode = InputMode::Comment;
        on_hunk.tab_mut().comment_file = "src/app/state/mod.rs".to_string();
        on_hunk.tab_mut().comment_line_num = None;
        on_hunk.tab_mut().comment_hunk = 2;
        assert!(
            draw_bottom_bar(&on_hunk, 160, 4).contains("mod.rs:h3"),
            "a hunk-anchored draft names the 1-based hunk"
        );
    }

    #[test]
    fn render_bottom_bar_comment_mode_offers_the_type_toggle_only_for_a_fresh_draft() {
        let mut fresh = App::new_for_test(vec![]);
        fresh.input_mode = InputMode::Comment;
        fresh.tab_mut().comment_file = "mod.rs".to_string();
        assert!(
            draw_bottom_bar(&fresh, 160, 4).contains("Ctrl+t"),
            "a new file-anchored draft can cycle question → note → comment"
        );

        let mut reply = App::new_for_test(vec![]);
        reply.input_mode = InputMode::Comment;
        reply.tab_mut().comment_file = "mod.rs".to_string();
        reply.tab_mut().comment_reply_to = Some("gh-1".to_string());
        assert!(
            !draw_bottom_bar(&reply, 160, 4).contains("Ctrl+t"),
            "a reply inherits its parent's kind and cannot be retyped"
        );
    }

    #[test]
    fn render_bottom_bar_filter_mode_echoes_the_filter_input() {
        let mut app = App::new_for_test(vec![]);
        app.input_mode = InputMode::Filter;
        app.tab_mut().filter_input = "*.rs".to_string();
        let text = draw_bottom_bar(&app, 100, 1);

        assert!(text.contains(" filter "), "got: {text:?}");
        assert!(
            text.contains("*.rs█"),
            "the caret trails the input, got: {text:?}"
        );
        assert!(text.contains("Enter apply"), "got: {text:?}");
    }

    #[test]
    fn render_bottom_bar_remote_url_mode_echoes_the_typed_url() {
        let mut app = App::new_for_test(vec![]);
        app.input_mode = InputMode::RemoteUrl;
        app.remote_url_input = "https://github.com/o/r/pull/9".to_string();
        let text = draw_bottom_bar(&app, 100, 1);

        assert!(text.contains(" remote "), "got: {text:?}");
        assert!(
            text.contains("https://github.com/o/r/pull/9"),
            "got: {text:?}"
        );
        assert!(text.contains("Enter open"), "got: {text:?}");
    }

    #[test]
    fn render_bottom_bar_search_mode_echoes_the_query() {
        let mut app = App::new_for_test(vec![]);
        app.input_mode = InputMode::Search;
        app.tab_mut().search_query = "todo".to_string();
        let text = draw_bottom_bar(&app, 100, 1);

        assert!(text.starts_with(" / todo█"), "got: {text:?}");
        assert!(text.contains("Enter confirm"), "got: {text:?}");
    }

    #[test]
    fn render_bottom_bar_commit_mode_echoes_the_commit_message() {
        let mut app = App::new_for_test(vec![]);
        app.input_mode = InputMode::Commit;
        app.tab_mut().commit_input = "fix: panel scroll".to_string();
        let text = draw_bottom_bar(&app, 100, 1);

        assert!(text.contains(" commit "), "got: {text:?}");
        assert!(text.contains("fix: panel scroll█"), "got: {text:?}");
    }

    #[test]
    fn render_bottom_bar_normal_mode_renders_the_packed_hints() {
        let app = App::new_for_test(vec![]);
        let text = draw_bottom_bar(&app, 120, 1);

        assert!(text.contains("j/k"), "got: {text:?}");
        assert!(text.contains("nav"), "got: {text:?}");
        assert!(text.contains("^q"), "got: {text:?}");
    }

    #[test]
    fn render_bottom_bar_normal_mode_spreads_wrapped_hints_over_several_rows_in_order() {
        let mut app = App::new_for_test(vec![]);
        app.tab_mut().panel = Some(PanelContent::FileDetail);
        // 20 columns cannot hold the panel hint list, so it packs onto several rows.
        let text = draw_bottom_bar(&app, 20, 8);

        let rows: Vec<&str> = text.lines().collect();
        let first_row = rows.iter().position(|r| r.contains("j/k"));
        let quit_row = rows.iter().position(|r| r.contains("^q"));
        assert_eq!(
            first_row,
            Some(0),
            "the first packed line lands on the first row, got: {text:?}"
        );
        assert!(
            quit_row.is_some_and(|q| q > 0),
            "hints that overflow the first row continue on later rows, got: {text:?}"
        );
    }
}
