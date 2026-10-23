use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use super::styles;
use er_engine::app::{DirEntry, HubAction, HubItem, HubKind, OverlayData, Worktree};

/// Render the active overlay on top of the main UI
/// Note: ConfigHub overlay is rendered separately in ui/mod.rs since it needs App access.
pub fn render_overlay(f: &mut Frame, area: Rect, overlay: &OverlayData) {
    match overlay {
        OverlayData::WorktreePicker {
            worktrees,
            selected,
        } => {
            render_worktree_picker(f, area, worktrees, *selected);
        }
        OverlayData::DirectoryBrowser {
            current_path,
            entries,
            selected,
        } => {
            render_directory_browser(f, area, current_path, entries, *selected);
        }
        OverlayData::ConfigHub { .. } => {
            // Handled in ui/mod.rs draw()
        }
        OverlayData::FilterHistory {
            history,
            selected,
            preset_count,
        } => {
            render_filter_history(f, area, history, *selected, *preset_count);
        }
        OverlayData::ModalHub {
            kind,
            title,
            items,
            selected,
        } => {
            render_modal_hub(f, area, *kind, title.as_deref(), items, *selected);
        }
        OverlayData::ExportPicker {
            include_comments,
            include_findings,
            include_questions,
            include_notes,
            selected,
        } => {
            render_export_picker(
                f,
                area,
                *include_comments,
                *include_findings,
                *include_questions,
                *include_notes,
                *selected,
            );
        }
        OverlayData::ReviewFlowResults {
            results,
            selected,
            selected_ids,
            detail_scroll,
        } => {
            let total = results.findings.len();
            let selected = (*selected).min(total.saturating_sub(1));
            let popup_height = area.height.saturating_sub(2).max(5);
            let inner_height = popup_height.saturating_sub(2);
            let list_height = inner_height.saturating_sub(7).clamp(3, 9) as usize;
            let visible = visible_result_range(total, selected, list_height.saturating_sub(2));
            let rows: Vec<ReviewFlowOverlayRow<'_>> = results.findings[visible.clone()]
                .iter()
                .map(|finding| ReviewFlowOverlayRow {
                    file: &finding.file,
                    line: finding.line,
                    title: &finding.title,
                    accepted: finding.accepted,
                    selectable: finding.selectable,
                    checked: selected_ids.contains(&finding.id),
                })
                .collect();
            let detail = results.findings.get(selected).map(|finding| {
                let verdict = match &finding.verdict {
                    er_engine::arena::Verdict::Kept => "Kept",
                    er_engine::arena::Verdict::Escalated => "Escalated",
                    _ => "Not eligible",
                };
                ReviewFlowOverlayDetail {
                    file: &finding.file,
                    line: finding.line,
                    title: &finding.title,
                    body: &finding.body,
                    confidence: finding.confidence,
                    raised_by: &finding.raised_by,
                    verdict,
                    accepted: finding.accepted,
                    selectable: finding.selectable,
                }
            });
            render_review_flow_results(
                f,
                area,
                ReviewFlowResultsView {
                    run_id: &results.run_id,
                    total,
                    first_visible: visible.start,
                    selected,
                    findings: &rows,
                    detail,
                    detail_scroll: *detail_scroll,
                },
            );
        }
    }
}

fn render_worktree_picker(f: &mut Frame, area: Rect, worktrees: &[Worktree], selected: usize) {
    let popup_height = (worktrees.len() as u16 + 2).min(area.height.saturating_sub(6));
    let popup_width = 70u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    // Clear backdrop
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = worktrees
        .iter()
        .enumerate()
        .map(|(idx, wt)| {
            let is_sel = idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                Span::styled(
                    format!("{:<20}", wt.branch),
                    if is_sel {
                        ratatui::style::Style::default().fg(styles::BRIGHT())
                    } else {
                        ratatui::style::Style::default().fg(styles::TEXT())
                    },
                ),
                Span::styled(&wt.path, ratatui::style::Style::default().fg(styles::DIM())),
            ]);

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(
            " WORKTREES (Enter=select, Esc=close) ",
            ratatui::style::Style::default().fg(styles::CYAN()),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

fn render_directory_browser(
    f: &mut Frame,
    area: Rect,
    current_path: &str,
    entries: &[DirEntry],
    selected: usize,
) {
    let popup_height = (entries.len() as u16 + 2)
        .min(area.height.saturating_sub(6))
        .max(5);
    let popup_width = 70u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    if entries.is_empty() {
        let block = Block::default()
            .title(Span::styled(
                format!(" {} ", current_path),
                ratatui::style::Style::default().fg(styles::CYAN()),
            ))
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
            .style(ratatui::style::Style::default().bg(styles::PANEL()));

        let empty = Paragraph::new(Line::from(Span::styled(
            "  (empty directory)",
            ratatui::style::Style::default().fg(styles::MUTED()),
        )))
        .block(block);

        f.render_widget(empty, popup);
        return;
    }

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let is_sel = idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let icon = if entry.is_git_repo || entry.is_dir {
                " "
            } else {
                "  "
            };

            let name_style = if entry.is_git_repo {
                ratatui::style::Style::default().fg(styles::GREEN())
            } else if entry.is_dir {
                ratatui::style::Style::default().fg(styles::BLUE())
            } else {
                ratatui::style::Style::default().fg(styles::TEXT())
            };

            let mut spans = vec![
                Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                Span::styled(icon, name_style),
                Span::styled(
                    &entry.name,
                    if is_sel {
                        ratatui::style::Style::default().fg(styles::BRIGHT())
                    } else {
                        name_style
                    },
                ),
            ];

            if entry.is_git_repo {
                spans.push(Span::styled(
                    "  [git]",
                    ratatui::style::Style::default().fg(styles::GREEN()),
                ));
            } else if entry.is_dir {
                spans.push(Span::styled(
                    "/",
                    ratatui::style::Style::default().fg(styles::DIM()),
                ));
            }

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    // Shorten path for title if too long, using char counts to avoid UTF-8 byte-boundary panics.
    let max_title_width = popup_width.saturating_sub(20) as usize;
    let title_path = {
        let char_count = current_path.chars().count();
        if char_count > max_title_width {
            let suffix: String = current_path
                .chars()
                .rev()
                .take(max_title_width)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("…{suffix}")
        } else {
            current_path.to_string()
        }
    };

    let block = Block::default()
        .title(Span::styled(
            format!(" {} (Enter=open, Bksp=up, Esc=close) ", title_path),
            ratatui::style::Style::default().fg(styles::CYAN()),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

fn render_filter_history(
    f: &mut Frame,
    area: Rect,
    history: &[String],
    selected: usize,
    preset_count: usize,
) {
    use er_engine::app::filter::FILTER_PRESETS;

    let separator_lines = if !history.is_empty() { 1 } else { 0 };
    let total_rows = preset_count + separator_lines + history.len();
    let popup_height = (total_rows as u16 + 2)
        .min(area.height.saturating_sub(6))
        .max(4);
    let popup_width = 60u16.min(area.width.saturating_sub(6));
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    let mut items: Vec<ListItem> = Vec::new();

    // Presets section
    for (idx, preset) in FILTER_PRESETS.iter().enumerate().take(preset_count) {
        let is_sel = idx == selected;
        let marker = if is_sel { "▶ " } else { "  " };

        let line = Line::from(vec![
            Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
            Span::styled(
                format!("{:<10}", preset.name),
                if is_sel {
                    ratatui::style::Style::default()
                        .fg(styles::BRIGHT())
                        .add_modifier(ratatui::style::Modifier::BOLD)
                } else {
                    ratatui::style::Style::default()
                        .fg(styles::BLUE())
                        .add_modifier(ratatui::style::Modifier::BOLD)
                },
            ),
            Span::styled(
                preset.expr,
                ratatui::style::Style::default().fg(styles::DIM()),
            ),
        ]);

        let style = if is_sel {
            styles::selected_style()
        } else {
            ratatui::style::Style::default().bg(styles::PANEL())
        };

        items.push(ListItem::new(line).style(style));
    }

    // Separator + history section
    if !history.is_empty() {
        items.push(
            ListItem::new(Line::from(Span::styled(
                "── history ──",
                ratatui::style::Style::default().fg(styles::MUTED()),
            )))
            .style(ratatui::style::Style::default().bg(styles::PANEL())),
        );

        for (idx, expr) in history.iter().enumerate() {
            let abs_idx = preset_count + idx;
            let is_sel = abs_idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(
                    marker,
                    ratatui::style::Style::default().fg(styles::YELLOW()),
                ),
                Span::styled(
                    expr.as_str(),
                    if is_sel {
                        ratatui::style::Style::default().fg(styles::BRIGHT())
                    } else {
                        ratatui::style::Style::default().fg(styles::TEXT())
                    },
                ),
            ]);

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            items.push(ListItem::new(line).style(style));
        }
    }

    let block = Block::default()
        .title(Span::styled(
            " FILTERS (Enter=apply, Esc=close) ",
            ratatui::style::Style::default().fg(styles::CYAN()),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

// Use the shared centered_rect from utils (deduplicated from overlay + settings)
use super::utils::{centered_rect, word_wrap};

fn render_modal_hub(
    f: &mut Frame,
    area: Rect,
    kind: HubKind,
    title_override: Option<&str>,
    items: &[HubItem],
    selected: usize,
) {
    // For Help hub, use wider popup to fit descriptions
    let is_help = kind == HubKind::Help;
    let popup_width = if is_help {
        70u16.min(area.width.saturating_sub(6))
    } else {
        55u16.min(area.width.saturating_sub(6))
    };
    let popup_height = (items.len() as u16 + 2)
        .min(area.height.saturating_sub(4))
        .max(5);
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    let title_color = match kind {
        HubKind::Git => styles::GREEN(),
        HubKind::Ai => styles::PURPLE(),
        HubKind::AiProvider | HubKind::AiModel | HubKind::AiEffort | HubKind::AiExpert => {
            styles::PURPLE()
        }
        HubKind::Verify | HubKind::VerifyPackage => styles::YELLOW(),
        HubKind::Help => styles::CYAN(),
        HubKind::Open => styles::BLUE(),
        HubKind::Copy => styles::CYAN(),
    };

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            if item.is_header {
                // Section header: rendered as dimmed label
                return ListItem::new(Line::from(Span::styled(
                    &item.label,
                    ratatui::style::Style::default()
                        .fg(title_color)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )))
                .style(ratatui::style::Style::default().bg(styles::PANEL()));
            }

            let is_sel = idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };

            let label_style = if !item.enabled {
                ratatui::style::Style::default().fg(styles::MUTED())
            } else if is_sel {
                ratatui::style::Style::default().fg(styles::BRIGHT())
            } else {
                ratatui::style::Style::default().fg(styles::TEXT())
            };

            let mut spans = vec![
                Span::styled(marker, ratatui::style::Style::default().fg(title_color)),
                Span::styled(&item.label, label_style),
            ];

            // For Help hub, show description inline after the label
            if is_help && !item.description.is_empty() {
                // Pad label to align descriptions
                let pad = 12usize.saturating_sub(item.label.len());
                spans.push(Span::raw(" ".repeat(pad)));
                spans.push(Span::styled(
                    &item.description,
                    ratatui::style::Style::default().fg(styles::DIM()),
                ));
            } else {
                // For action hubs, show hint right-aligned and description dimmed
                if !item.hint.is_empty() {
                    spans.push(Span::styled(
                        format!("  [{}]", item.hint),
                        ratatui::style::Style::default().fg(styles::DIM()),
                    ));
                }
                if !item.description.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", item.description),
                        ratatui::style::Style::default().fg(styles::MUTED()),
                    ));
                }
            }

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    // Rows with a secondary action advertise it (stacked PRs: `b` opens the PR
    // on GitHub, Enter opens it for review).
    let has_secondary = items
        .iter()
        .any(|item| item.enabled && matches!(item.action, HubAction::OpenStackPr { .. }));
    let close_hint = if is_help {
        "Esc=close"
    } else if has_secondary {
        "Enter=review, b=browser, Esc=close"
    } else {
        "Enter=select, Esc=close"
    };

    let display_title = title_override.unwrap_or_else(|| kind.title());
    let block = Block::default()
        .title(Span::styled(
            format!(" {} ({}) ", display_title, close_hint),
            ratatui::style::Style::default().fg(title_color),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(title_color))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

    let list = List::new(list_items).block(block);
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(list, popup, &mut state);
}

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "one flag per export category, passed straight from the overlay state"
)]
fn render_export_picker(
    f: &mut Frame,
    area: Rect,
    include_comments: bool,
    include_findings: bool,
    include_questions: bool,
    include_notes: bool,
    selected: usize,
) {
    let popup_width = 52u16.min(area.width.saturating_sub(6));
    let popup_height = 9u16.min(area.height.saturating_sub(4)).max(7);
    let popup = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup);

    let rows = [
        ("GitHub comments", include_comments),
        ("AI findings", include_findings),
        ("Questions", include_questions),
        ("Notes", include_notes),
    ];

    let list_items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(idx, (label, checked))| {
            let is_sel = idx == selected;
            let marker = if is_sel { "▶ " } else { "  " };
            let box_char = if *checked { "[x]" } else { "[ ]" };

            let label_style = if is_sel {
                ratatui::style::Style::default().fg(styles::BRIGHT())
            } else {
                ratatui::style::Style::default().fg(styles::TEXT())
            };

            let line = Line::from(vec![
                Span::styled(marker, ratatui::style::Style::default().fg(styles::CYAN())),
                Span::styled(
                    format!("{box_char} "),
                    ratatui::style::Style::default().fg(styles::YELLOW()),
                ),
                Span::styled(*label, label_style),
            ]);

            let style = if is_sel {
                styles::selected_style()
            } else {
                ratatui::style::Style::default().bg(styles::PANEL())
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(
            " EXPORT (Space=toggle, Enter=copy) ",
            ratatui::style::Style::default().fg(styles::CYAN()),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));

    let list = List::new(list_items).block(block);
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(list, popup, &mut state);
}

struct ReviewFlowOverlayRow<'a> {
    file: &'a str,
    line: Option<usize>,
    title: &'a str,
    accepted: bool,
    selectable: bool,
    checked: bool,
}

#[derive(Clone, Copy)]
struct ReviewFlowOverlayDetail<'a> {
    file: &'a str,
    line: Option<usize>,
    title: &'a str,
    body: &'a str,
    confidence: f32,
    raised_by: &'a [String],
    verdict: &'static str,
    accepted: bool,
    selectable: bool,
}

fn visible_result_range(total: usize, selected: usize, capacity: usize) -> std::ops::Range<usize> {
    let visible = total.min(capacity.max(1));
    let start = selected
        .min(total.saturating_sub(1))
        .saturating_sub(visible / 2)
        .min(total.saturating_sub(visible));
    start..start + visible
}

struct ReviewFlowResultsView<'a> {
    run_id: &'a str,
    total: usize,
    first_visible: usize,
    selected: usize,
    findings: &'a [ReviewFlowOverlayRow<'a>],
    detail: Option<ReviewFlowOverlayDetail<'a>>,
    detail_scroll: usize,
}

fn render_review_flow_results(f: &mut Frame, area: Rect, view: ReviewFlowResultsView<'_>) {
    let ReviewFlowResultsView {
        run_id,
        total,
        first_visible,
        selected,
        findings,
        detail,
        detail_scroll,
    } = view;
    let popup_width = 100u16.min(area.width.saturating_sub(4)).max(24);
    let popup_height = area.height.saturating_sub(2).max(5);
    let popup = centered_rect(popup_width, popup_height, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            format!(
                " REVIEW RESULTS · {} · {} of {} · Space select · a accept · PgUp/PgDn detail · Esc close ",
                run_id,
                if total == 0 { 0 } else { selected + 1 },
                total,
            ),
            ratatui::style::Style::default().fg(styles::CYAN()),
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::CYAN()))
        .style(ratatui::style::Style::default().bg(styles::PANEL()));
    let inner = block.inner(popup);
    f.render_widget(block, popup);
    let list_height = inner.height.saturating_sub(7).clamp(3, 9);
    let panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(list_height), Constraint::Min(0)])
        .split(inner);
    let items: Vec<ListItem> = if total == 0 {
        vec![ListItem::new(Line::from(Span::styled(
            "No Kept or Escalated findings in this run.",
            ratatui::style::Style::default().fg(styles::MUTED()),
        )))]
    } else {
        findings
            .iter()
            .enumerate()
            .map(|(index, finding)| {
                result_finding_title_item(finding, first_visible + index == selected)
            })
            .collect()
    };
    let title = Block::default()
        .title(format!(
            "Findings {}–{}",
            first_visible + 1,
            first_visible + items.len()
        ))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::DIM()));
    let list = List::new(items).block(title);
    let selected_local = selected.checked_sub(first_visible);
    let mut state = ratatui::widgets::ListState::default().with_selected(selected_local);
    f.render_stateful_widget(list, panes[0], &mut state);

    let detail_text = result_finding_detail(detail, panes[1].width.saturating_sub(4) as usize);
    let detail = Paragraph::new(Text::from(detail_text))
        .scroll((detail_scroll.min(u16::MAX as usize) as u16, 0))
        .block(
            Block::default()
                .title("Selected finding")
                .borders(Borders::ALL),
        );
    f.render_widget(detail, panes[1]);
}

fn result_finding_title_item(
    finding: &ReviewFlowOverlayRow<'_>,
    selected: bool,
) -> ListItem<'static> {
    let marker = if finding.accepted {
        "[accepted]"
    } else if !finding.selectable {
        "[view]"
    } else if finding.checked {
        "[x]"
    } else {
        "[ ]"
    };
    let location = finding
        .line
        .map(|line| format!("{}:{line}", finding.file))
        .unwrap_or_else(|| finding.file.to_owned());
    let line = Line::from(format!("{marker} {location} · {}", finding.title));
    let style = if selected {
        styles::selected_style()
    } else {
        ratatui::style::Style::default()
    };
    ListItem::new(line).style(style)
}

fn result_finding_detail(
    detail: Option<ReviewFlowOverlayDetail<'_>>,
    width: usize,
) -> Vec<Line<'static>> {
    let Some(detail) = detail else {
        return vec![Line::from("No finding selected.")];
    };
    let location = detail
        .line
        .map(|line| format!("{}:{line}", detail.file))
        .unwrap_or_else(|| detail.file.to_owned());
    let raisers = if detail.raised_by.is_empty() {
        "unknown".to_owned()
    } else {
        detail.raised_by.join(", ")
    };
    let acceptance = if detail.accepted {
        "[accepted]".to_owned()
    } else if detail.selectable {
        "[ ]".to_owned()
    } else {
        "[view]".to_owned()
    };
    let mut lines = vec![
        Line::from(location),
        Line::from(detail.title.to_owned()),
        Line::from(format!(
            "{} · {:.0}% confidence · raised by {}",
            detail.verdict,
            detail.confidence * 100.0,
            raisers
        )),
        Line::from(acceptance),
        Line::from(""),
    ];
    lines.extend(
        word_wrap(detail.body, width.max(1))
            .into_iter()
            .map(Line::from),
    );
    lines
}

#[cfg(test)]
mod review_flow_results_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn rendered_text(accepted: bool, selectable: bool) -> String {
        let row = ReviewFlowOverlayRow {
            file: "src/auth.rs",
            line: Some(42),
            title: "Unchecked token audience",
            accepted,
            selectable,
            checked: false,
        };
        let raised_by = vec!["security".to_owned(), "general".to_owned()];
        let detail = ReviewFlowOverlayDetail {
            file: "src/auth.rs",
            line: Some(42),
            title: "Unchecked token audience",
            body: "The token audience is read before it is validated.",
            confidence: 0.87,
            raised_by: &raised_by,
            verdict: "Kept",
            accepted,
            selectable,
        };
        let backend = TestBackend::new(110, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_review_flow_results(
                    frame,
                    frame.area(),
                    ReviewFlowResultsView {
                        run_id: "run-1",
                        total: 1,
                        first_visible: 0,
                        selected: 0,
                        findings: &[row],
                        detail: Some(detail),
                        detail_scroll: 0,
                    },
                );
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn results_overlay_shows_finding_details_and_acceptance_state() {
        let text = rendered_text(false, true);
        for expected in [
            "src/auth.rs:42",
            "Unchecked token audience",
            "The token audience is read before it is validated.",
            "87% confidence",
            "raised by security, general",
            "[ ]",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in {text:?}");
        }
        assert!(rendered_text(true, false).contains("[accepted]"));
    }

    #[test]
    fn viewport_keeps_selected_finding_visible_without_rendering_older_rows() {
        let selected = 50;
        let visible = visible_result_range(100, selected, 7);
        assert_eq!(visible, 47..54);
        let rows: Vec<_> = visible
            .clone()
            .map(|index| ReviewFlowOverlayRow {
                file: "src/file.rs",
                line: Some(index),
                title: if index == selected {
                    "SELECTED RESULT"
                } else {
                    "neighbor"
                },
                accepted: false,
                selectable: true,
                checked: false,
            })
            .collect();
        let detail = ReviewFlowOverlayDetail {
            file: "src/file.rs",
            line: Some(selected),
            title: "SELECTED RESULT",
            body: "Selected body only.",
            confidence: 0.9,
            raised_by: &[],
            verdict: "Kept",
            accepted: false,
            selectable: true,
        };
        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_review_flow_results(
                    frame,
                    frame.area(),
                    ReviewFlowResultsView {
                        run_id: "run-1",
                        total: 100,
                        first_visible: visible.start,
                        selected,
                        findings: &rows,
                        detail: Some(detail),
                        detail_scroll: 0,
                    },
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("SELECTED RESULT"));
        assert!(!text.contains("src/file.rs:0"));
    }

    #[test]
    fn detail_scroll_reaches_evidence_at_the_end_of_a_long_body() {
        let row = ReviewFlowOverlayRow {
            file: "src/file.rs",
            line: Some(1),
            title: "Long finding",
            accepted: false,
            selectable: true,
            checked: false,
        };
        let mut body = (0..40)
            .map(|line| format!("Evidence line {line}"))
            .collect::<Vec<_>>();
        body.push("TRAILING EVIDENCE SENTENCE".to_owned());
        let body = body.join("\n");
        let raised_by = vec!["security".to_owned()];
        let detail = ReviewFlowOverlayDetail {
            file: "src/file.rs",
            line: Some(1),
            title: "Long finding",
            body: &body,
            confidence: 0.9,
            raised_by: &raised_by,
            verdict: "Kept",
            accepted: false,
            selectable: true,
        };

        let render = |detail_scroll| {
            let backend = TestBackend::new(100, 28);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| {
                    render_review_flow_results(
                        frame,
                        frame.area(),
                        ReviewFlowResultsView {
                            run_id: "run-1",
                            total: 1,
                            first_visible: 0,
                            selected: 0,
                            findings: std::slice::from_ref(&row),
                            detail: Some(ReviewFlowOverlayDetail {
                                body: detail.body,
                                ..detail
                            }),
                            detail_scroll,
                        },
                    );
                })
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<Vec<_>>()
                .join("")
        };

        assert!(!render(0).contains("TRAILING EVIDENCE SENTENCE"));
        assert!(render(40).contains("TRAILING EVIDENCE SENTENCE"));
    }
}
