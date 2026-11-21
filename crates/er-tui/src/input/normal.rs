use super::keys::{AiSummaryAction, HistoryAction, NormalAction, TourAction};
use anyhow::Result;
use er_engine::ai::ReviewFocus;
use er_engine::app::{App, ConfirmAction, DiffMode, InputMode, SplitSide};
use er_engine::watch::{FileWatcher, WatchEvent};
use std::path::Path;
use std::sync::mpsc;

#[cfg(test)]
pub fn handle_normal_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    watch_tx: &mpsc::Sender<WatchEvent>,
    watcher: &mut Option<FileWatcher>,
) -> Result<()> {
    super::execute(app, super::step(app, key), watch_tx, watcher)
}

pub fn execute(
    app: &mut App,
    action: NormalAction,
    watch_tx: &mpsc::Sender<WatchEvent>,
    watcher: &mut Option<FileWatcher>,
) -> Result<()> {
    use NormalAction as A;
    // ── Global keys: work in all view modes including AiReview ──

    match action {
        // Quit (Ctrl+q)
        A::Quit => {
            app.should_quit = true;
            return Ok(());
        }

        // Personal question on current line (q)
        A::StartQuestion => {
            app.start_comment(er_engine::ai::CommentType::Question);
            return Ok(());
        }

        // Mode switching — dynamic tab numbers based on visible modes
        A::SwitchMode(mode) => {
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
            return Ok(());
        }
        // Toggle mtime sort (works in any mode)
        A::ToggleMtimeSort => {
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
        A::Refresh => {
            app.tab_mut().refresh_diff()?;
            app.notify("Refreshed");
            return Ok(());
        }

        // Toggle watch mode
        A::ToggleWatch => {
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
        A::Edit => {
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
        A::PrevHint => {
            app.prev_hint();
            return Ok(());
        }
        A::NextHint => {
            app.next_hint();
            return Ok(());
        }
        // AI finding jumping across files (Ctrl+j / Ctrl+k)
        A::NextFinding => {
            app.next_finding();
            return Ok(());
        }
        A::PrevFinding => {
            app.prev_finding();
            return Ok(());
        }
        // Delete watched file in Hidden mode
        A::AskDeleteWatchedFile => {
            if let Some(idx) = app.tab().selected_watched {
                if let Some(wf) = app.tab().watched_files.get(idx) {
                    let path = wf.path.clone();
                    app.input_mode = InputMode::Confirm(ConfirmAction::DeleteWatchedFile { path });
                }
            }
            return Ok(());
        }
        // Delete focused comment (after J/K jump) — only if deletable
        A::AskDeleteComment => {
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
        A::Reply => {
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
        A::AskClearQuestions => {
            if app.tab().is_remote() {
                return Ok(());
            }
            let count = app.tab().ai.local_draft_count();
            app.input_mode = InputMode::Confirm(ConfirmAction::CleanupQuestions { count });
            return Ok(());
        }
        A::AskClearReviews => {
            if app.tab().is_remote() {
                return Ok(());
            }
            let count = app.tab().ai.review.as_ref().map_or(0, |r| r.files.len());
            app.input_mode = InputMode::Confirm(ConfirmAction::CleanupReviews { count });
            return Ok(());
        }
        A::CloseTab => {
            app.close_tab();
            return Ok(());
        }
        // Tab switching ([ / ])
        A::NextTab => {
            app.next_tab();
            return Ok(());
        }
        A::PrevTab => {
            app.prev_tab();
            return Ok(());
        }

        // Repo overlays
        A::OpenOpenHub => {
            app.open_open_hub();
            return Ok(());
        }

        // Toggle watched files section visibility
        A::ToggleWatchedFiles => {
            let tab = app.tab_mut();
            if tab.watched_config.paths.is_empty() {
                app.notify("No watched paths configured — add them in the config hub (,)");
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
        A::ResizeFileTree(delta) => {
            let w = app.last_terminal_width;
            app.tab_mut().resize_file_tree(delta, w);
            return Ok(());
        }

        // Resize side panel ({/})
        A::ResizePanel(delta) => {
            let w = app.last_terminal_width;
            app.tab_mut().resize_panel(delta, w);
            return Ok(());
        }

        // ── Panel focused: FileDetail's j/k navigate findings when the current
        // file has any, and scroll otherwise; PrOverview and the rest scroll ──
        A::NavigatePanelFinding(forward) => {
            app.navigate_panel_finding(forward);
            return Ok(());
        }
        A::JumpToFocusedFinding => {
            app.jump_to_focused_finding();
            return Ok(());
        }
        A::PanelLineDown => {
            app.tab_mut().panel_scroll_down(1);
            app.tab_mut().panel_scroll = app.tab().panel_scroll.min(4096);
            return Ok(());
        }
        A::PanelLineUp => {
            app.tab_mut().panel_scroll_up(1);
            return Ok(());
        }
        A::UnfocusPanel => {
            app.tab_mut().panel_focus = false;
            app.tab_mut().focused_finding_id = None;
            return Ok(());
        }

        // ── Shared feature keys: work in all diff modes including History ──

        // Search
        A::StartSearch => {
            app.input_mode = InputMode::Search;
            let tab = app.tab_mut();
            tab.search_query.clear();
            tab.search_query_lower.clear();
            return Ok(());
        }

        // Filter
        A::StartFilter => {
            app.input_mode = InputMode::Filter;
            // Pre-populate with current expression for editing
            app.tab_mut().filter_input = app.tab().filter_expr.clone();
            return Ok(());
        }

        // Filter history
        A::OpenFilterHistory => {
            app.open_filter_history();
            return Ok(());
        }

        // Open config hub overlay
        A::OpenConfigHub => {
            app.open_config_hub();
            return Ok(());
        }

        // Toggle AI findings layer (A)
        A::ToggleAiFindings => {
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
        A::AskPush => {
            if app.tab().mode == DiffMode::Staged && !app.tab().is_remote() {
                app.input_mode = InputMode::Confirm(ConfirmAction::Push);
            }
            return Ok(());
        }

        // Toggle context panel (p) — cycles through panel states
        A::TogglePanel => {
            app.tab_mut().toggle_panel();
            return Ok(());
        }

        // Tab: resume comment draft, or toggle split/panel focus
        A::ResumeComment => {
            app.resume_comment();
            return Ok(());
        }
        A::ToggleSplitFocus => {
            let tab = app.tab_mut();
            tab.split_focus = match tab.split_focus {
                SplitSide::Old => SplitSide::New,
                SplitSide::New => SplitSide::Old,
            };
            return Ok(());
        }
        A::TogglePanelFocus => {
            let tab = app.tab_mut();
            if tab.panel.is_some() {
                tab.panel_focus = !tab.panel_focus;
            }
            return Ok(());
        }

        // In Staged mode, c = commit; otherwise c = GitHub comment. A tab
        // already probed with no PR gates the composer on that held state.
        A::StartCommit => {
            app.start_commit();
            return Ok(());
        }
        A::StartGithubComment => {
            if app.tab().pr_absent {
                app.notify(er_engine::github_post::NO_PR_FOR_COMMENT);
            } else {
                app.start_comment(er_engine::ai::CommentType::GitHubComment);
            }
            return Ok(());
        }

        // Toggle comment layer visibility (C)
        A::ToggleComments => {
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
        A::ToggleQuestions => {
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
        A::ToggleHideResolved => {
            app.tab_mut().toggle_hide_resolved();
            let on = app.tab().layers.hide_resolved;
            app.notify(if on {
                "Resolved: hidden"
            } else {
                "Resolved: visible"
            });
            return Ok(());
        }

        // Cycle the confidence gate (T). Tightening it is only meaningful once
        // an arbiter has graded the diff, so the message says which mode the
        // review is in rather than leaving the behaviour change silent.
        A::CycleMinTrust => {
            let graded = app.tab().ai.arbiter_effect.graded();
            let next = app.tab_mut().cycle_min_trust();
            app.notify(&format!(
                "Confidence: {} — {}",
                super::min_trust_label(next),
                if graded {
                    "arbiter-graded"
                } else {
                    "self-reported grades, nothing graded yet"
                }
            ));
            return Ok(());
        }

        // GitHub comment sync (pull)
        A::SyncGithubComments => {
            app.sync_github_comments()?;
            return Ok(());
        }

        // Toggle context panel backward (P) — cycles through panel states in reverse
        A::TogglePanelReverse => {
            app.tab_mut().toggle_panel_reverse();
            return Ok(());
        }

        // AI modal hub (a)
        A::OpenAiHub => {
            app.open_ai_hub();
            return Ok(());
        }

        // Git modal hub (g)
        A::OpenGitHub => {
            app.open_git_hub();
            return Ok(());
        }

        // Verify modal hub (v)
        A::OpenVerifyHub => {
            app.open_verify_hub();
            return Ok(());
        }

        // Help modal hub (?)
        A::OpenHelpHub => {
            app.open_help_hub();
            return Ok(());
        }

        // Expand/compact toggle for compacted files (no-op in History — commit files aren't compacted)
        A::ToggleCompacted => {
            app.tab_mut().toggle_compacted()?;
            return Ok(());
        }

        // Expand / collapse context lines for current file
        A::ExpandContext => {
            if app.tab().is_remote() {
                app.notify("Context expansion not available in remote mode");
                return Ok(());
            }
            app.tab_mut().expand_context()?;
            return Ok(());
        }
        A::CollapseContext => {
            if app.tab().is_remote() {
                app.notify("Context expansion not available in remote mode");
                return Ok(());
            }
            app.tab_mut().collapse_context()?;
            return Ok(());
        }

        // Clear search first, then filter (History gains filter-clear, which is correct)
        A::ClearSearch => {
            let tab = app.tab_mut();
            tab.search_query.clear();
            tab.search_query_lower.clear();
            return Ok(());
        }
        A::ClearFilter => {
            app.tab_mut().clear_filter();
            app.notify("Filter cleared");
            return Ok(());
        }

        // Stage/unstage file (or update snapshot for watched files) — not meaningful in History or remote mode
        A::UpdateWatchedSnapshot => {
            if app.tab().watched_config.diff_mode == "snapshot" {
                match app.tab_mut().update_watched_snapshot() {
                    Ok(()) => app.notify("Snapshot updated"),
                    Err(e) => app.notify(&format!("Snapshot error: {}", e)),
                }
            } else {
                app.notify("Snapshot mode not enabled (diff_mode = \"content\")");
            }
            return Ok(());
        }
        A::ToggleStaged => {
            app.toggle_stage_file()?;
            return Ok(());
        }

        // Toggle unreviewed-only filter — not meaningful in History/Tour
        A::ToggleUnreviewedOnly => {
            app.toggle_unreviewed_filter();
            return Ok(());
        }

        // Toggle reviewed — review tracking is per-branch, not meaningful in History.
        // Tour handles `space` in its own handler (operates on the tour file list).
        A::ToggleReviewed => {
            app.toggle_reviewed()?;
            return Ok(());
        }

        // Jump to next unreviewed file — not meaningful in History/Tour
        A::NextUnreviewedFile => {
            app.next_unreviewed_file();
            return Ok(());
        }

        // Copy hub — offers full file, path, hunk, or line copy options
        A::OpenCopyHub => {
            app.open_copy_hub();
            return Ok(());
        }

        // Export picker — multiselect annotation kinds to clipboard
        A::OpenExportPicker => {
            app.open_export_picker();
            return Ok(());
        }

        // ── Non-History navigation keys ──

        // File navigation
        A::PrevFile => {
            app.tab_mut().prev_file();
            let threshold = app.config.display.auto_context_threshold;
            app.tab_mut().maybe_auto_expand_context(threshold);
        }
        A::NextFile => {
            app.tab_mut().next_file();
            let threshold = app.config.display.auto_context_threshold;
            app.tab_mut().maybe_auto_expand_context(threshold);
        }

        // Line/comment navigation (arrow keys: comments when focused, else lines)
        // Shift+arrow extends selection, plain arrow clears it
        A::ExtendSelectionDown => {
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
        A::ExtendSelectionUp => {
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
        A::NextLine => {
            app.tab_mut().next_line();
        }
        A::PrevLine => {
            app.tab_mut().prev_line();
        }

        // Hunk navigation
        A::NextHunk => app.tab_mut().next_hunk(),
        A::PrevHunk => app.tab_mut().prev_hunk(),

        // Horizontal scroll (for long lines)
        A::ScrollRight => {
            if app.split_diff_active(&app.config.clone()) {
                app.tab_mut().scroll_right_split();
            } else {
                app.tab_mut().scroll_right(8);
            }
        }
        A::ScrollLeft => {
            if app.split_diff_active(&app.config.clone()) {
                app.tab_mut().scroll_left_split();
            } else {
                app.tab_mut().scroll_left(8);
            }
        }
        A::ScrollHome => {
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
        A::ScrollDown(amount) => {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_down(amount);
                app.tab_mut().panel_scroll = app.tab().panel_scroll.min(4096);
            } else {
                app.tab_mut().scroll_down(amount);
            }
        }
        A::ScrollUp(amount) => {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_up(amount);
            } else {
                app.tab_mut().scroll_up(amount);
            }
        }
    }
    Ok(())
}

pub fn execute_ai_summary(app: &mut App, action: AiSummaryAction) -> Result<()> {
    match action {
        // Navigation within focused column
        AiSummaryAction::Next => {
            app.tab_mut().review_next();
        }
        AiSummaryAction::Prev => {
            app.tab_mut().review_prev();
        }

        // Switch focus between left/right columns
        AiSummaryAction::ToggleFocus => {
            app.tab_mut().review_toggle_focus();
            let (files_offset, checklist_offset) = app.tab().ai_summary_section_offsets();
            app.tab_mut().panel_scroll = match app.tab().review_focus {
                ReviewFocus::Files => files_offset,
                ReviewFocus::Checklist => checklist_offset,
            };
        }

        // Toggle checklist item
        AiSummaryAction::ToggleChecklistItem => {
            app.review_toggle_checklist()?;
        }

        // Jump to file
        AiSummaryAction::JumpToFile => {
            app.review_jump_to_file();
        }

        // Scroll — routes to focused column's scroll offset
        AiSummaryAction::ScrollDown(amount) => ai_review_scroll(app, amount, true),
        AiSummaryAction::ScrollUp(amount) => ai_review_scroll(app, amount, false),

        // Esc closes panel focus
        AiSummaryAction::Unfocus => {
            app.tab_mut().panel_focus = false;
        }
    }
    Ok(())
}

pub fn execute_history(app: &mut App, action: HistoryAction) -> Result<()> {
    match action {
        // Commit navigation (left panel)
        HistoryAction::NextCommit => {
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
        HistoryAction::PrevCommit => {
            app.tab_mut().history_prev_commit();
        }

        // File navigation within commit diff (n/N)
        HistoryAction::NextFile => app.tab_mut().history_next_file(),
        HistoryAction::PrevFile => app.tab_mut().history_prev_file(),

        // Line navigation (arrows)
        HistoryAction::NextLine => app.tab_mut().history_next_line(),
        HistoryAction::PrevLine => app.tab_mut().history_prev_line(),

        // Horizontal scroll
        HistoryAction::ScrollRight => app.tab_mut().history_scroll_right(8),
        HistoryAction::ScrollLeft => app.tab_mut().history_scroll_left(8),
        HistoryAction::ScrollHome => {
            if let Some(ref mut h) = app.tab_mut().history {
                h.h_scroll = 0;
            }
        }

        // Scroll
        HistoryAction::ScrollDown(amount) => app.tab_mut().history_scroll_down(amount),
        HistoryAction::ScrollUp(amount) => app.tab_mut().history_scroll_up(amount),
    }
    Ok(())
}

pub fn execute_tour(app: &mut App, action: TourAction) -> Result<()> {
    match action {
        // Pillar navigation (left panel) — k next, j prev (matches History).
        TourAction::NextPillar => app.tab_mut().tour_next_pillar(),
        TourAction::PrevPillar => app.tab_mut().tour_prev_pillar(),

        // File navigation within the tour (n/N)
        TourAction::NextFile => app.tab_mut().tour_next_file(),
        TourAction::PrevFile => app.tab_mut().tour_prev_file(),

        // Line navigation (arrows)
        TourAction::NextLine => app.tab_mut().tour_next_line(),
        TourAction::PrevLine => app.tab_mut().tour_prev_line(),

        // Horizontal scroll
        TourAction::ScrollRight => app.tab_mut().tour_scroll_right(8),
        TourAction::ScrollLeft => app.tab_mut().tour_scroll_left(8),
        TourAction::ScrollHome => {
            if let Some(ref mut t) = app.tab_mut().tour {
                t.h_scroll = 0;
            }
        }

        // Vertical scroll (u/d page through all files; pillars switch as you go)
        TourAction::ScrollDown(amount) => app.tab_mut().tour_scroll_down(amount),
        TourAction::ScrollUp(amount) => app.tab_mut().tour_scroll_up(amount),

        // Toggle reviewed for the current file
        TourAction::ToggleReviewed => app.tab_mut().tour_toggle_reviewed(),

        // Bulk-review the whole pillar
        TourAction::ReviewPillar => {
            app.tab_mut().tour_bulk_review_pillar();
            app.notify("Reviewed all files in pillar");
        }
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
