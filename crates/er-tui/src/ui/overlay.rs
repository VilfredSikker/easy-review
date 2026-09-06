use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use super::styles;
use er_engine::app::{DirEntry, HubItem, HubKind, OverlayData, Worktree};

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
use super::utils::centered_rect;

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

    let close_hint = if is_help {
        "Esc=close"
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

#[cfg(test)]
mod overlay_render_tests {
    use super::*;
    use er_engine::app::{App, HubAction};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    fn draw_overlay(overlay: &OverlayData, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|f| render_overlay(f, f.area(), overlay))
            .expect("draw overlay");
        terminal.backend().buffer().clone()
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn text(rows: &[String]) -> String {
        rows.join("\n")
    }

    fn row_containing<'a>(rows: &'a [String], needle: &str) -> &'a str {
        match rows.iter().find(|r| r.contains(needle)) {
            Some(row) => row.as_str(),
            None => panic!("no rendered row contains {needle:?}:\n{}", text(rows)),
        }
    }

    /// Column (not byte offset) at which `needle` starts in a rendered row.
    fn col_of(row: &str, needle: &str) -> usize {
        let byte = row
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} not in {row:?}"));
        row[..byte].chars().count()
    }

    fn entry(name: &str, is_dir: bool, is_git_repo: bool) -> DirEntry {
        DirEntry {
            name: name.to_string(),
            is_dir,
            is_git_repo,
        }
    }

    fn hub_item(label: &str, hint: &str, description: &str) -> HubItem {
        HubItem {
            label: label.to_string(),
            hint: hint.to_string(),
            description: description.to_string(),
            action: HubAction::Noop,
            is_header: false,
            enabled: true,
        }
    }

    // ── render_overlay dispatch ──

    #[test]
    fn overlay_routes_worktree_picker_to_the_worktree_list() {
        let overlay = OverlayData::WorktreePicker {
            worktrees: vec![
                Worktree {
                    path: "/repos/trunk".to_string(),
                    branch: "main".to_string(),
                },
                Worktree {
                    path: "/repos/topic".to_string(),
                    branch: "feature-x".to_string(),
                },
            ],
            selected: 1,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);

        assert!(text(&rows).contains("WORKTREES"), "{}", text(&rows));
        assert!(
            row_containing(&rows, "feature-x").contains("▶"),
            "the selected worktree carries the marker"
        );
        assert!(
            !row_containing(&rows, "/repos/trunk").contains("▶"),
            "unselected worktrees do not"
        );
    }

    /// The ConfigHub variant is deliberately a no-op here — `ui::draw` renders it
    /// through `settings::render_config_hub` because it needs `&App`. If this arm
    /// ever started drawing, the settings overlay would be painted twice.
    #[test]
    fn overlay_draws_nothing_for_the_config_hub_variant() {
        let mut app = App::new_for_test(vec![]);
        app.open_config_hub();
        let overlay = app.overlay.as_ref().expect("config hub overlay opened");

        let buf = draw_overlay(overlay, 60, 12);
        let rows = rows(&buf);

        assert!(
            rows.iter().all(|r| r.trim().is_empty()),
            "ConfigHub must be left to ui::draw, but render_overlay painted:\n{}",
            text(&rows)
        );
    }

    #[test]
    fn overlay_routes_export_picker_and_marks_the_checked_options() {
        let overlay = OverlayData::ExportPicker {
            include_comments: true,
            include_findings: false,
            include_questions: true,
            include_notes: false,
            selected: 2,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);

        assert!(text(&rows).contains("EXPORT"), "{}", text(&rows));
        assert!(row_containing(&rows, "GitHub comments").contains("[x]"));
        assert!(row_containing(&rows, "AI findings").contains("[ ]"));
        assert!(
            row_containing(&rows, "Questions").contains("▶"),
            "selected row 2 is Questions"
        );
    }

    // ── render_directory_browser ──

    #[test]
    fn directory_browser_shows_a_placeholder_for_an_empty_directory() {
        let overlay = OverlayData::DirectoryBrowser {
            current_path: "/home/user/empty".to_string(),
            entries: vec![],
            selected: 0,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);
        let out = text(&rows);

        assert!(out.contains("(empty directory)"), "{out}");
        assert!(
            out.contains("/home/user/empty"),
            "the empty-state title is the bare path, without key hints: {out}"
        );
        assert!(
            !out.contains("Enter=open"),
            "the empty state has nothing to open: {out}"
        );
    }

    #[test]
    fn directory_browser_tags_git_repos_and_suffixes_plain_directories() {
        let overlay = OverlayData::DirectoryBrowser {
            current_path: "/home/user".to_string(),
            entries: vec![
                entry("easy-review", true, true),
                entry("documents", true, false),
                entry("notes.txt", false, false),
            ],
            selected: 1,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);

        let repo = row_containing(&rows, "easy-review");
        assert!(repo.contains("[git]"), "git repos are tagged: {repo}");
        assert!(!repo.contains("▶"), "row 0 is not selected: {repo}");

        let dir = row_containing(&rows, "documents");
        assert!(
            dir.contains("documents/"),
            "plain directories get a trailing slash: {dir}"
        );
        assert!(!dir.contains("[git]"), "{dir}");
        assert!(dir.contains("▶"), "row 1 is the selected one: {dir}");

        let file = row_containing(&rows, "notes.txt");
        assert!(!file.contains("[git]"), "{file}");
        assert!(
            !file.contains("notes.txt/"),
            "files get neither slash nor tag: {file}"
        );
    }

    /// The title keeps the *tail* of a long path — the leading directories are
    /// what you can afford to lose, the current folder is what you need to see.
    #[test]
    fn directory_browser_truncates_a_long_path_from_the_front() {
        // width 40 → popup width 34 → max title width 14, so the 24-char path
        // is cut down to its last 14 characters.
        let overlay = OverlayData::DirectoryBrowser {
            current_path: "/home/user/projects/deep".to_string(),
            entries: vec![entry("src", true, false)],
            selected: 0,
        };
        let buf = draw_overlay(&overlay, 40, 24);
        let rows = rows(&buf);
        let out = text(&rows);

        assert!(out.contains("…/projects/deep"), "{out}");
        assert!(
            !out.contains("/home/user"),
            "the head of the path must be dropped: {out}"
        );
    }

    // ── render_filter_history ──

    #[test]
    fn filter_history_lists_presets_and_omits_the_separator_when_history_is_empty() {
        let overlay = OverlayData::FilterHistory {
            history: vec![],
            selected: 0,
            preset_count: 2,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);
        let out = text(&rows);

        assert!(out.contains("FILTERS"), "{out}");
        assert!(out.contains("frontend"), "{out}");
        assert!(out.contains("backend"), "{out}");
        assert!(
            !out.contains("── history ──"),
            "no separator without history entries: {out}"
        );
        assert!(row_containing(&rows, "frontend").contains("▶"));
        assert!(!row_containing(&rows, "backend").contains("▶"));
    }

    /// The separator row occupies a visual row but no selection index: with two
    /// presets, index 3 is the *second* history entry, not the first.
    #[test]
    fn filter_history_selection_index_skips_the_separator_row() {
        let overlay = OverlayData::FilterHistory {
            history: vec!["mine-only".to_string(), "risk-high".to_string()],
            selected: 3,
            preset_count: 2,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);
        let out = text(&rows);

        assert!(out.contains("── history ──"), "{out}");
        assert!(
            row_containing(&rows, "risk-high").contains("▶"),
            "index 3 = preset_count(2) + history index 1"
        );
        assert!(!row_containing(&rows, "mine-only").contains("▶"), "{out}");
        assert!(!row_containing(&rows, "frontend").contains("▶"), "{out}");
        assert!(
            !row_containing(&rows, "── history ──").contains("▶"),
            "the separator is never selectable: {out}"
        );
    }

    // ── render_modal_hub ──

    #[test]
    fn modal_hub_help_shows_descriptions_inline_and_suppresses_key_hints() {
        let overlay = OverlayData::ModalHub {
            kind: HubKind::Help,
            title: None,
            items: vec![hub_item("quit", "Ctrl+q", "leave er")],
            selected: 0,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);

        assert!(
            text(&rows).contains("HELP (Esc=close)"),
            "help has nothing to select: {}",
            text(&rows)
        );
        let row = row_containing(&rows, "quit");
        assert!(row.contains("leave er"), "{row}");
        assert!(
            !row.contains("[Ctrl+q]"),
            "the help hub is already a keybinding list — it must not re-render hints: {row}"
        );
    }

    #[test]
    fn modal_hub_action_kind_shows_both_key_hint_and_description() {
        let overlay = OverlayData::ModalHub {
            kind: HubKind::Git,
            title: None,
            items: vec![hub_item("Push to remote", "Ctrl+P", "send commits up")],
            selected: 0,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);

        assert!(
            text(&rows).contains("GIT (Enter=select, Esc=close)"),
            "{}",
            text(&rows)
        );
        let row = row_containing(&rows, "Push to remote");
        assert!(row.contains("[Ctrl+P]"), "{row}");
        assert!(row.contains("send commits up"), "{row}");
    }

    #[test]
    fn modal_hub_headers_render_flush_and_without_a_selection_marker() {
        let mut header = hub_item("Actions", "", "");
        header.is_header = true;
        let overlay = OverlayData::ModalHub {
            kind: HubKind::Ai,
            title: None,
            items: vec![header, hub_item("Stage file", "", "")],
            selected: 1,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);

        let header_row = row_containing(&rows, "Actions");
        assert!(
            !header_row.contains("▶"),
            "section headers are not selectable: {header_row}"
        );
        let item_row = row_containing(&rows, "Stage file");
        assert!(item_row.contains("▶"), "{item_row}");
        assert_eq!(
            col_of(item_row, "Stage file"),
            col_of(header_row, "Actions") + 2,
            "items are indented by the marker column; headers sit flush"
        );
    }

    #[test]
    fn modal_hub_title_override_replaces_the_kind_title() {
        let overlay = OverlayData::ModalHub {
            kind: HubKind::Verify,
            title: Some("VERIFY / frontend".to_string()),
            items: vec![hub_item("run tests", "", "")],
            selected: 0,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let out = text(&rows(&buf));

        assert!(out.contains("VERIFY / frontend (Enter=select"), "{out}");
    }

    /// Expectations are written out as literals rather than read back from
    /// `kind.title()` — checking the renderer against the very call it makes
    /// would still pass if every title collapsed to the empty string.
    #[test]
    fn modal_hub_renders_the_title_of_every_kind() {
        for (kind, expected) in [
            (HubKind::Git, "GIT"),
            (HubKind::Ai, "AI"),
            (HubKind::AiProvider, "AI PROVIDER"),
            (HubKind::AiModel, "AI MODEL"),
            (HubKind::AiEffort, "EFFORT"),
            (HubKind::AiExpert, "SPECIALIZED REVIEW"),
            (HubKind::Verify, "VERIFY"),
            (HubKind::VerifyPackage, "VERIFY"),
            (HubKind::Help, "HELP"),
            (HubKind::Open, "OPEN"),
            (HubKind::Copy, "COPY"),
        ] {
            let overlay = OverlayData::ModalHub {
                kind,
                title: None,
                items: vec![hub_item("an item", "", "")],
                selected: 0,
            };
            let buf = draw_overlay(&overlay, 100, 24);
            let out = text(&rows(&buf));
            // The border title is ` {title} ({hint}) `, so the trailing " (" pins
            // a whole title — " AI (" cannot be satisfied by " AI PROVIDER (".
            assert!(
                out.contains(&format!(" {expected} (")),
                "{kind:?} must render the title {expected:?}:\n{out}"
            );
        }
    }

    /// A disabled item is dimmed to muted while an enabled one keeps the normal
    /// text colour. (Selection is carried by the ▶ marker, not by colour — the
    /// bright and normal text tokens resolve to the same value in every theme.)
    #[test]
    fn modal_hub_dims_disabled_items_and_marks_the_selected_one() {
        let mut disabled = hub_item("third", "", "");
        disabled.enabled = false;
        let overlay = OverlayData::ModalHub {
            kind: HubKind::Git,
            title: None,
            items: vec![
                hub_item("first", "", ""),
                hub_item("second", "", ""),
                disabled,
            ],
            selected: 0,
        };
        let buf = draw_overlay(&overlay, 100, 24);
        let rows = rows(&buf);

        let fg_of = |needle: &str| {
            let y = rows
                .iter()
                .position(|r| r.contains(needle))
                .unwrap_or_else(|| panic!("{needle:?} not rendered:\n{}", text(&rows)));
            let x = col_of(&rows[y], needle);
            buf[(x as u16, y as u16)].fg
        };

        let enabled = fg_of("second");
        let disabled_fg = fg_of("third");

        assert_ne!(
            enabled, disabled_fg,
            "a disabled item is dimmed relative to an enabled one"
        );
        assert!(
            row_containing(&rows, "first").contains("▶"),
            "the selected item carries the marker"
        );
        assert!(
            !row_containing(&rows, "third").contains("▶"),
            "…and nothing else does"
        );
    }
}
