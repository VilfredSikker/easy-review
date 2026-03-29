use crate::ai::{PanelContent, ReviewFocus};
use crate::app::{App, ConfirmAction, DiffMode, InputMode, SplitSide};
use crate::watch::{FileWatcher, WatchEvent};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use std::sync::mpsc;

use super::quiz::handle_quiz_input;
use super::sync_github_comments;
use super::wizard::lookup_wizard_symbol_refs;

pub fn handle_normal_input(
    app: &mut App,
    key: KeyEvent,
    watch_tx: &mpsc::Sender<WatchEvent>,
    watcher: &mut Option<FileWatcher>,
) -> Result<()> {
    // ── Global keys: work in all view modes including AiReview ──

    match key.code {
        // Quit (Ctrl+q)
        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
            return Ok(());
        }

        // Personal question on current line (q)
        KeyCode::Char('q') => {
            app.start_comment(crate::ai::CommentType::Question);
            return Ok(());
        }

        // Mode switching — dynamic tab numbers based on visible modes
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as usize) - ('1' as usize);
            let visible = app.tab().visible_modes(&app.config);
            if let Some(&mode) = visible.get(idx) {
                app.tab_mut().set_mode(mode);
            } else if !app.tab().is_remote() {
                // Show helpful messages for modes that need data files
                if app.config.features.view_wizard && app.tab().ai.wizard.is_none() {
                    app.notify("Wizard requires .er/wizard.json — run /er-wizard first");
                } else if app.config.features.view_quiz && app.tab().ai.quiz.is_none() {
                    app.notify("Quiz requires .er/quiz.json — run /er-quiz first");
                }
            }
            return Ok(());
        }
        // Toggle mtime sort (works in any mode)
        KeyCode::Char('m') => {
            let tab = app.tab_mut();
            tab.sort_by_mtime = !tab.sort_by_mtime;
            let _ = tab.refresh_diff();
            let label = if app.tab().sort_by_mtime {
                "Sort: recent first"
            } else {
                "Sort: default"
            };
            app.notify(label);
            return Ok(());
        }

        // Reload/refresh diff
        KeyCode::Char('R') => {
            app.tab_mut().refresh_diff()?;
            app.notify("Refreshed");
            return Ok(());
        }

        // Toggle watch mode
        KeyCode::Char('w') => {
            if app.tab().is_remote() {
                app.notify("Watch not available in remote mode");
                return Ok(());
            }
            if app.watching {
                *watcher = None;
                app.watching = false;
                app.notify("Watch stopped");
            } else {
                let root_str = app.tab().repo_root.clone();
                let root = Path::new(&root_str);
                match FileWatcher::new(root, 500, watch_tx.clone()) {
                    Ok(w) => {
                        *watcher = Some(w);
                        app.watching = true;
                        app.notify("Watching for changes...");
                    }
                    Err(e) => {
                        app.notify(&format!("Watch error: {}", e));
                    }
                }
            }
            return Ok(());
        }

        // Open in editor (or edit focused comment if own top-level)
        KeyCode::Char('e') => {
            if let Some(id) = app.tab().focused_comment_id.clone() {
                if let Some(comment) = app.tab().ai.find_comment(&id) {
                    if comment.author() == "You" && comment.in_reply_to().is_none() {
                        app.start_edit_comment(&id);
                        return Ok(());
                    }
                }
            }
            if app.tab().is_remote() {
                app.notify("Editor not available in remote mode");
            } else {
                app.tab().open_in_editor()?;
            }
            return Ok(());
        }

        // Unified hint jumping across files (Shift+J / Shift+K)
        KeyCode::Char('J') => {
            app.prev_hint();
            return Ok(());
        }
        KeyCode::Char('K') => {
            app.next_hint();
            return Ok(());
        }
        // AI finding jumping across files (Ctrl+j / Ctrl+k)
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.next_finding();
            return Ok(());
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.prev_finding();
            return Ok(());
        }
        // Delete watched file in Hidden mode
        KeyCode::Char('x')
            if key.modifiers == KeyModifiers::NONE && app.tab().mode == DiffMode::Hidden =>
        {
            if let Some(idx) = app.tab().selected_watched {
                if let Some(wf) = app.tab().watched_files.get(idx) {
                    let path = wf.path.clone();
                    app.input_mode = InputMode::Confirm(ConfirmAction::DeleteWatchedFile { path });
                }
            }
            return Ok(());
        }
        // Delete focused comment (after J/K jump) — only if deletable
        KeyCode::Char('x')
            if key.modifiers == KeyModifiers::NONE && app.tab().focused_comment_id.is_some() =>
        {
            if let Some(ref id) = app.tab().focused_comment_id.clone() {
                if let Some(comment) = app.tab().ai.find_comment(id) {
                    if comment.can_delete() {
                        app.input_mode = InputMode::Confirm(ConfirmAction::DeleteComment {
                            comment_id: id.clone(),
                        });
                    }
                }
            }
            return Ok(());
        }
        // Wizard mode: 'r' looks up symbol references for word under cursor
        KeyCode::Char('r') if app.tab().mode == DiffMode::Wizard => {
            lookup_wizard_symbol_refs(app);
            return Ok(());
        }

        // Reply to focused comment/question or finding
        KeyCode::Char('r') => {
            if let Some(id) = app.tab().focused_comment_id.clone() {
                if let Some(comment) = app.tab().ai.find_comment(&id) {
                    if comment.can_reply() {
                        app.start_reply_comment(&id);
                    }
                }
            } else if let Some(id) = app.tab().focused_finding_id.clone() {
                app.start_reply_finding(&id);
            }
            return Ok(());
        }
        // Cleanup AI sidecar files
        KeyCode::Char('z') if key.modifiers == KeyModifiers::NONE => {
            if app.tab().is_remote() {
                return Ok(());
            }
            let count = app
                .tab()
                .ai
                .questions
                .as_ref()
                .map_or(0, |q| q.questions.len());
            app.input_mode = InputMode::Confirm(ConfirmAction::CleanupQuestions { count });
            return Ok(());
        }
        KeyCode::Char('Z') => {
            if app.tab().is_remote() {
                return Ok(());
            }
            let count = app.tab().ai.review.as_ref().map_or(0, |r| r.files.len());
            app.input_mode = InputMode::Confirm(ConfirmAction::CleanupReviews { count });
            return Ok(());
        }
        KeyCode::Char('x') => {
            app.close_tab();
            return Ok(());
        }
        // Tab switching ([ / ])
        KeyCode::Char(']') => {
            app.next_tab();
            return Ok(());
        }
        KeyCode::Char('[') => {
            app.prev_tab();
            return Ok(());
        }

        // Repo overlays
        KeyCode::Char('o') => {
            app.open_open_hub();
            return Ok(());
        }

        // Toggle watched files section visibility
        KeyCode::Char('W') => {
            let tab = app.tab_mut();
            if tab.watched_config.paths.is_empty() {
                app.notify("No watched paths in .er-config.toml");
            } else {
                tab.show_watched = !tab.show_watched;
                if tab.show_watched {
                    tab.refresh_watched_files();
                    app.notify("Watched files shown");
                } else {
                    tab.watched_files.clear();
                    tab.selected_watched = None;
                    app.notify("Watched files hidden");
                }
            }
            return Ok(());
        }

        // Resize file tree panel (</>)
        KeyCode::Char('<') => {
            let w = app.last_terminal_width;
            app.tab_mut().resize_file_tree(-2, w);
            return Ok(());
        }
        KeyCode::Char('>') => {
            let w = app.last_terminal_width;
            app.tab_mut().resize_file_tree(2, w);
            return Ok(());
        }

        // Resize side panel ({/})
        KeyCode::Char('{') => {
            let w = app.last_terminal_width;
            app.tab_mut().resize_panel(-4, w);
            return Ok(());
        }
        KeyCode::Char('}') => {
            let w = app.last_terminal_width;
            app.tab_mut().resize_panel(4, w);
            return Ok(());
        }

        _ => {}
    }

    // ── Panel focused: route navigation keys to the appropriate panel handler ──
    if app.tab().panel_focus && app.tab().panel.is_some() {
        if app.tab().panel == Some(PanelContent::AiSummary) {
            return handle_ai_review_input(app, key);
        }
        // FileDetail / PrOverview panels: route j/k and arrow keys to panel scrolling
        match key.code {
            KeyCode::Char('k') | KeyCode::Down => {
                app.tab_mut().panel_scroll_down(1);
                app.tab_mut().panel_scroll = app.tab().panel_scroll.min(4096);
                return Ok(());
            }
            KeyCode::Char('j') | KeyCode::Up => {
                app.tab_mut().panel_scroll_up(1);
                return Ok(());
            }
            KeyCode::Esc => {
                app.tab_mut().panel_focus = false;
                return Ok(());
            }
            _ => {}
        }
    }

    // ── Shared feature keys: work in all diff modes including History ──
    let mode = app.tab().mode;

    match key.code {
        // Search
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Search;
            let tab = app.tab_mut();
            tab.search_query.clear();
            tab.search_query_lower.clear();
            return Ok(());
        }

        // Filter
        KeyCode::Char('f') => {
            app.input_mode = InputMode::Filter;
            // Pre-populate with current expression for editing
            app.tab_mut().filter_input = app.tab().filter_expr.clone();
            return Ok(());
        }

        // Filter history
        KeyCode::Char('F') => {
            app.open_filter_history();
            return Ok(());
        }

        // Open config hub overlay
        KeyCode::Char(',') => {
            app.open_config_hub();
            return Ok(());
        }

        // Toggle AI findings layer (A)
        KeyCode::Char('A') => {
            app.tab_mut().toggle_layer_ai();
            let on = app.tab().layers.show_ai_findings;
            app.notify(if on {
                "AI findings: ON"
            } else {
                "AI findings: OFF"
            });
            return Ok(());
        }

        // Push current branch to remote (Staged mode only)
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.tab().mode == DiffMode::Staged && !app.tab().is_remote() {
                app.input_mode = InputMode::Confirm(ConfirmAction::Push);
            }
            return Ok(());
        }

        // Toggle context panel (p) — cycles through panel states
        KeyCode::Char('p') => {
            app.tab_mut().toggle_panel();
            return Ok(());
        }

        // Tab: in History mode only toggle panel focus; in other modes also handle split pane
        KeyCode::Tab => {
            if mode != DiffMode::History && app.split_diff_active(&app.config.clone()) {
                let tab = app.tab_mut();
                tab.split_focus = match tab.split_focus {
                    SplitSide::Old => SplitSide::New,
                    SplitSide::New => SplitSide::Old,
                };
            } else {
                let tab = app.tab_mut();
                if tab.panel.is_some() {
                    tab.panel_focus = !tab.panel_focus;
                }
            }
            return Ok(());
        }

        // In Staged mode, c = commit; otherwise c = GitHub comment
        KeyCode::Char('c') => {
            if app.tab().mode == DiffMode::Staged {
                app.start_commit();
            } else {
                app.start_comment(crate::ai::CommentType::GitHubComment);
            }
            return Ok(());
        }

        // Toggle comment layer visibility (C)
        KeyCode::Char('C') => {
            app.tab_mut().toggle_layer_comments();
            let on = app.tab().layers.show_github_comments;
            app.notify(if on {
                "Comments: visible"
            } else {
                "Comments: hidden"
            });
            return Ok(());
        }

        // Toggle question layer visibility (Q)
        KeyCode::Char('Q') => {
            app.tab_mut().toggle_layer_questions();
            let on = app.tab().layers.show_questions;
            app.notify(if on {
                "Questions: visible"
            } else {
                "Questions: hidden"
            });
            return Ok(());
        }

        // GitHub comment sync (pull)
        KeyCode::Char('G') => {
            sync_github_comments(app)?;
            return Ok(());
        }

        // Toggle context panel backward (P) — cycles through panel states in reverse
        KeyCode::Char('P') => {
            app.tab_mut().toggle_panel_reverse();
            return Ok(());
        }

        // AI modal hub (a)
        KeyCode::Char('a') => {
            app.open_ai_hub();
            return Ok(());
        }

        // Git modal hub (g)
        KeyCode::Char('g') => {
            app.open_git_hub();
            return Ok(());
        }

        // Verify modal hub (v)
        KeyCode::Char('v') => {
            app.open_verify_hub();
            return Ok(());
        }

        // Help modal hub (?)
        KeyCode::Char('?') => {
            app.open_help_hub();
            return Ok(());
        }

        // Expand/compact toggle for compacted files (no-op in History — commit files aren't compacted)
        KeyCode::Enter => {
            let is_compacted = app.tab().selected_diff_file().is_some_and(|f| f.compacted);
            if is_compacted {
                app.tab_mut().toggle_compacted()?;
            }
            return Ok(());
        }

        // Expand / collapse context lines for current file
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if app.tab().is_remote() {
                app.notify("Context expansion not available in remote mode");
                return Ok(());
            }
            app.tab_mut().expand_context()?;
            return Ok(());
        }
        KeyCode::Char('-') => {
            if app.tab().is_remote() {
                app.notify("Context expansion not available in remote mode");
                return Ok(());
            }
            app.tab_mut().collapse_context()?;
            return Ok(());
        }

        // Clear search first, then filter (History gains filter-clear, which is correct)
        KeyCode::Esc => {
            if !app.tab().search_query.is_empty() {
                let tab = app.tab_mut();
                tab.search_query.clear();
                tab.search_query_lower.clear();
            } else if !app.tab().filter_expr.is_empty() {
                app.tab_mut().clear_filter();
                app.notify("Filter cleared");
            }
            return Ok(());
        }

        // Stage/unstage file (or update snapshot for watched files) — not meaningful in History or remote mode
        KeyCode::Char('s')
            if mode != DiffMode::History
                && !app.tab().is_remote()
                && key.modifiers == KeyModifiers::NONE =>
        {
            if app.tab().selected_watched.is_some() {
                // Update snapshot for watched file
                if app.tab().watched_config.diff_mode == "snapshot" {
                    match app.tab_mut().update_watched_snapshot() {
                        Ok(()) => app.notify("Snapshot updated"),
                        Err(e) => app.notify(&format!("Snapshot error: {}", e)),
                    }
                } else {
                    app.notify("Snapshot mode not enabled (diff_mode = \"content\")");
                }
            } else {
                app.toggle_stage_file()?;
            }
            return Ok(());
        }

        // Toggle unreviewed-only filter — not meaningful in History
        KeyCode::Char('!') if mode != DiffMode::History => {
            app.toggle_unreviewed_filter();
            return Ok(());
        }

        // Wizard mode: Space marks reviewed and advances to next unreviewed file
        KeyCode::Char(' ') if mode == DiffMode::Wizard => {
            app.tab_mut().wizard_mark_reviewed();
            return Ok(());
        }

        // Wizard mode: 'i' toggles showing hidden Info files
        KeyCode::Char('i') if mode == DiffMode::Wizard => {
            if let Some(ref mut wizard) = app.tab_mut().wizard {
                wizard.show_hidden = !wizard.show_hidden;
            }
            return Ok(());
        }

        // Toggle reviewed — review tracking is per-branch, not meaningful in History
        KeyCode::Char(' ') if mode != DiffMode::History => {
            app.toggle_reviewed()?;
            return Ok(());
        }

        // Jump to next unreviewed file — not meaningful in History
        KeyCode::Char('U') if mode != DiffMode::History && key.modifiers == KeyModifiers::NONE => {
            app.next_unreviewed_file();
            return Ok(());
        }

        // Copy hub — offers full file, path, hunk, or line copy options
        KeyCode::Char('y') if mode != DiffMode::History && key.modifiers == KeyModifiers::NONE => {
            app.open_copy_hub();
            return Ok(());
        }

        _ => {}
    }

    // ── History mode: route to dedicated handler (pure navigation only) ──
    if mode == DiffMode::History {
        return handle_history_input(app, key);
    }

    // ── Quiz mode: route to dedicated handler ──
    if mode == DiffMode::Quiz {
        return handle_quiz_input(app, key);
    }

    // ── Non-History navigation keys ──

    match key.code {
        // File navigation
        KeyCode::Char('j') => {
            app.tab_mut().prev_file();
            let threshold = app.config.display.auto_context_threshold;
            app.tab_mut().maybe_auto_expand_context(threshold);
        }
        KeyCode::Char('k') => {
            app.tab_mut().next_file();
            let threshold = app.config.display.auto_context_threshold;
            app.tab_mut().maybe_auto_expand_context(threshold);
        }

        // Line/comment navigation (arrow keys: comments when focused, else lines)
        // Shift+arrow extends selection, plain arrow clears it
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let tab = app.tab_mut();
            if tab.selection_anchor.is_none() {
                tab.selection_anchor = tab.current_line.or(Some(0));
            }
            let total_lines = tab.current_hunk_line_count();
            if total_lines > 0 {
                match tab.current_line {
                    None => {
                        tab.current_line = Some(0);
                        tab.scroll_to_current_hunk();
                    }
                    Some(line) => {
                        if line + 1 < total_lines {
                            tab.current_line = Some(line + 1);
                            tab.scroll_to_current_hunk();
                        }
                    }
                }
            }
        }
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let tab = app.tab_mut();
            if tab.selection_anchor.is_none() {
                tab.selection_anchor = tab.current_line.or(Some(0));
            }
            match tab.current_line {
                None => {}
                Some(0) => {}
                Some(line) => {
                    tab.current_line = Some(line - 1);
                    tab.scroll_to_current_hunk();
                }
            }
        }
        KeyCode::Down => {
            app.tab_mut().next_line();
        }
        KeyCode::Up => {
            app.tab_mut().prev_line();
        }

        // Hunk navigation
        KeyCode::Char('n') => app.tab_mut().next_hunk(),
        KeyCode::Char('N') => app.tab_mut().prev_hunk(),

        // Horizontal scroll (for long lines)
        KeyCode::Char('l') | KeyCode::Right => {
            if app.split_diff_active(&app.config.clone()) {
                app.tab_mut().scroll_right_split();
            } else {
                app.tab_mut().scroll_right(8);
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            if app.split_diff_active(&app.config.clone()) {
                app.tab_mut().scroll_left_split();
            } else {
                app.tab_mut().scroll_left(8);
            }
        }
        KeyCode::Home => {
            if app.split_diff_active(&app.config.clone()) {
                let tab = app.tab_mut();
                match tab.split_focus {
                    SplitSide::Old => tab.h_scroll_old = 0,
                    SplitSide::New => tab.h_scroll_new = 0,
                }
            }
            app.tab_mut().h_scroll = 0;
        }

        // Scroll — routes to panel when panel is focused
        KeyCode::Char('d')
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_down(10);
                app.tab_mut().panel_scroll = app.tab().panel_scroll.min(4096);
            } else {
                app.tab_mut().scroll_down(10);
            }
        }
        KeyCode::Char('u')
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_up(10);
            } else {
                app.tab_mut().scroll_up(10);
            }
        }
        KeyCode::PageDown => {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_down(20);
                app.tab_mut().panel_scroll = app.tab().panel_scroll.min(4096);
            } else {
                app.tab_mut().scroll_down(20);
            }
        }
        KeyCode::PageUp => {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_up(20);
            } else {
                app.tab_mut().scroll_up(20);
            }
        }

        _ => {}
    }
    Ok(())
}

pub fn handle_ai_review_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Navigation within focused column
        KeyCode::Char('k') | KeyCode::Down => {
            app.tab_mut().review_next();
        }
        KeyCode::Char('j') | KeyCode::Up => {
            app.tab_mut().review_prev();
        }

        // Switch focus between left/right columns
        KeyCode::Tab
        | KeyCode::Char('l')
        | KeyCode::Right
        | KeyCode::BackTab
        | KeyCode::Char('h')
        | KeyCode::Left => {
            app.tab_mut().review_toggle_focus();
            let (files_offset, checklist_offset) = app.tab().ai_summary_section_offsets();
            app.tab_mut().panel_scroll = match app.tab().review_focus {
                ReviewFocus::Files => files_offset,
                ReviewFocus::Checklist => checklist_offset,
            };
        }

        // Toggle checklist item
        KeyCode::Char(' ') => {
            app.review_toggle_checklist()?;
        }

        // Jump to file
        KeyCode::Enter => {
            app.review_jump_to_file();
        }

        // Scroll — routes to focused column's scroll offset
        KeyCode::Char('d')
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            ai_review_scroll(app, 10, true);
        }
        KeyCode::Char('u')
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            ai_review_scroll(app, 10, false);
        }
        KeyCode::PageDown => ai_review_scroll(app, 20, true),
        KeyCode::PageUp => ai_review_scroll(app, 20, false),

        // Esc closes panel focus
        KeyCode::Esc => {
            app.tab_mut().panel_focus = false;
        }

        _ => {}
    }
    Ok(())
}

pub fn handle_history_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Commit navigation (left panel)
        KeyCode::Char('k') => {
            // Check if at the end and need to load more
            let at_end = app
                .tab()
                .history
                .as_ref()
                .map(|h| h.selected_commit + 1 >= h.commits.len())
                .unwrap_or(false);
            if at_end {
                app.tab_mut().history_load_more();
            }
            app.tab_mut().history_next_commit();
        }
        KeyCode::Char('j') => {
            app.tab_mut().history_prev_commit();
        }

        // File navigation within commit diff (n/N)
        KeyCode::Char('n') => app.tab_mut().history_next_file(),
        KeyCode::Char('N') => app.tab_mut().history_prev_file(),

        // Line navigation (arrows)
        KeyCode::Down => app.tab_mut().history_next_line(),
        KeyCode::Up => app.tab_mut().history_prev_line(),

        // Horizontal scroll
        KeyCode::Char('l') | KeyCode::Right => app.tab_mut().history_scroll_right(8),
        KeyCode::Char('h') | KeyCode::Left => app.tab_mut().history_scroll_left(8),
        KeyCode::Home => {
            if let Some(ref mut h) = app.tab_mut().history {
                h.h_scroll = 0;
            }
        }

        // Scroll
        KeyCode::Char('d')
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.tab_mut().history_scroll_down(10);
        }
        KeyCode::Char('u')
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.tab_mut().history_scroll_up(10);
        }
        KeyCode::PageDown => app.tab_mut().history_scroll_down(20),
        KeyCode::PageUp => app.tab_mut().history_scroll_up(20),

        _ => {}
    }
    Ok(())
}

pub(super) fn ai_review_scroll(app: &mut App, amount: u16, down: bool) {
    let tab = app.tab_mut();
    if down {
        // Cap at 4096 — panel content is never this long and ratatui does not
        // clamp scroll internally (it renders blank lines past the content end).
        tab.panel_scroll = tab.panel_scroll.saturating_add(amount).min(4096);
    } else {
        tab.panel_scroll = tab.panel_scroll.saturating_sub(amount);
    }
}
