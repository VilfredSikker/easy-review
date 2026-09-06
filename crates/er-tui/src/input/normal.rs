use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::ai::{PanelContent, ReviewFocus};
use er_engine::app::{App, ConfirmAction, DiffMode, InputMode, SplitSide};
use er_engine::watch::{FileWatcher, WatchEvent};
use std::path::Path;
use std::sync::mpsc;

use super::sync_github_comments;

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
            app.start_comment(er_engine::ai::CommentType::Question);
            return Ok(());
        }

        // Mode switching — dynamic tab numbers based on visible modes
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as usize) - ('1' as usize);
            let visible = app.tab().visible_modes(&app.config);
            if let Some(&mode) = visible.get(idx) {
                if mode == DiffMode::PrDiff {
                    // Only enter PrDiff when not already there (avoids re-fetching refs
                    // on a remote tab that is already in PrDiff from construction).
                    if app.tab().mode != DiffMode::PrDiff {
                        if let Err(e) = app.tab_mut().enter_pr_diff() {
                            app.notify(&format!("PR diff unavailable: {}", e));
                        }
                    }
                } else {
                    app.tab_mut().set_mode(mode);
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
            let count = app.tab().ai.local_draft_count();
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
        // FileDetail: j/k navigate findings when the current file has any; fall back to scroll.
        // PrOverview / other panels: always scroll.
        let has_findings = app.tab().panel == Some(PanelContent::FileDetail)
            && app
                .tab()
                .files
                .get(app.tab().selected_file)
                .and_then(|f| app.tab().ai.file_review(&f.path))
                .is_some_and(|fr| !fr.findings.is_empty());

        match key.code {
            KeyCode::Char('j') | KeyCode::Down if has_findings => {
                app.navigate_panel_finding(true);
                return Ok(());
            }
            KeyCode::Char('k') | KeyCode::Up if has_findings => {
                app.navigate_panel_finding(false);
                return Ok(());
            }
            KeyCode::Enter if app.tab().focused_finding_id.is_some() => {
                app.jump_to_focused_finding();
                return Ok(());
            }
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
                app.tab_mut().focused_finding_id = None;
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

        // Tab: resume comment draft, or toggle split/panel focus
        KeyCode::Tab => {
            if app.has_comment_draft() {
                app.resume_comment();
                return Ok(());
            }
            if !matches!(mode, DiffMode::History | DiffMode::Tour)
                && app.split_diff_active(&app.config.clone())
            {
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
                app.start_comment(er_engine::ai::CommentType::GitHubComment);
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

        // Toggle hide resolved comments (X)
        KeyCode::Char('X') => {
            app.tab_mut().toggle_hide_resolved();
            let on = app.tab().layers.hide_resolved;
            app.notify(if on {
                "Resolved: hidden"
            } else {
                "Resolved: visible"
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
            if !matches!(mode, DiffMode::History | DiffMode::Tour)
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

        // Toggle unreviewed-only filter — not meaningful in History/Tour
        KeyCode::Char('!') if !matches!(mode, DiffMode::History | DiffMode::Tour) => {
            app.toggle_unreviewed_filter();
            return Ok(());
        }

        // Toggle reviewed — review tracking is per-branch, not meaningful in History.
        // Tour handles `space` in its own handler (operates on the tour file list).
        KeyCode::Char(' ') if !matches!(mode, DiffMode::History | DiffMode::Tour) => {
            app.toggle_reviewed()?;
            return Ok(());
        }

        // Jump to next unreviewed file — not meaningful in History/Tour
        KeyCode::Char('U')
            if !matches!(mode, DiffMode::History | DiffMode::Tour)
                && key.modifiers == KeyModifiers::NONE =>
        {
            app.next_unreviewed_file();
            return Ok(());
        }

        // Copy hub — offers full file, path, hunk, or line copy options
        KeyCode::Char('y')
            if !matches!(mode, DiffMode::History | DiffMode::Tour)
                && key.modifiers == KeyModifiers::NONE =>
        {
            app.open_copy_hub();
            return Ok(());
        }

        // Export picker — multiselect annotation kinds to clipboard
        KeyCode::Char('E')
            if !matches!(mode, DiffMode::History | DiffMode::Tour)
                && key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            app.open_export_picker();
            return Ok(());
        }

        _ => {}
    }

    // ── History mode: route to dedicated handler (pure navigation only) ──
    if mode == DiffMode::History {
        return handle_history_input(app, key);
    }

    // ── Tour mode: route to dedicated handler ──
    if mode == DiffMode::Tour {
        return handle_tour_input(app, key);
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

pub fn handle_tour_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Pillar navigation (left panel) — k next, j prev (matches History).
        KeyCode::Char('k') => app.tab_mut().tour_next_pillar(),
        KeyCode::Char('j') => app.tab_mut().tour_prev_pillar(),

        // File navigation within the tour (n/N)
        KeyCode::Char('n') => app.tab_mut().tour_next_file(),
        KeyCode::Char('N') => app.tab_mut().tour_prev_file(),

        // Line navigation (arrows)
        KeyCode::Down => app.tab_mut().tour_next_line(),
        KeyCode::Up => app.tab_mut().tour_prev_line(),

        // Horizontal scroll
        KeyCode::Char('l') | KeyCode::Right => app.tab_mut().tour_scroll_right(8),
        KeyCode::Char('h') | KeyCode::Left => app.tab_mut().tour_scroll_left(8),
        KeyCode::Home => {
            if let Some(ref mut t) = app.tab_mut().tour {
                t.h_scroll = 0;
            }
        }

        // Vertical scroll (u/d page through all files; pillars switch as you go)
        KeyCode::Char('d')
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.tab_mut().tour_scroll_down(10);
        }
        KeyCode::Char('u')
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.tab_mut().tour_scroll_up(10);
        }
        KeyCode::PageDown => app.tab_mut().tour_scroll_down(20),
        KeyCode::PageUp => app.tab_mut().tour_scroll_up(20),

        // Toggle reviewed for the current file
        KeyCode::Char(' ') => app.tab_mut().tour_toggle_reviewed(),

        // Bulk-review the whole pillar
        KeyCode::Char('b') => {
            app.tab_mut().tour_bulk_review_pillar();
            app.notify("Reviewed all files in pillar");
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{
        ChecklistItem, ErChecklist, ErFileReview, ErReview, ErTour, RiskLevel, TourFile, TourPillar,
    };
    use er_engine::git::{CommitInfo, DiffFile, DiffHunk, DiffLine, FileStatus, LineType};
    use er_engine::ErRoot;
    use std::collections::HashMap;

    // ── Fixtures ──

    fn hunk_with(new_start: usize, count: usize) -> DiffHunk {
        DiffHunk {
            header: format!("@@ -{new_start},{count} +{new_start},{count} @@"),
            old_start: new_start,
            old_count: count,
            new_start,
            new_count: count,
            lines: (0..count)
                .map(|i| DiffLine {
                    line_type: LineType::Context,
                    content: format!("line {}", new_start + i),
                    old_num: Some(new_start + i),
                    new_num: Some(new_start + i),
                })
                .collect(),
        }
    }

    fn diff_file(path: &str, hunks: Vec<DiffHunk>) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks,
            adds: 1,
            dels: 1,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    fn commit_info(hash: &str, subject: &str) -> CommitInfo {
        CommitInfo {
            hash: hash.to_string(),
            short_hash: hash.chars().take(7).collect(),
            subject: subject.to_string(),
            author: "Ada".to_string(),
            date: "2026-01-01T00:00:00Z".to_string(),
            relative_date: "1 day ago".to_string(),
            file_count: 1,
            adds: 1,
            dels: 0,
            is_merge: false,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// A throwaway repo-local `er_root` whose `.er/` directory already exists, so
    /// sidecar writes (`checklist.json`, `reviewed`) actually land instead of
    /// failing on a missing parent. Returns `(root, ErRoot)`.
    fn temp_er_root(name: &str) -> (std::path::PathBuf, ErRoot) {
        let root = std::env::temp_dir().join(format!("er-tui-normal-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".er")).unwrap();
        let root_s = root.to_string_lossy().to_string();
        (root, ErRoot::RepoLocal(root_s))
    }

    // ── handle_ai_review_input ──

    fn review_with(files: Vec<(&str, RiskLevel)>) -> ErReview {
        let mut file_map = HashMap::new();
        for (path, risk) in files {
            file_map.insert(
                path.to_string(),
                ErFileReview {
                    risk,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: Vec::new(),
                },
            );
        }
        ErReview {
            version: 1,
            diff_hash: String::new(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: file_map,
            file_hashes: HashMap::new(),
        }
    }

    fn checklist_with(items: Vec<(&str, &str)>) -> ErChecklist {
        ErChecklist {
            version: 1,
            diff_hash: String::new(),
            items: items
                .into_iter()
                .map(|(id, text)| ChecklistItem {
                    id: id.to_string(),
                    text: text.to_string(),
                    category: String::new(),
                    checked: false,
                    related_findings: Vec::new(),
                    related_files: Vec::new(),
                })
                .collect(),
        }
    }

    /// Two reviewed files (one High, one Low) and a two-item checklist.
    fn ai_review_app() -> App {
        let mut app = App::new_for_test(vec![
            diff_file("high.rs", vec![hunk_with(1, 2)]),
            diff_file("low.rs", vec![hunk_with(1, 2)]),
        ]);
        app.tab_mut().ai.review = Some(review_with(vec![
            ("high.rs", RiskLevel::High),
            ("low.rs", RiskLevel::Low),
        ]));
        app.tab_mut().ai.checklist = Some(checklist_with(vec![
            ("c-1", "Check the error path"),
            ("c-2", "Check the tests"),
        ]));
        app
    }

    #[test]
    fn ai_review_k_advances_the_cursor_and_stops_at_the_last_item() {
        let mut app = ai_review_app();
        assert_eq!(app.tab().review_focus, ReviewFocus::Files);

        handle_ai_review_input(&mut app, key(KeyCode::Char('k'))).unwrap();
        assert_eq!(app.tab().review_cursor, 1);

        handle_ai_review_input(&mut app, key(KeyCode::Down)).unwrap();
        assert_eq!(
            app.tab().review_cursor,
            1,
            "k/Down must not walk past the last of the two reviewed files"
        );
    }

    #[test]
    fn ai_review_j_walks_the_cursor_back_and_stops_at_zero() {
        let mut app = ai_review_app();
        app.tab_mut().review_cursor = 1;

        handle_ai_review_input(&mut app, key(KeyCode::Char('j'))).unwrap();
        assert_eq!(app.tab().review_cursor, 0);

        handle_ai_review_input(&mut app, key(KeyCode::Up)).unwrap();
        assert_eq!(
            app.tab().review_cursor,
            0,
            "j/Up must not underflow past the first item"
        );
    }

    #[test]
    fn ai_review_tab_switches_column_and_scrolls_the_panel_to_that_section() {
        let mut app = ai_review_app();
        app.tab_mut().review_cursor = 1;
        let (files_offset, checklist_offset) = app.tab().ai_summary_section_offsets();
        assert_ne!(
            files_offset, checklist_offset,
            "fixture guard: the two sections must sit at different panel offsets"
        );

        handle_ai_review_input(&mut app, key(KeyCode::Tab)).unwrap();

        assert_eq!(app.tab().review_focus, ReviewFocus::Checklist);
        assert_eq!(
            app.tab().review_cursor,
            0,
            "switching columns restarts the cursor at the top of the new list"
        );
        assert_eq!(app.tab().panel_scroll, checklist_offset);

        handle_ai_review_input(&mut app, key(KeyCode::Char('h'))).unwrap();

        assert_eq!(app.tab().review_focus, ReviewFocus::Files);
        assert_eq!(app.tab().panel_scroll, files_offset);
    }

    #[test]
    fn ai_review_space_checks_the_focused_checklist_item_and_persists_it() {
        let (root, er_root) = temp_er_root("checklist-toggle");
        let mut app = ai_review_app();
        app.tab_mut().er_root = er_root;
        app.tab_mut().review_focus = ReviewFocus::Checklist;
        app.tab_mut().review_cursor = 1;

        handle_ai_review_input(&mut app, key(KeyCode::Char(' '))).unwrap();

        let items = &app.tab().ai.checklist.as_ref().unwrap().items;
        assert!(!items[0].checked, "only the item under the cursor toggles");
        assert!(items[1].checked);

        let written = std::fs::read_to_string(root.join(".er").join("checklist.json"))
            .expect("space must persist checklist.json");
        assert!(
            written.contains("\"checked\": true"),
            "the checked flag reaches disk: {written}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ai_review_space_is_ignored_while_the_files_column_has_focus() {
        let (root, er_root) = temp_er_root("checklist-guard");
        let mut app = ai_review_app();
        app.tab_mut().er_root = er_root;
        assert_eq!(app.tab().review_focus, ReviewFocus::Files);

        handle_ai_review_input(&mut app, key(KeyCode::Char(' '))).unwrap();

        assert!(
            app.tab()
                .ai
                .checklist
                .as_ref()
                .unwrap()
                .items
                .iter()
                .all(|i| !i.checked),
            "space only toggles while the checklist column has focus"
        );
        assert!(
            !root.join(".er").join("checklist.json").exists(),
            "and writes nothing from the files column"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ai_review_enter_jumps_to_the_highest_risk_file() {
        let mut app = ai_review_app();
        app.tab_mut().selected_file = 1;

        handle_ai_review_input(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(
            app.tab().selected_file,
            0,
            "cursor 0 of the risk-sorted list is high.rs, which is index 0 of the diff"
        );
        assert!(
            matches!(app.tab().panel, Some(PanelContent::FileDetail)),
            "jumping opens the file detail panel"
        );
        assert_eq!(app.watch_message.as_deref(), Some("Jumped to: high.rs"));
    }

    #[test]
    fn ai_review_d_and_u_scroll_the_panel_by_ten() {
        let mut app = ai_review_app();

        handle_ai_review_input(&mut app, key(KeyCode::Char('d'))).unwrap();
        assert_eq!(app.tab().panel_scroll, 10);

        handle_ai_review_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        )
        .unwrap();
        assert_eq!(
            app.tab().panel_scroll,
            20,
            "Ctrl+d scrolls the same amount as bare d"
        );

        handle_ai_review_input(&mut app, key(KeyCode::Char('u'))).unwrap();
        assert_eq!(app.tab().panel_scroll, 10);
    }

    #[test]
    fn ai_review_page_keys_scroll_twice_as_far_as_d_and_u() {
        let mut app = ai_review_app();

        handle_ai_review_input(&mut app, key(KeyCode::PageDown)).unwrap();
        assert_eq!(app.tab().panel_scroll, 20);

        handle_ai_review_input(&mut app, key(KeyCode::PageUp)).unwrap();
        assert_eq!(app.tab().panel_scroll, 0);
    }

    #[test]
    fn ai_review_scroll_down_is_capped_at_4096() {
        let mut app = ai_review_app();
        app.tab_mut().panel_scroll = 4090;

        handle_ai_review_input(&mut app, key(KeyCode::PageDown)).unwrap();

        assert_eq!(
            app.tab().panel_scroll,
            4096,
            "ratatui does not clamp panel scroll, so the handler caps it"
        );
    }

    #[test]
    fn ai_review_scroll_up_saturates_at_the_top() {
        let mut app = ai_review_app();

        handle_ai_review_input(&mut app, key(KeyCode::Char('u'))).unwrap();

        assert_eq!(
            app.tab().panel_scroll,
            0,
            "scrolling up from the top must not underflow"
        );
    }

    #[test]
    fn ai_review_esc_releases_panel_focus() {
        let mut app = ai_review_app();
        app.tab_mut().panel_focus = true;

        handle_ai_review_input(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(!app.tab().panel_focus);
    }

    #[test]
    fn ai_review_shift_d_is_not_a_scroll_key() {
        let mut app = ai_review_app();
        app.tab_mut().panel_scroll = 12;

        handle_ai_review_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::SHIFT),
        )
        .unwrap();

        assert_eq!(
            app.tab().panel_scroll,
            12,
            "the scroll arm accepts only NONE or CONTROL"
        );
    }

    // ── handle_history_input ──

    /// A tab in History mode holding two commits and a two-file commit diff.
    ///
    /// `HistoryState` is crate-private to er-engine, so it can't be built here —
    /// `set_mode(History)` builds it for us. The repo root points at a directory
    /// that does not exist, so the `git log` inside `set_mode` fails and yields an
    /// empty state we then fill in through the (public) fields.
    ///
    /// `current_branch` is blanked on purpose: Branch → History changes the review
    /// bucket, which calls `apply_managed_root()`; an empty branch makes that
    /// return early instead of creating managed storage directories for real.
    fn history_app() -> App {
        let absent = std::env::temp_dir().join("er-tui-history-no-repo");
        let absent_s = absent.to_string_lossy().to_string();
        let mut app = App::new_for_test(vec![]);
        {
            let tab = app.tab_mut();
            tab.repo_root = absent_s.clone();
            tab.current_branch = String::new();
            tab.er_root = ErRoot::RepoLocal(absent_s);
        }
        app.tab_mut().set_mode(DiffMode::History);
        {
            let history = app
                .tab_mut()
                .history
                .as_mut()
                .expect("entering History mode builds the history state");
            history.commits = vec![
                commit_info("aaa1111", "newest"),
                commit_info("bbb2222", "older"),
            ];
            history.all_loaded = true;
            history.commit_files = vec![
                diff_file("f0.rs", vec![hunk_with(1, 3), hunk_with(20, 2)]),
                diff_file("f1.rs", vec![hunk_with(1, 2)]),
            ];
        }
        app
    }

    #[test]
    fn history_k_selects_the_next_older_commit_and_loads_its_diff() {
        let mut app = history_app();
        // Seed the LRU so the reload resolves from memory instead of shelling
        // out to git for a commit hash that does not exist.
        app.tab_mut().history.as_mut().unwrap().diff_cache.insert(
            "bbb2222".to_string(),
            vec![diff_file("older.rs", vec![hunk_with(1, 1)])],
        );

        handle_history_input(&mut app, key(KeyCode::Char('k'))).unwrap();

        let history = app.tab().history.as_ref().unwrap();
        assert_eq!(history.selected_commit, 1);
        assert_eq!(
            history
                .commit_files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            vec!["older.rs"],
            "moving commits reloads the right pane with that commit's diff"
        );
        assert_eq!(
            history.selected_file, 0,
            "the file cursor restarts inside the newly selected commit"
        );
    }

    #[test]
    fn history_k_at_the_last_commit_closes_pagination_when_the_next_page_is_empty() {
        let mut app = history_app();
        {
            let history = app.tab_mut().history.as_mut().unwrap();
            history.commits.truncate(1);
            history.all_loaded = false;
        }

        handle_history_input(&mut app, key(KeyCode::Char('k'))).unwrap();

        let history = app.tab().history.as_ref().unwrap();
        assert!(
            history.all_loaded,
            "an empty next page ends pagination instead of refetching on every k"
        );
        assert_eq!(
            history.selected_commit, 0,
            "there was no further commit to move onto"
        );
    }

    #[test]
    fn history_j_returns_to_the_newer_commit() {
        let mut app = history_app();
        {
            let history = app.tab_mut().history.as_mut().unwrap();
            history.selected_commit = 1;
            history
                .diff_cache
                .insert("aaa1111".to_string(), vec![diff_file("newer.rs", vec![])]);
        }

        handle_history_input(&mut app, key(KeyCode::Char('j'))).unwrap();

        let history = app.tab().history.as_ref().unwrap();
        assert_eq!(history.selected_commit, 0);
        assert_eq!(
            history
                .commit_files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            vec!["newer.rs"]
        );
    }

    #[test]
    fn history_n_and_shift_n_move_between_files_of_the_selected_commit() {
        let mut app = history_app();
        app.tab_mut().history.as_mut().unwrap().current_line = Some(2);

        handle_history_input(&mut app, key(KeyCode::Char('n'))).unwrap();
        {
            let history = app.tab().history.as_ref().unwrap();
            assert_eq!(history.selected_file, 1);
            assert_eq!(
                history.current_hunk, 0,
                "a new file restarts at its first hunk"
            );
            assert_eq!(
                history.current_line, None,
                "and drops the previous file's line focus"
            );
        }

        handle_history_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT),
        )
        .unwrap();

        assert_eq!(app.tab().history.as_ref().unwrap().selected_file, 0);
    }

    #[test]
    fn history_arrow_keys_walk_lines_inside_the_commit_diff() {
        let mut app = history_app();

        handle_history_input(&mut app, key(KeyCode::Down)).unwrap();
        assert_eq!(
            app.tab().history.as_ref().unwrap().current_line,
            Some(0),
            "Down with nothing selected lands on the first line"
        );

        handle_history_input(&mut app, key(KeyCode::Down)).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().current_line, Some(1));

        handle_history_input(&mut app, key(KeyCode::Up)).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().current_line, Some(0));
    }

    #[test]
    fn history_l_and_h_pan_the_commit_diff_horizontally_by_eight() {
        let mut app = history_app();

        handle_history_input(&mut app, key(KeyCode::Char('l'))).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().h_scroll, 8);

        handle_history_input(&mut app, key(KeyCode::Right)).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().h_scroll, 16);

        handle_history_input(&mut app, key(KeyCode::Char('h'))).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().h_scroll, 8);

        handle_history_input(&mut app, key(KeyCode::Left)).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().h_scroll, 0);
    }

    #[test]
    fn history_home_snaps_horizontal_scroll_back_to_the_left_edge() {
        let mut app = history_app();
        app.tab_mut().history.as_mut().unwrap().h_scroll = 40;

        handle_history_input(&mut app, key(KeyCode::Home)).unwrap();

        assert_eq!(app.tab().history.as_ref().unwrap().h_scroll, 0);
    }

    #[test]
    fn history_d_and_u_scroll_by_ten_and_page_keys_by_twenty() {
        let mut app = history_app();

        handle_history_input(&mut app, key(KeyCode::Char('d'))).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().diff_scroll, 10);

        handle_history_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        )
        .unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().diff_scroll, 0);

        handle_history_input(&mut app, key(KeyCode::PageDown)).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().diff_scroll, 20);

        handle_history_input(&mut app, key(KeyCode::PageUp)).unwrap();
        assert_eq!(app.tab().history.as_ref().unwrap().diff_scroll, 0);
    }

    #[test]
    fn history_scroll_up_saturates_at_the_top() {
        let mut app = history_app();

        handle_history_input(&mut app, key(KeyCode::Char('u'))).unwrap();

        assert_eq!(
            app.tab().history.as_ref().unwrap().diff_scroll,
            0,
            "scrolling up from the top must not underflow"
        );
    }

    #[test]
    fn history_shift_d_is_not_a_scroll_key() {
        let mut app = history_app();

        handle_history_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::SHIFT),
        )
        .unwrap();

        assert_eq!(
            app.tab().history.as_ref().unwrap().diff_scroll,
            0,
            "the scroll arm accepts only NONE or CONTROL"
        );
    }

    // ── handle_tour_input ──

    fn tour_pillar(id: &str, order: u32, path: &str) -> TourPillar {
        TourPillar {
            id: id.to_string(),
            title: format!("Pillar {id}"),
            description: String::new(),
            order,
            importance: 50,
            foundation: false,
            files: vec![TourFile {
                path: path.to_string(),
                reason: String::new(),
                finding_ids: Vec::new(),
                related: Vec::new(),
            }],
        }
    }

    /// Two single-file pillars over a two-file diff: `p0a.rs` (one 3-line hunk)
    /// in pillar 0, `p1a.rs` (one 2-line hunk) in pillar 1.
    fn tour_app() -> App {
        let mut app = App::new_for_test(vec![
            diff_file("p0a.rs", vec![hunk_with(1, 3)]),
            diff_file("p1a.rs", vec![hunk_with(1, 2)]),
        ]);
        app.tab_mut().ai.tour = Some(ErTour {
            version: 1,
            diff_hash: String::new(),
            created_at: String::new(),
            title: String::new(),
            overview: String::new(),
            pillars: vec![
                tour_pillar("p0", 0, "p0a.rs"),
                tour_pillar("p1", 1, "p1a.rs"),
            ],
        });
        app.tab_mut().rebuild_tour_state();
        app
    }

    /// `rebuild_tour_state` silently drops a pillar whose files are absent from
    /// the diff and sweeps unreferenced files into a trailing "Other changes"
    /// pillar, so a typo'd fixture path would quietly change the shape under test.
    fn assert_two_pillar_shape(app: &App) {
        let tour = app.tab().tour.as_ref().expect("tour state built");
        assert_eq!(tour.pillars.len(), 2);
        assert_eq!(tour.pillar_file_ranges, vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn tour_k_moves_to_the_next_pillar_and_lands_on_its_first_file() {
        let mut app = tour_app();
        assert_two_pillar_shape(&app);

        handle_tour_input(&mut app, key(KeyCode::Char('k'))).unwrap();

        let tour = app.tab().tour.as_ref().unwrap();
        assert_eq!(tour.selected_pillar, 1);
        assert_eq!(
            tour.selected_file, 1,
            "the new pillar's first file becomes the selection"
        );
        assert_eq!(tour.current_line, None);
    }

    #[test]
    fn tour_k_stops_at_the_last_pillar() {
        let mut app = tour_app();
        assert_two_pillar_shape(&app);
        app.tab_mut().tour.as_mut().unwrap().selected_pillar = 1;

        handle_tour_input(&mut app, key(KeyCode::Char('k'))).unwrap();

        assert_eq!(app.tab().tour.as_ref().unwrap().selected_pillar, 1);
    }

    #[test]
    fn tour_j_moves_back_to_the_previous_pillar() {
        let mut app = tour_app();
        assert_two_pillar_shape(&app);
        handle_tour_input(&mut app, key(KeyCode::Char('k'))).unwrap();

        handle_tour_input(&mut app, key(KeyCode::Char('j'))).unwrap();

        let tour = app.tab().tour.as_ref().unwrap();
        assert_eq!(tour.selected_pillar, 0);
        assert_eq!(tour.selected_file, 0);
    }

    #[test]
    fn tour_n_crosses_into_the_next_file_and_follows_its_pillar() {
        let mut app = tour_app();
        assert_two_pillar_shape(&app);

        handle_tour_input(&mut app, key(KeyCode::Char('n'))).unwrap();
        {
            let tour = app.tab().tour.as_ref().unwrap();
            assert_eq!(tour.selected_file, 1);
            assert_eq!(
                tour.selected_pillar, 1,
                "crossing a pillar boundary moves the left-list selection too"
            );
        }

        handle_tour_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT),
        )
        .unwrap();

        let tour = app.tab().tour.as_ref().unwrap();
        assert_eq!(tour.selected_file, 0);
        assert_eq!(tour.selected_pillar, 0);
    }

    #[test]
    fn tour_arrow_keys_walk_lines_inside_the_tour_diff() {
        let mut app = tour_app();

        handle_tour_input(&mut app, key(KeyCode::Down)).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().current_line, Some(0));

        handle_tour_input(&mut app, key(KeyCode::Down)).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().current_line, Some(1));

        handle_tour_input(&mut app, key(KeyCode::Up)).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().current_line, Some(0));
    }

    #[test]
    fn tour_l_and_h_pan_the_tour_diff_horizontally_by_eight() {
        let mut app = tour_app();

        handle_tour_input(&mut app, key(KeyCode::Char('l'))).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().h_scroll, 8);

        handle_tour_input(&mut app, key(KeyCode::Right)).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().h_scroll, 16);

        handle_tour_input(&mut app, key(KeyCode::Char('h'))).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().h_scroll, 8);

        handle_tour_input(&mut app, key(KeyCode::Left)).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().h_scroll, 0);
    }

    #[test]
    fn tour_home_snaps_horizontal_scroll_back_to_the_left_edge() {
        let mut app = tour_app();
        app.tab_mut().tour.as_mut().unwrap().h_scroll = 40;

        handle_tour_input(&mut app, key(KeyCode::Home)).unwrap();

        assert_eq!(app.tab().tour.as_ref().unwrap().h_scroll, 0);
    }

    #[test]
    fn tour_d_scrolls_ten_rows_and_reselects_the_pillar_under_the_cursor() {
        let mut app = tour_app();
        assert_two_pillar_shape(&app);

        handle_tour_input(&mut app, key(KeyCode::Char('d'))).unwrap();

        let tour = app.tab().tour.as_ref().unwrap();
        assert_eq!(tour.diff_scroll, 10);
        // Row layout per file: 2 header rows, then `1 + lines + 1` per hunk.
        // p0a.rs (one 3-line hunk) is 2 + 5 = 7 rows, so row 10 sits in p1a.rs.
        assert_eq!(
            tour.selected_pillar, 1,
            "free-scrolling past the first file re-selects the pillar now on screen"
        );
    }

    #[test]
    fn tour_u_scrolls_back_and_reselects_the_first_pillar() {
        let mut app = tour_app();
        assert_two_pillar_shape(&app);
        handle_tour_input(&mut app, key(KeyCode::Char('d'))).unwrap();

        handle_tour_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        )
        .unwrap();

        let tour = app.tab().tour.as_ref().unwrap();
        assert_eq!(tour.diff_scroll, 0);
        assert_eq!(tour.selected_pillar, 0);
    }

    #[test]
    fn tour_page_keys_scroll_twice_as_far_as_d_and_u() {
        let mut app = tour_app();

        handle_tour_input(&mut app, key(KeyCode::PageDown)).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().diff_scroll, 20);

        handle_tour_input(&mut app, key(KeyCode::PageUp)).unwrap();
        assert_eq!(app.tab().tour.as_ref().unwrap().diff_scroll, 0);
    }

    #[test]
    fn tour_space_toggles_reviewed_for_the_selected_file() {
        let (root, er_root) = temp_er_root("tour-reviewed");
        let mut app = tour_app();
        app.tab_mut().er_root = er_root;

        handle_tour_input(&mut app, key(KeyCode::Char(' '))).unwrap();
        assert_eq!(
            app.tab()
                .reviewed
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["p0a.rs"]
        );

        handle_tour_input(&mut app, key(KeyCode::Char(' '))).unwrap();
        assert!(
            app.tab().reviewed.is_empty(),
            "space is a toggle, not a one-way mark"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tour_b_bulk_reviews_only_the_selected_pillar() {
        let (root, er_root) = temp_er_root("tour-bulk");
        let mut app = tour_app();
        app.tab_mut().er_root = er_root;
        assert_two_pillar_shape(&app);

        handle_tour_input(&mut app, key(KeyCode::Char('b'))).unwrap();

        let marked: Vec<&str> = app.tab().reviewed.keys().map(String::as_str).collect();
        assert_eq!(
            marked,
            vec!["p0a.rs"],
            "the second pillar's file stays unreviewed"
        );
        assert_eq!(
            app.watch_message.as_deref(),
            Some("Reviewed all files in pillar")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tour_shift_d_is_not_a_scroll_key() {
        let mut app = tour_app();

        handle_tour_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::SHIFT),
        )
        .unwrap();

        assert_eq!(
            app.tab().tour.as_ref().unwrap().diff_scroll,
            0,
            "the scroll arm accepts only NONE or CONTROL"
        );
    }
}
