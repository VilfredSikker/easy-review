use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding},
    Frame,
};
use std::time::SystemTime;

use super::styles;
use super::utils::{horizontal_rule, word_wrap};
use er_engine::ai::{Finding, RiskLevel};
use er_engine::app::{App, DiffMode};
use er_engine::git::FileStatus;

/// Format a SystemTime as a relative time string (e.g. "2m ago", "1h ago")
fn format_relative_time(mtime: SystemTime) -> String {
    let elapsed = SystemTime::now().duration_since(mtime).unwrap_or_default();
    let secs = elapsed.as_secs();
    if secs < 60 {
        return format!("{}s ago", secs);
    }
    if secs < 3600 {
        return format!("{}m ago", secs / 60);
    }
    if secs < 86400 {
        return format!("{}h ago", secs / 3600);
    }
    format!("{}d ago", secs / 86400)
}

fn finding_severity_style(severity: RiskLevel, stale: bool) -> ratatui::style::Style {
    if stale {
        styles::stale_style()
    } else {
        match severity {
            RiskLevel::High => styles::risk_high(),
            RiskLevel::Medium => styles::risk_medium(),
            RiskLevel::Low => styles::risk_low(),
            RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE()),
        }
    }
}

fn finding_dots_display_width(count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    const MAX_DOTS: usize = 3;
    if count > MAX_DOTS {
        MAX_DOTS + format!("+{} ", count - MAX_DOTS).chars().count()
    } else {
        count + 1
    }
}

fn finding_dot_spans(findings: &[&Finding], stale: bool) -> Vec<Span<'static>> {
    const MAX_DOTS: usize = 3;
    if findings.is_empty() {
        return Vec::new();
    }
    let mut spans = Vec::new();
    for f in findings.iter().take(MAX_DOTS) {
        spans.push(Span::styled(
            f.severity.symbol().to_string(),
            finding_severity_style(f.severity, stale),
        ));
    }
    if findings.len() > MAX_DOTS {
        spans.push(Span::styled(
            format!("+{} ", findings.len() - MAX_DOTS),
            ratatui::style::Style::default().fg(styles::DIM()),
        ));
    } else {
        spans.push(Span::raw(" "));
    }
    spans
}

/// Render the file tree panel (left side)
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();

    // History mode: render commit list instead of file tree
    if tab.mode == DiffMode::History {
        render_commit_list(f, area, app);
        return;
    }

    // Tour mode: render pillar list instead of file tree
    if tab.mode == DiffMode::Tour {
        render_pillar_list(f, area, app);
        return;
    }

    // Conflicts mode uses the standard file tree (falls through below)

    let visible = tab.visible_files();
    let total = tab.files.len();
    let in_overlay = tab.layers.show_ai_findings;
    let ai_stale = tab.ai.is_stale;

    let stale_count = tab.ai.stale_files.len();
    let visible_watched = tab.visible_watched_files();
    let watched_count = visible_watched.len();
    let visible_count = visible.len();
    let has_filter =
        !tab.filter_expr.is_empty() || !tab.search_query.is_empty() || tab.show_unreviewed_only;
    let count_label = if has_filter {
        format!("{}/{}", visible_count, total)
    } else {
        format!("{}", total)
    };
    let title = if in_overlay && tab.ai.has_data() {
        let findings = tab.ai.total_findings();
        if ai_stale && stale_count > 0 {
            format!(
                " FILES ({}) ⚠ {} findings · {} stale ",
                count_label, findings, stale_count
            )
        } else if ai_stale {
            format!(" FILES ({}) ⚠ {} findings [stale] ", count_label, findings)
        } else {
            format!(" FILES ({}) · {} findings ", count_label, findings)
        }
    } else if watched_count > 0 {
        format!(" FILES ({}) · {} watched ", total, watched_count)
    } else {
        format!(" FILES ({}) ", count_label)
    };

    // Virtualized rendering: find which position the selected file is in the visible list,
    // then only render items in the viewport window
    let viewport_height = area.height.saturating_sub(1) as usize; // -1 for border/title
                                                                  // unwrap_or(0) falls back to position 0 when the selected file isn't in the
                                                                  // visible (filtered) list, so the scroll centres on the first item.
    let selected_pos = visible
        .iter()
        .position(|(i, _)| *i == tab.selected_file)
        .unwrap_or(0);

    // Calculate file_scroll to keep selection visible
    // We compute the scroll position based on the selected file's position in visible list
    let file_scroll = if visible.len() <= viewport_height || selected_pos < viewport_height / 2 {
        0 // Everything fits or near the top
    } else if selected_pos > visible.len().saturating_sub(viewport_height / 2) {
        visible.len().saturating_sub(viewport_height) // Near the bottom
    } else {
        selected_pos.saturating_sub(viewport_height / 2) // Center the selection
    };

    let viewport_end = (file_scroll + viewport_height).min(visible.len());
    let viewport_slice = &visible[file_scroll..viewport_end];

    let mut items: Vec<ListItem> = viewport_slice
        .iter()
        .map(|(idx, file)| {
            let is_selected = tab.selected_watched.is_none() && *idx == tab.selected_file;

            // Status symbol with color
            let is_conflicts_mode = tab.mode == DiffMode::Conflicts;
            let (symbol, symbol_style) = if is_conflicts_mode {
                match &file.status {
                    FileStatus::Unmerged => ("\u{2717}", styles::status_unmerged()),
                    _ => ("\u{2713}", styles::status_resolved()),
                }
            } else {
                match &file.status {
                    FileStatus::Added => ("+", styles::status_added()),
                    FileStatus::Deleted => ("-", styles::status_deleted()),
                    FileStatus::Modified => ("~", styles::status_modified()),
                    FileStatus::Renamed(_) => ("R", styles::status_modified()),
                    FileStatus::Copied(_) => ("C", styles::status_modified()),
                    FileStatus::Unmerged => ("!", styles::status_unmerged()),
                }
            };

            let file_stale = in_overlay && tab.ai.is_file_stale(&file.path);
            let active_findings = if in_overlay {
                tab.ai.file_active_findings(&file.path)
            } else {
                Vec::new()
            };
            let finding_width = finding_dots_display_width(active_findings.len());

            // Comment indicators (questions = yellow ◆N, notes = yellow ▪N, github = cyan ◆N)
            let question_count = tab.ai.file_question_count(&file.path);
            let note_count = tab.ai.file_note_count(&file.path);
            let gh_comment_count = tab.ai.file_github_comment_count(&file.path);
            let has_questions = question_count > 0;
            let has_notes = note_count > 0;
            let has_gh_comments = gh_comment_count > 0;

            // Relative time when sorting by mtime — read from the cache populated on refresh,
            // not from the filesystem directly (avoids per-frame syscalls).
            let time_str = if tab.sort_by_mtime {
                let mtime = tab
                    .mtime_cache
                    .get(&file.path)
                    .copied()
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                Some(format_relative_time(mtime))
            } else {
                None
            };
            // Time column takes up to 8 chars (e.g. "15m ago " or "3h ago  ")
            let time_width: usize = if time_str.is_some() { 8 } else { 0 };

            // Comment indicator width: "◆N " where N is 1-2 digits (3-4 chars each)
            let q_indicator = if has_questions {
                format!("\u{25c6}{} ", question_count)
            } else {
                String::new()
            };
            let n_indicator = if has_notes {
                format!("\u{25aa}{} ", note_count)
            } else {
                String::new()
            };
            let gh_indicator = if has_gh_comments {
                format!("\u{25c6}{} ", gh_comment_count)
            } else {
                String::new()
            };
            let comment_width: usize = q_indicator.chars().count()
                + n_indicator.chars().count()
                + gh_indicator.chars().count();

            // Adjust path width to account for finding dots, comment indicators, and time column
            let path = shorten_path(
                &file.path,
                (area.width as usize)
                    .saturating_sub(16 + finding_width + comment_width + time_width),
            );

            // Stats: +adds -dels
            let stats = format!("+{} -{}", file.adds, file.dels);

            let is_reviewed = tab.reviewed.contains_key(&file.path);
            let is_compacted = file.compacted;

            let line_style = if is_selected {
                styles::selected_style()
            } else if is_compacted || is_reviewed {
                ratatui::style::Style::default()
                    .fg(styles::DIM())
                    .bg(styles::SURFACE())
            } else {
                styles::surface_style()
            };

            // Dim the symbol if reviewed (unless selected)
            let effective_symbol_style = if is_reviewed && !is_selected {
                ratatui::style::Style::default().fg(styles::DIM())
            } else {
                symbol_style
            };

            let path_width = (area.width as usize)
                .saturating_sub(14 + finding_width + comment_width + time_width)
                .max(1);

            let mut spans = vec![Span::styled(
                format!(" {} ", symbol),
                effective_symbol_style,
            )];

            spans.push(Span::styled(
                format!("{:<width$}", path, width = path_width),
                if is_selected {
                    styles::selected_style()
                } else if is_reviewed {
                    ratatui::style::Style::default().fg(styles::DIM())
                } else {
                    ratatui::style::Style::default().fg(styles::TEXT())
                },
            ));

            spans.extend(finding_dot_spans(&active_findings, file_stale));

            // Comment indicators after path (with counts)
            if has_questions {
                spans.push(Span::styled(
                    q_indicator,
                    ratatui::style::Style::default().fg(styles::YELLOW()),
                ));
            }
            if has_notes {
                spans.push(Span::styled(
                    n_indicator,
                    ratatui::style::Style::default().fg(styles::YELLOW()),
                ));
            }
            if has_gh_comments {
                spans.push(Span::styled(
                    gh_indicator,
                    ratatui::style::Style::default().fg(styles::CYAN()),
                ));
            }
            // Show relative time when sorting by mtime
            if let Some(ref ts) = time_str {
                spans.push(Span::styled(
                    format!("{:>7} ", ts),
                    ratatui::style::Style::default().fg(styles::MUTED()),
                ));
            }
            if area.width > 24 {
                spans.push(Span::styled(
                    format!("{:>8} ", stats),
                    ratatui::style::Style::default().fg(styles::DIM()),
                ));
            }

            ListItem::new(Line::from(spans)).style(line_style)
        })
        .collect();

    // ── Watched files section ──
    if !visible_watched.is_empty() {
        // Separator
        let sep_width = area.width.saturating_sub(2) as usize;
        let sep_label = " watched ";
        let dash_count = sep_width.saturating_sub(sep_label.len()) / 2;
        let sep_text = format!(
            "{}{}{}",
            horizontal_rule(dash_count),
            sep_label,
            horizontal_rule(sep_width.saturating_sub(dash_count + sep_label.len()))
        );
        items.push(
            ListItem::new(Line::from(Span::styled(
                format!(" {}", sep_text),
                ratatui::style::Style::default().fg(styles::WATCHED_MUTED()),
            )))
            .style(styles::surface_style()),
        );

        // Watched files
        for (idx, watched) in &visible_watched {
            let is_selected = tab.selected_watched == Some(*idx);
            let age = format_relative_time(watched.modified);
            let not_ignored = tab.watched_not_ignored.contains(&watched.path);

            let path = shorten_path(&watched.path, (area.width as usize).saturating_sub(16));
            let path_width = (area.width as usize).saturating_sub(14).max(1);

            let line_style = if is_selected {
                styles::selected_style()
            } else {
                styles::surface_style()
            };

            let icon = if not_ignored { "\u{26a0}" } else { "\u{25c9}" };
            let icon_style = if not_ignored {
                ratatui::style::Style::default().fg(styles::YELLOW())
            } else {
                ratatui::style::Style::default().fg(styles::WATCHED_TEXT())
            };

            let mut spans = vec![Span::styled(format!(" {} ", icon), icon_style)];

            spans.push(Span::styled(
                format!("{:<width$}", path, width = path_width),
                if is_selected {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().fg(styles::WATCHED_TEXT())
                },
            ));
            if area.width > 24 {
                spans.push(Span::styled(
                    format!("{:>8} ", age),
                    ratatui::style::Style::default().fg(styles::WATCHED_MUTED()),
                ));
            }

            items.push(ListItem::new(Line::from(spans)).style(line_style));
        }
    }

    let title_style = if in_overlay && tab.ai.has_data() && !ai_stale {
        ratatui::style::Style::default().fg(styles::PURPLE())
    } else if ai_stale {
        styles::stale_style()
    } else {
        ratatui::style::Style::default().fg(styles::MUTED())
    };

    let block = Block::default()
        .title(Span::styled(title, title_style))
        .borders(Borders::RIGHT)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER()))
        .style(ratatui::style::Style::default().bg(styles::SURFACE()))
        .padding(Padding::new(0, 0, 0, 0));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Render the commit list panel (left side, History mode)
fn render_commit_list(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let visible = tab.visible_commits();
    let total = tab.history.as_ref().map(|h| h.commits.len()).unwrap_or(0);
    let selected_commit = tab.history.as_ref().map(|h| h.selected_commit).unwrap_or(0);

    let title = format!(" COMMITS ({}) ", total);

    // " ● " = 3 chars for the indicator prefix; leave 1 char margin on the right
    let indicator_width: usize = 3;
    let subject_width = (area.width as usize)
        .saturating_sub(indicator_width + 1)
        .max(1);

    // Calculate the visual height of each commit item (subject lines + author + separator)
    let item_heights: Vec<usize> = visible
        .iter()
        .map(|(_, commit)| {
            let merge_prefix = if commit.is_merge { "⊕ " } else { "" };
            let full_subject = format!("{}{}", merge_prefix, commit.subject);
            let subject_lines = word_wrap(&full_subject, subject_width).len().max(1);
            subject_lines + 2 // author line + separator
        })
        .collect();

    // Find which visual index corresponds to the selected commit
    let selected_visual_idx = visible
        .iter()
        .position(|(i, _)| *i == selected_commit)
        .unwrap_or(0);

    let available_height = area.height.saturating_sub(2) as usize; // account for border/title

    // Determine scroll_start: the first commit index to render, so selection stays in view
    let height_before_selected: usize = item_heights[..selected_visual_idx].iter().sum();
    let selected_height = item_heights.get(selected_visual_idx).copied().unwrap_or(3);

    let scroll_start = if height_before_selected + selected_height > available_height {
        // Selection would fall below the viewport — scroll down
        let target = height_before_selected + selected_height - available_height;
        let mut accumulated = 0;
        let mut start = 0;
        for (i, h) in item_heights.iter().enumerate() {
            if accumulated >= target {
                break;
            }
            accumulated += h;
            start = i + 1;
        }
        start
    } else {
        0
    };

    let visible_from_scroll = &visible[scroll_start..];

    let items: Vec<ListItem> = visible_from_scroll
        .iter()
        .flat_map(|(idx, commit)| {
            let is_selected = *idx == selected_commit;

            let line_style = if is_selected {
                styles::selected_style()
            } else {
                styles::surface_style()
            };

            let indicator = if is_selected { "●" } else { "○" };
            let merge_prefix = if commit.is_merge { "⊕ " } else { "" };
            let full_subject = format!("{}{}", merge_prefix, commit.subject);

            let wrapped_lines = word_wrap(&full_subject, subject_width);

            let indicator_style = if is_selected {
                ratatui::style::Style::default().fg(styles::PURPLE())
            } else {
                ratatui::style::Style::default().fg(styles::DIM())
            };
            let subject_style = if is_selected {
                ratatui::style::Style::default().fg(styles::BRIGHT())
            } else {
                ratatui::style::Style::default().fg(styles::TEXT())
            };
            let continuation_indent = " ".repeat(indicator_width);

            // First wrapped line: indicator + subject text
            let first_line = Line::from(vec![
                Span::styled(format!(" {} ", indicator), indicator_style),
                Span::styled(
                    wrapped_lines.first().cloned().unwrap_or_default(),
                    subject_style,
                ),
            ]);

            // Additional wrapped lines (indented to align with subject)
            let continuation_lines: Vec<ListItem> = wrapped_lines
                .iter()
                .skip(1)
                .map(|segment| {
                    let line = Line::from(vec![
                        Span::styled(
                            continuation_indent.clone(),
                            ratatui::style::Style::default(),
                        ),
                        Span::styled(segment.clone(), subject_style),
                    ]);
                    ListItem::new(line).style(line_style)
                })
                .collect();

            // Author line: indented, dimmed
            let author_line = Line::from(vec![Span::styled(
                format!("   {}", commit.author),
                ratatui::style::Style::default().fg(styles::DIM()),
            )]);

            // Separator line
            let separator = Line::from(Span::styled(
                horizontal_rule(area.width.saturating_sub(2) as usize),
                ratatui::style::Style::default().fg(styles::BORDER()),
            ));

            let mut result = vec![ListItem::new(first_line).style(line_style)];
            result.extend(continuation_lines);
            result.push(ListItem::new(author_line).style(line_style));
            result.push(ListItem::new(separator).style(styles::surface_style()));
            result
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(
            title,
            ratatui::style::Style::default().fg(styles::MUTED()),
        ))
        .borders(Borders::RIGHT)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER()))
        .style(ratatui::style::Style::default().bg(styles::SURFACE()))
        .padding(Padding::new(0, 0, 0, 0));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Render the tour pillar list (left panel in Tour mode). Each pillar shows its
/// title, a foundation/importance badge, a reviewed/total count, and its files.
fn render_pillar_list(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();
    let Some(tour) = tab.tour.as_ref() else {
        let block = Block::default()
            .title(Span::styled(
                " TOUR ",
                ratatui::style::Style::default().fg(styles::MUTED()),
            ))
            .borders(Borders::RIGHT)
            .border_style(ratatui::style::Style::default().fg(styles::BORDER()))
            .style(ratatui::style::Style::default().bg(styles::SURFACE()));
        f.render_widget(block, area);
        return;
    };

    let total: usize = tour.pillars.len();
    let title = format!(" PILLARS ({}) ", total);
    let inner_width = (area.width as usize).saturating_sub(2).max(1);

    let mut items: Vec<ListItem> = Vec::new();
    for (pi, pillar) in tour.pillars.iter().enumerate() {
        let is_selected = pi == tour.selected_pillar;
        let (start, end) = tour.pillar_file_ranges.get(pi).copied().unwrap_or((0, 0));
        let files = tour.files.get(start..end).unwrap_or(&[]);
        let reviewed = files
            .iter()
            .filter(|fd| tab.reviewed.contains_key(&fd.path))
            .count();
        let total_files = files.len();
        let all_reviewed = total_files > 0 && reviewed == total_files;

        let line_style = if is_selected {
            styles::selected_style()
        } else {
            styles::surface_style()
        };
        let indicator = if is_selected { "●" } else { "○" };
        let indicator_style = if is_selected {
            ratatui::style::Style::default().fg(styles::PURPLE())
        } else {
            ratatui::style::Style::default().fg(styles::DIM())
        };
        let title_style = if is_selected {
            ratatui::style::Style::default().fg(styles::BRIGHT())
        } else {
            ratatui::style::Style::default().fg(styles::TEXT())
        };

        // Badge: foundation marker + reviewed count.
        let badge = if all_reviewed {
            " ✓".to_string()
        } else {
            format!(" {:02}/{:02}", reviewed, total_files)
        };
        let badge_style = if all_reviewed {
            ratatui::style::Style::default().fg(styles::GREEN())
        } else {
            ratatui::style::Style::default().fg(styles::DIM())
        };

        let title_text = if pillar.foundation {
            format!("◆ {}", pillar.title)
        } else {
            pillar.title.clone()
        };
        let title_width = inner_width.saturating_sub(3 + badge.len());
        let wrapped = word_wrap(&title_text, title_width.max(1));

        let first = Line::from(vec![
            Span::styled(format!(" {} ", indicator), indicator_style),
            Span::styled(wrapped.first().cloned().unwrap_or_default(), title_style),
            Span::styled(badge.clone(), badge_style),
        ]);
        items.push(ListItem::new(first).style(line_style));
        for seg in wrapped.iter().skip(1) {
            let line = Line::from(vec![
                Span::raw("   "),
                Span::styled(seg.clone(), title_style),
            ]);
            items.push(ListItem::new(line).style(line_style));
        }

        // File rows under the pillar.
        for (fi, fd) in files.iter().enumerate() {
            let abs_idx = start + fi;
            let is_file_selected = is_selected && abs_idx == tour.selected_file;
            // Co-located related files (tests/styles/stories/snapshots) render
            // indented under their primary file with a "↳" marker.
            let is_related = tour.file_is_related.get(abs_idx).copied().unwrap_or(false);
            let reviewed_mark = if tab.reviewed.contains_key(&fd.path) {
                "✓ "
            } else {
                "  "
            };
            // `indent_cols` is the display width (not byte length — "↳" is a
            // 3-byte, 1-column glyph, so `indent.len()` would over-truncate).
            let (indent, indent_cols) = if is_related {
                ("     ↳ ", 7)
            } else {
                ("   ", 3)
            };
            let name = shorten_path(&fd.path, inner_width.saturating_sub(indent_cols + 3));
            let fstyle = if is_file_selected {
                ratatui::style::Style::default().fg(styles::BRIGHT())
            } else if tab.reviewed.contains_key(&fd.path) || is_related {
                ratatui::style::Style::default().fg(styles::DIM())
            } else {
                ratatui::style::Style::default().fg(styles::TEXT())
            };
            let mark_style = ratatui::style::Style::default().fg(styles::GREEN());
            let row_style = if is_file_selected {
                styles::selected_style()
            } else {
                styles::surface_style()
            };
            let line = Line::from(vec![
                Span::styled(indent, ratatui::style::Style::default().fg(styles::DIM())),
                Span::styled(reviewed_mark, mark_style),
                Span::styled(name, fstyle),
            ]);
            items.push(ListItem::new(line).style(row_style));
        }

        // Separator
        items.push(
            ListItem::new(Line::from(Span::styled(
                horizontal_rule(area.width.saturating_sub(2) as usize),
                ratatui::style::Style::default().fg(styles::BORDER()),
            )))
            .style(styles::surface_style()),
        );
    }

    let block = Block::default()
        .title(Span::styled(
            title,
            ratatui::style::Style::default().fg(styles::MUTED()),
        ))
        .borders(Borders::RIGHT)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER()))
        .style(ratatui::style::Style::default().bg(styles::SURFACE()))
        .padding(Padding::new(0, 0, 0, 0));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Shorten a file path to fit within max_width
pub fn shorten_path(path: &str, max_width: usize) -> String {
    if path.len() <= max_width {
        return path.to_string();
    }

    // Try to show just the filename
    if let Some(name) = path.rsplit('/').next() {
        if name.len() <= max_width {
            let remaining = max_width.saturating_sub(name.len() + 4);
            if remaining > 0 {
                // Show partial directory
                let dir_part: String = path[..path.len() - name.len() - 1]
                    .chars()
                    .take(remaining)
                    .collect();
                return format!("{}…/{}", dir_part, name);
            }
            return name.to_string();
        }
        // Truncate the filename itself
        let truncated: String = name.chars().take(max_width.saturating_sub(1)).collect();
        return format!("{}…", truncated);
    }

    let truncated: String = path.chars().take(max_width.saturating_sub(1)).collect();
    format!("{}…", truncated)
}

#[cfg(test)]
mod tests {
    use super::shorten_path;

    #[test]
    fn path_shorter_than_max_width_returned_as_is() {
        assert_eq!(shorten_path("src/main.rs", 30), "src/main.rs");
    }

    #[test]
    fn path_equal_to_max_width_returned_as_is() {
        assert_eq!(shorten_path("src/main.rs", 11), "src/main.rs");
    }

    #[test]
    fn long_path_filename_fits_directory_truncated() {
        // len("src/very/long/nested/path/main.rs") = 34 > 20
        // filename = "main.rs" (7), remaining = 20 - (7+4) = 9
        // dir_part = first 9 chars of "src/very/long/nested/path" = "src/very/"
        assert_eq!(
            shorten_path("src/very/long/nested/path/main.rs", 20),
            "src/very/…/main.rs"
        );
    }

    #[test]
    fn path_with_no_directory_returned_as_is() {
        assert_eq!(shorten_path("README.md", 30), "README.md");
    }

    #[test]
    fn filename_longer_than_max_width_truncated_with_ellipsis() {
        // len("very_long_filename_here.rs") = 26 > 10
        // name = "very_long_filename_here.rs" (no '/'), name.len() 26 > 10
        // truncated = first 9 chars = "very_long", result = "very_long…"
        assert_eq!(shorten_path("very_long_filename_here.rs", 10), "very_long…");
    }

    #[test]
    fn max_width_zero_does_not_panic() {
        // len("src/main.rs") = 11 > 0
        // name = "main.rs" (7), 7 > 0, so truncate: take(0) = "", result = "…"
        assert_eq!(shorten_path("src/main.rs", 0), "…");
    }

    #[test]
    fn single_component_deep_path_fits_max_width() {
        // len("a/b/c/d/e/f.rs") = 14 > 10
        // filename = "f.rs" (4), remaining = 10 - (4+4) = 2
        // dir_part = first 2 chars of "a/b/c/d/e" = "a/"
        assert_eq!(shorten_path("a/b/c/d/e/f.rs", 10), "a/…/f.rs");
    }

    #[test]
    fn exact_boundary_remaining_zero_returns_filename_only() {
        // len("some/dir/main.rs") = 16 > 11
        // filename = "main.rs" (7), remaining = 11 - (7+4) = 0
        // remaining is not > 0, so returns filename only
        assert_eq!(shorten_path("some/dir/main.rs", 11), "main.rs");
    }
}

#[cfg(test)]
mod finding_severity_style_tests {
    use super::finding_severity_style;
    use er_engine::ai::RiskLevel;
    use ratatui::style::Modifier;

    // Colours are theme-dependent (and the theme is global mutable state shared by
    // every test in this binary), so these pin the theme-independent facts: which
    // severities are emphasised, and that severities stay distinguishable.

    #[test]
    fn high_and_medium_findings_are_bold_low_and_info_are_not() {
        assert!(finding_severity_style(RiskLevel::High, false)
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(finding_severity_style(RiskLevel::Medium, false)
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(!finding_severity_style(RiskLevel::Low, false)
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(!finding_severity_style(RiskLevel::Info, false)
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn each_severity_gets_its_own_colour() {
        let high = finding_severity_style(RiskLevel::High, false).fg;
        let medium = finding_severity_style(RiskLevel::Medium, false).fg;
        let low = finding_severity_style(RiskLevel::Low, false).fg;
        let info = finding_severity_style(RiskLevel::Info, false).fg;

        assert!(high.is_some(), "severity dots always carry a colour");
        assert_ne!(high, medium);
        assert_ne!(high, low);
        assert_ne!(high, info);
        assert_ne!(medium, low);
        assert_ne!(medium, info);
        assert_ne!(low, info);
    }

    /// A stale finding was generated against a diff that has since changed — it
    /// must stop shouting, whatever severity it claims.
    #[test]
    fn stale_findings_lose_their_severity_emphasis() {
        for level in [
            RiskLevel::High,
            RiskLevel::Medium,
            RiskLevel::Low,
            RiskLevel::Info,
        ] {
            assert!(
                !finding_severity_style(level, true)
                    .add_modifier
                    .contains(Modifier::BOLD),
                "{level:?} must not stay bold when stale"
            );
        }
        assert!(
            finding_severity_style(RiskLevel::High, false)
                .add_modifier
                .contains(Modifier::BOLD),
            "…but a fresh high-severity finding still is"
        );
    }

    #[test]
    fn stale_collapses_every_severity_onto_one_colour() {
        let high = finding_severity_style(RiskLevel::High, true).fg;
        for level in [RiskLevel::Medium, RiskLevel::Low, RiskLevel::Info] {
            assert_eq!(
                finding_severity_style(level, true).fg,
                high,
                "stale styling ignores severity ({level:?})"
            );
        }
    }
}

#[cfg(test)]
mod commit_list_render_tests {
    use super::*;
    use er_engine::git::CommitInfo;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    fn draw_tree(app: &App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|f| render(f, f.area(), app))
            .expect("draw file tree");
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

    fn commit(subject: &str, author: &str, is_merge: bool) -> CommitInfo {
        CommitInfo {
            hash: format!("hash-{subject}"),
            short_hash: format!("s{subject}"),
            subject: subject.to_string(),
            author: author.to_string(),
            date: "2026-01-01".to_string(),
            relative_date: "1 day ago".to_string(),
            file_count: 1,
            adds: 1,
            dels: 0,
            is_merge,
        }
    }

    /// History mode on a local PR tab: `set_mode` takes the commit list straight
    /// from `pr_commits` (no `git log`), and keeping `remote_repo` set pins the
    /// review bucket across the switch so no managed-storage dirs are touched.
    fn app_in_history(commits: Vec<CommitInfo>, selected: usize) -> App {
        let mut app = App::new_for_test(vec![]);
        {
            let tab = app.tab_mut();
            tab.remote_repo = Some("owner/repo".to_string());
            tab.local_branch_view = Some("feature".to_string());
            tab.local_branch_checkout_root = Some("/er-tui-test/no-such-repo".to_string());
            tab.pr_number = Some(1);
            tab.pr_commits = commits;
            tab.set_mode(DiffMode::History);
            if let Some(history) = tab.history.as_mut() {
                history.selected_commit = selected;
            }
        }
        app
    }

    #[test]
    fn commit_list_marks_the_selected_commit_and_flags_merges() {
        let app = app_in_history(
            vec![
                commit("add the parser", "ada", false),
                commit("bring topic into trunk", "grace", true),
            ],
            1,
        );
        let buf = draw_tree(&app, 40, 20);
        let rows = rows(&buf);
        let out = text(&rows);

        assert!(out.contains("COMMITS (2)"), "{out}");

        let merge = row_containing(&rows, "bring topic into trunk");
        assert!(merge.contains("●"), "the selected commit gets a filled dot");
        assert!(
            merge.contains("⊕"),
            "merge commits get a merge glyph: {merge}"
        );

        let plain = row_containing(&rows, "add the parser");
        assert!(plain.contains("○"), "unselected commits get a hollow dot");
        assert!(!plain.contains("⊕"), "non-merges get no glyph: {plain}");

        assert!(out.contains("ada"), "author line renders: {out}");
        assert!(out.contains("grace"), "{out}");
    }

    /// The list scrolls by accumulated *row height*, not commit index, so the
    /// selection stays on screen even though each commit is three rows tall.
    #[test]
    fn commit_list_scrolls_so_the_selected_commit_stays_visible() {
        let subjects = [
            "commit-zero",
            "commit-one",
            "commit-two",
            "commit-three",
            "commit-four",
            "commit-five",
        ];
        let app = app_in_history(
            subjects
                .iter()
                .map(|s| commit(s, "ada", false))
                .collect::<Vec<_>>(),
            5,
        );
        // height 8 → 6 usable rows → only the last two 3-row commits fit.
        let buf = draw_tree(&app, 40, 8);
        let rows = rows(&buf);
        let out = text(&rows);

        assert!(
            out.contains("commit-five"),
            "selection must be visible:\n{out}"
        );
        assert!(row_containing(&rows, "commit-five").contains("●"));
        assert!(out.contains("commit-four"), "{out}");
        for hidden in ["commit-zero", "commit-one", "commit-two", "commit-three"] {
            assert!(
                !out.contains(hidden),
                "{hidden} should have scrolled off:\n{out}"
            );
        }
        assert!(
            out.contains("COMMITS (6)"),
            "the header still counts every commit:\n{out}"
        );
    }

    #[test]
    fn commit_list_shows_a_zero_count_before_history_loads() {
        let mut app = App::new_for_test(vec![]);
        app.tab_mut().mode = DiffMode::History;

        let buf = draw_tree(&app, 40, 12);
        let out = text(&rows(&buf));

        assert!(out.contains("COMMITS (0)"), "{out}");
        assert!(
            !out.contains("FILES"),
            "History mode replaces the file tree"
        );
    }
}

#[cfg(test)]
mod pillar_list_render_tests {
    use super::*;
    use er_engine::ai::{ErTour, TourFile, TourPillar, TourRelatedFile};
    use er_engine::git::DiffFile;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    fn draw_tree(app: &App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|f| render(f, f.area(), app))
            .expect("draw file tree");
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

    fn col_of(row: &str, needle: &str) -> usize {
        let byte = row
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} not in {row:?}"));
        row[..byte].chars().count()
    }

    fn diff_file(path: &str) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks: vec![],
            adds: 1,
            dels: 0,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    fn tour_file(path: &str, related: Vec<TourRelatedFile>) -> TourFile {
        TourFile {
            path: path.to_string(),
            reason: String::new(),
            finding_ids: Vec::new(),
            related,
        }
    }

    fn pillar(
        id: &str,
        title: &str,
        order: u32,
        foundation: bool,
        files: Vec<TourFile>,
    ) -> TourPillar {
        TourPillar {
            id: id.to_string(),
            title: title.to_string(),
            description: String::new(),
            order,
            importance: 0,
            foundation,
            files,
        }
    }

    fn app_in_tour(files: Vec<DiffFile>, pillars: Vec<TourPillar>) -> App {
        let mut app = App::new_for_test(files);
        {
            let tab = app.tab_mut();
            tab.ai.tour = Some(ErTour {
                version: 1,
                diff_hash: String::new(),
                created_at: String::new(),
                title: String::new(),
                overview: String::new(),
                pillars,
            });
            tab.rebuild_tour_state();
            tab.mode = DiffMode::Tour;
        }
        app
    }

    #[test]
    fn pillar_list_without_a_tour_renders_an_empty_tour_header() {
        let mut app = App::new_for_test(vec![diff_file("src/a.rs")]);
        app.tab_mut().mode = DiffMode::Tour;

        let buf = draw_tree(&app, 40, 12);
        let out = text(&rows(&buf));

        assert!(out.contains("TOUR"), "{out}");
        assert!(
            !out.contains("PILLARS"),
            "no pillar count without a tour: {out}"
        );
        assert!(
            !out.contains("src/a.rs"),
            "the diff files are not listed until a tour exists: {out}"
        );
    }

    #[test]
    fn pillar_list_flags_foundation_pillars_and_counts_unreviewed_files() {
        let app = app_in_tour(
            vec![diff_file("src/auth.rs"), diff_file("src/ui.rs")],
            vec![
                pillar(
                    "p-auth",
                    "Auth core",
                    0,
                    true,
                    vec![tour_file("src/auth.rs", vec![])],
                ),
                pillar(
                    "p-ui",
                    "UI shell",
                    1,
                    false,
                    vec![tour_file("src/ui.rs", vec![])],
                ),
            ],
        );
        let buf = draw_tree(&app, 40, 20);
        let rows = rows(&buf);
        let out = text(&rows);

        assert!(out.contains("PILLARS (2)"), "{out}");

        let auth = row_containing(&rows, "Auth core");
        assert!(
            auth.contains("◆ Auth core"),
            "foundation pillars get the ◆ marker: {auth}"
        );
        assert!(auth.contains("00/01"), "0 of 1 reviewed: {auth}");
        assert!(auth.contains("●"), "the first pillar is selected: {auth}");

        let ui = row_containing(&rows, "UI shell");
        assert!(
            !ui.contains("◆"),
            "non-foundation pillars get no marker: {ui}"
        );
        assert!(ui.contains("○"), "{ui}");

        assert!(
            out.contains("src/auth.rs"),
            "pillar files are listed: {out}"
        );
        assert!(out.contains("src/ui.rs"), "{out}");
    }

    #[test]
    fn pillar_list_swaps_the_counter_for_a_check_once_every_file_is_reviewed() {
        let mut app = app_in_tour(
            vec![diff_file("src/auth.rs"), diff_file("src/ui.rs")],
            vec![
                pillar(
                    "p-auth",
                    "Auth core",
                    0,
                    false,
                    vec![tour_file("src/auth.rs", vec![])],
                ),
                pillar(
                    "p-ui",
                    "UI shell",
                    1,
                    false,
                    vec![tour_file("src/ui.rs", vec![])],
                ),
            ],
        );
        app.tab_mut()
            .reviewed
            .insert("src/auth.rs".to_string(), String::new());

        let buf = draw_tree(&app, 40, 20);
        let rows = rows(&buf);

        let auth = row_containing(&rows, "Auth core");
        assert!(
            auth.contains("✓"),
            "fully reviewed pillar shows a check: {auth}"
        );
        assert!(!auth.contains("00/01"), "…instead of a counter: {auth}");

        let ui = row_containing(&rows, "UI shell");
        assert!(
            ui.contains("00/01"),
            "a pillar with unreviewed files keeps its counter: {ui}"
        );

        let file_row = row_containing(&rows, "src/auth.rs");
        assert!(
            file_row.contains("✓ src/auth.rs"),
            "reviewed files are ticked: {file_row}"
        );
        let unreviewed = row_containing(&rows, "src/ui.rs");
        assert!(!unreviewed.contains("✓"), "{unreviewed}");
    }

    /// Co-located tests/styles/stories hang off their primary file with a ↳ and an
    /// extra four columns of indent.
    #[test]
    fn pillar_list_indents_related_files_under_their_primary() {
        let app = app_in_tour(
            vec![diff_file("src/auth.rs"), diff_file("src/auth.test.rs")],
            vec![pillar(
                "p-auth",
                "Auth core",
                0,
                false,
                vec![tour_file(
                    "src/auth.rs",
                    vec![TourRelatedFile {
                        path: "src/auth.test.rs".to_string(),
                        kind: "test".to_string(),
                        reason: String::new(),
                    }],
                )],
            )],
        );
        let buf = draw_tree(&app, 44, 20);
        let rows = rows(&buf);

        let primary = row_containing(&rows, "src/auth.rs");
        let related = row_containing(&rows, "src/auth.test.rs");

        assert!(
            related.contains("↳"),
            "related files get the ↳ marker: {related}"
        );
        assert!(!primary.contains("↳"), "primaries do not: {primary}");
        assert_eq!(
            col_of(related, "src/auth.test.rs"),
            col_of(primary, "src/auth.rs") + 4,
            "related rows are indented four columns further than their primary"
        );
    }
}
