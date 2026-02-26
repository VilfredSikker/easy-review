use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding},
    Frame,
};
use std::time::SystemTime;

use crate::ai::RiskLevel;
use crate::app::{App, DiffMode};
use crate::git::FileStatus;
use super::styles;
use super::utils::word_wrap;

/// Format a SystemTime as a relative time string (e.g. "2m ago", "1h ago")
fn format_relative_time(mtime: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or_default();
    let secs = elapsed.as_secs();
    if secs < 60 { return format!("{}s ago", secs); }
    if secs < 3600 { return format!("{}m ago", secs / 60); }
    if secs < 86400 { return format!("{}h ago", secs / 3600); }
    format!("{}d ago", secs / 86400)
}

/// Render the file tree panel (left side)
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.tab();

    // History mode: render commit list instead of file tree
    if tab.mode == DiffMode::History {
        render_commit_list(f, area, app);
        return;
    }

    let visible = tab.visible_files();
    let total = tab.files.len();
    let in_overlay = tab.layers.show_ai_findings;
    let ai_stale = tab.ai.is_stale;

    let stale_count = tab.ai.stale_files.len();
    let visible_watched = tab.visible_watched_files();
    let watched_count = visible_watched.len();
    let visible_count = visible.len();
    let has_filter = !tab.filter_expr.is_empty() || !tab.search_query.is_empty() || tab.show_unreviewed_only;
    let count_label = if has_filter {
        format!("{}/{}", visible_count, total)
    } else {
        format!("{}", total)
    };
    let title = if in_overlay && tab.ai.has_data() {
        let findings = tab.ai.total_findings();
        if ai_stale && stale_count > 0 {
            format!(" FILES ({}) ⚠ {} findings · {} stale ", count_label, findings, stale_count)
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
    let selected_pos = visible.iter().position(|(i, _)| *i == tab.selected_file).unwrap_or(0);

    // Calculate file_scroll to keep selection visible
    // We compute the scroll position based on the selected file's position in visible list
    let file_scroll = if visible.len() <= viewport_height {
        0 // Everything fits, no scroll needed
    } else if selected_pos < viewport_height / 2 {
        0 // Near the top
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
            let (symbol, symbol_style) = match &file.status {
                FileStatus::Added => ("+", styles::status_added()),
                FileStatus::Deleted => ("-", styles::status_deleted()),
                FileStatus::Modified => ("~", styles::status_modified()),
                FileStatus::Renamed(_) => ("R", styles::status_modified()),
                FileStatus::Copied(_) => ("C", styles::status_modified()),
            };

            // Risk dot (only in overlay mode with AI data)
            let risk_dot = if in_overlay {
                if let Some(fr) = tab.ai.file_review(&file.path) {
                    let file_stale = tab.ai.is_file_stale(&file.path);
                    let dot_style = if file_stale {
                        styles::stale_style()
                    } else {
                        match fr.risk {
                            RiskLevel::High => styles::risk_high(),
                            RiskLevel::Medium => styles::risk_medium(),
                            RiskLevel::Low => styles::risk_low(),
                            RiskLevel::Info => ratatui::style::Style::default().fg(styles::BLUE),
                        }
                    };
                    Some(Span::styled(
                        format!("{} ", fr.risk.symbol()),
                        dot_style,
                    ))
                } else {
                    // No AI data for this file — show empty dot
                    Some(Span::styled(
                        "  ",
                        ratatui::style::Style::default(),
                    ))
                }
            } else {
                None
            };

            // Comment indicators (questions = yellow ◆N, github = cyan ◆N)
            let question_count = tab.ai.file_question_count(&file.path);
            let gh_comment_count = tab.ai.file_github_comment_count(&file.path);
            let has_questions = question_count > 0;
            let has_gh_comments = gh_comment_count > 0;

            // Relative time when sorting by mtime
            let time_str = if tab.sort_by_mtime {
                let mtime = std::fs::metadata(format!("{}/{}", tab.repo_root, file.path))
                    .and_then(|m| m.modified())
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
            let gh_indicator = if has_gh_comments {
                format!("\u{25c6}{} ", gh_comment_count)
            } else {
                String::new()
            };
            let comment_width: usize = q_indicator.chars().count() + gh_indicator.chars().count();

            // Adjust path width to account for risk dot, comment indicators, and time column
            let extra_width = if risk_dot.is_some() { 2 } else { 0 };
            let path = shorten_path(
                &file.path,
                (area.width as usize).saturating_sub(16 + extra_width + comment_width + time_width),
            );

            // Stats: +adds -dels
            let stats = format!("+{} -{}", file.adds, file.dels);

            let is_reviewed = tab.reviewed.contains(&file.path);
            let is_compacted = file.compacted;

            let line_style = if is_selected {
                styles::selected_style()
            } else if is_compacted {
                ratatui::style::Style::default().fg(styles::DIM).bg(styles::SURFACE)
            } else if is_reviewed {
                ratatui::style::Style::default().fg(styles::DIM).bg(styles::SURFACE)
            } else {
                styles::surface_style()
            };

            // Dim the symbol if reviewed (unless selected)
            let effective_symbol_style = if is_reviewed && !is_selected {
                ratatui::style::Style::default().fg(styles::DIM)
            } else {
                symbol_style
            };

            let path_width = (area.width as usize).saturating_sub(14 + extra_width + comment_width + time_width).max(1);

            let mut spans = vec![
                Span::styled(format!(" {} ", symbol), effective_symbol_style),
            ];

            // Insert risk dot after status symbol
            if let Some(dot) = risk_dot {
                spans.push(dot);
            }

            spans.push(Span::styled(
                format!("{:<width$}", path, width = path_width),
                if is_selected {
                    styles::selected_style()
                } else if is_reviewed {
                    ratatui::style::Style::default().fg(styles::DIM)
                } else {
                    ratatui::style::Style::default().fg(styles::TEXT)
                },
            ));
            // Comment indicators after path (with counts)
            if has_questions {
                spans.push(Span::styled(
                    q_indicator,
                    ratatui::style::Style::default().fg(styles::YELLOW),
                ));
            }
            if has_gh_comments {
                spans.push(Span::styled(
                    gh_indicator,
                    ratatui::style::Style::default().fg(styles::CYAN),
                ));
            }
            // Show relative time when sorting by mtime
            if let Some(ref ts) = time_str {
                spans.push(Span::styled(
                    format!("{:>7} ", ts),
                    ratatui::style::Style::default().fg(styles::MUTED),
                ));
            }
            if area.width > 24 {
                spans.push(Span::styled(
                    format!("{:>8} ", stats),
                    ratatui::style::Style::default().fg(styles::DIM),
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
            "\u{2500}".repeat(dash_count),
            sep_label,
            "\u{2500}".repeat(sep_width.saturating_sub(dash_count + sep_label.len()))
        );
        items.push(ListItem::new(Line::from(Span::styled(
            format!(" {}", sep_text),
            ratatui::style::Style::default().fg(styles::WATCHED_MUTED),
        ))).style(styles::surface_style()));

        // Watched files
        for (idx, watched) in &visible_watched {
            let is_selected = tab.selected_watched == Some(*idx);
            let age = format_relative_time(watched.modified);
            let not_ignored = tab.watched_not_ignored.contains(&watched.path);

            let path = shorten_path(
                &watched.path,
                (area.width as usize).saturating_sub(16),
            );
            let path_width = (area.width as usize).saturating_sub(14).max(1);

            let line_style = if is_selected {
                styles::selected_style()
            } else {
                styles::surface_style()
            };

            let icon = if not_ignored { "\u{26a0}" } else { "\u{25c9}" };
            let icon_style = if not_ignored {
                ratatui::style::Style::default().fg(styles::YELLOW)
            } else {
                ratatui::style::Style::default().fg(styles::WATCHED_TEXT)
            };

            let mut spans = vec![
                Span::styled(format!(" {} ", icon), icon_style),
            ];

            spans.push(Span::styled(
                format!("{:<width$}", path, width = path_width),
                if is_selected {
                    styles::selected_style()
                } else {
                    ratatui::style::Style::default().fg(styles::WATCHED_TEXT)
                },
            ));
            if area.width > 24 {
                spans.push(Span::styled(
                    format!("{:>8} ", age),
                    ratatui::style::Style::default().fg(styles::WATCHED_MUTED),
                ));
            }

            items.push(ListItem::new(Line::from(spans)).style(line_style));
        }
    }

    let title_style = if in_overlay && tab.ai.has_data() && !ai_stale {
        ratatui::style::Style::default().fg(styles::PURPLE)
    } else if ai_stale {
        styles::stale_style()
    } else {
        ratatui::style::Style::default().fg(styles::MUTED)
    };

    let block = Block::default()
        .title(Span::styled(title, title_style))
        .borders(Borders::RIGHT)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER))
        .style(ratatui::style::Style::default().bg(styles::SURFACE))
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
    let subject_width = (area.width as usize).saturating_sub(indicator_width + 1).max(1);

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
                ratatui::style::Style::default().fg(styles::PURPLE)
            } else {
                ratatui::style::Style::default().fg(styles::DIM)
            };
            let subject_style = if is_selected {
                ratatui::style::Style::default().fg(styles::BRIGHT)
            } else {
                ratatui::style::Style::default().fg(styles::TEXT)
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
                        Span::styled(continuation_indent.clone(), ratatui::style::Style::default()),
                        Span::styled(segment.clone(), subject_style),
                    ]);
                    ListItem::new(line).style(line_style)
                })
                .collect();

            // Author line: indented, dimmed
            let author_line = Line::from(vec![
                Span::styled(
                    format!("   {}", commit.author),
                    ratatui::style::Style::default().fg(styles::DIM),
                ),
            ]);

            // Separator line
            let separator = Line::from(Span::styled(
                "─".repeat(area.width.saturating_sub(2) as usize),
                ratatui::style::Style::default().fg(styles::BORDER),
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
            ratatui::style::Style::default().fg(styles::MUTED),
        ))
        .borders(Borders::RIGHT)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER))
        .style(ratatui::style::Style::default().bg(styles::SURFACE))
        .padding(Padding::new(0, 0, 0, 0));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

/// Shorten a file path to fit within max_width
fn shorten_path(path: &str, max_width: usize) -> String {
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
