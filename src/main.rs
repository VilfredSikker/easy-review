mod ai;
mod app;
mod config;
mod git;
mod github;
mod ui;
mod watch;

use anyhow::Result;
use app::{App, ConfirmAction, DiffMode, InputMode, SplitSide};
use crate::ai::{PanelContent, ReviewFocus};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use watch::{FileWatcher, WatchEvent};

/// Terminal UI for reviewing git diffs
#[derive(Parser)]
#[command(name = "er", version, about)]
struct Cli {
    /// Repository paths to open (defaults to current directory)
    paths: Vec<String>,

    /// Open a specific PR by number (from the current repo)
    #[arg(long)]
    pr: Option<u64>,

    /// Pre-apply a file filter expression (e.g. '+*.rs,-*.lock')
    #[arg(long)]
    filter: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Reject conflicting --pr and PR URL arguments
    if cli.pr.is_some() && cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
        anyhow::bail!("Cannot use --pr together with a PR URL argument");
    }

    // Init app state (detects repo, branch, base branch, runs initial diff)
    let mut app = App::new_with_args(&cli.paths)?;

    // Handle --pr flag: override the first tab's base branch
    if let Some(pr_number) = cli.pr {
        github::ensure_gh_installed()?;
        let repo_root = app.tab().repo_root.clone();
        let base = github::gh_pr_base_branch(pr_number, &repo_root)?;
        let base = github::ensure_base_ref_available(&repo_root, &base)?;
        let tab = app.tab_mut();
        tab.base_branch = base;
        tab.refresh_diff()?;
    }

    // Apply --filter flag if provided
    if let Some(ref filter_expr) = cli.filter {
        app.tab_mut().apply_filter_expr(filter_expr);
    }

    // Hint + PR data: check for PR in background (avoids blocking startup on network)
    let (hint_rx, pr_data_rx) = if cli.pr.is_none() && !cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
        let repo_root = app.tab().repo_root.clone();
        let current_base = app.tab().base_branch.clone();
        let (hint_tx, hint_rx) = mpsc::channel::<String>();
        let (pr_tx, pr_rx) = mpsc::channel::<github::PrOverviewData>();
        std::thread::spawn(move || {
            if let Some((pr_num, pr_base)) = github::gh_pr_for_current_branch(&repo_root) {
                if pr_base != current_base {
                    let _ = hint_tx.send(format!(
                        "PR #{} targets {} — run: er --pr {}",
                        pr_num, pr_base, pr_num
                    ));
                }
                // Fetch PR overview data regardless of base mismatch
                if let Some(data) = github::gh_pr_overview(&repo_root) {
                    let _ = pr_tx.send(data);
                }
            }
        });
        (Some(hint_rx), Some(pr_rx))
    } else {
        // For --pr flag or PR URL, fetch PR data synchronously (already in the right state)
        let repo_root = app.tab().repo_root.clone();
        let pr_data = github::gh_pr_overview(&repo_root);
        if let Some(data) = pr_data {
            app.tab_mut().pr_data = Some(data);
        }
        (None, None)
    };

    // Load syntax highlighting (once, reused for all files)
    let mut highlighter = ui::highlight::Highlighter::new();

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run event loop
    let result = run_app(&mut terminal, &mut app, &mut highlighter, hint_rx, pr_data_rx);

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {:?}", err);
    }

    // Print resume hint if multiple tabs were open
    if app.tabs.len() > 1 {
        let paths: Vec<&str> = app.tabs.iter().map(|t| t.repo_root.as_str()).collect();
        eprintln!("\x1b[2mer {}\x1b[0m", paths.join(" "));
    }

    Ok(())
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    hl: &mut ui::highlight::Highlighter,
    hint_rx: Option<mpsc::Receiver<String>>,
    pr_data_rx: Option<mpsc::Receiver<github::PrOverviewData>>,
) -> Result<()> {
    // Channel for file watch events
    let (watch_tx, watch_rx) = mpsc::channel::<WatchEvent>();
    let mut hint_rx = hint_rx;
    let mut pr_data_rx = pr_data_rx;

    // Debounce state for file watcher refreshes
    let mut pending_refresh = false;
    let mut refresh_deadline = Instant::now();
    let mut pending_file_count = 0usize;

    // Start watching by default
    let root_str = app.tab().repo_root.clone();
    let root = std::path::Path::new(&root_str);
    let mut _watcher: Option<FileWatcher> = match FileWatcher::new(root, 500, watch_tx.clone()) {
        Ok(w) => {
            app.watching = true;
            Some(w)
        }
        Err(_) => None,
    };

    loop {
        // Draw
        terminal.draw(|f| ui::draw(f, app, hl))?;

        // Poll for events with a timeout (lets us process watch events too)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Route keys: overlay takes priority, then search, then normal
                if app.overlay.is_some() {
                    handle_overlay_input(app, key)?;
                } else {
                    match &app.input_mode {
                        InputMode::Search => handle_search_input(app, key),
                        InputMode::Comment => handle_comment_input(app, key)?,
                        InputMode::Confirm(_) => handle_confirm_input(app, key)?,
                        InputMode::Filter => handle_filter_input(app, key),
                        InputMode::Commit => handle_commit_input(app, key)?,
                        InputMode::Normal => {
                            handle_normal_input(app, key, &watch_tx, &mut _watcher)?
                        }
                    }
                }
            }
        }

        // Check for file watch events (non-blocking) — debounced
        if let Ok(WatchEvent::FilesChanged(paths)) = watch_rx.try_recv() {
            pending_file_count += paths.len();
            pending_refresh = true;
            refresh_deadline = Instant::now() + Duration::from_millis(200);
        }

        // Execute debounced refresh when deadline passes
        if pending_refresh && Instant::now() >= refresh_deadline {
            pending_refresh = false;
            let count = pending_file_count;
            pending_file_count = 0;
            let _ = app.tab_mut().refresh_diff_quick();
            let ai_status = if app.tab().ai.has_data() {
                if app.tab().ai.is_stale { " · AI stale" } else { " · AI synced" }
            } else {
                ""
            };
            app.notify(&format!(
                "{} file{} changed{}",
                count,
                if count == 1 { "" } else { "s" },
                ai_status
            ));
        }

        // Check for .er-* file changes (throttled: every 10 ticks ≈ 1s)
        app.ai_poll_counter = app.ai_poll_counter.wrapping_add(1);
        if app.ai_poll_counter % 10 == 0 {
            if app.tab_mut().check_ai_files_changed() {
                app.notify("✓ AI data refreshed");
            }
        }

        // Rescan watched files (every 50 ticks ≈ 5s)
        if app.ai_poll_counter % 50 == 0 {
            app.tab_mut().refresh_watched_files();
        }

        // Check for PR base hint from background thread (fires once)
        if let Some(rx) = &hint_rx {
            if let Ok(msg) = rx.try_recv() {
                app.notify(&msg);
                hint_rx = None;
            }
        }

        // Check for PR overview data from background thread (fires once)
        if let Some(rx) = &pr_data_rx {
            if let Ok(data) = rx.try_recv() {
                app.tab_mut().pr_data = Some(data);
                pr_data_rx = None;
            }
        }

        // Tick — used for auto-clearing notifications
        app.tick();

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_overlay_input(app: &mut App, key: KeyEvent) -> Result<()> {
    // Settings overlay has additional keybindings
    if matches!(app.overlay, Some(app::OverlayData::Settings { .. })) {
        match key.code {
            KeyCode::Char('k') | KeyCode::Down => app.overlay_next(),
            KeyCode::Char('j') | KeyCode::Up => app.overlay_prev(),
            KeyCode::Char(' ') | KeyCode::Enter => {
                // Space and Enter both toggle the current item
                app.settings_toggle();
            }
            KeyCode::Char('s') => {
                // Save settings to disk
                app.settings_save();
            }
            KeyCode::Esc | KeyCode::Char('q') => app.overlay_close(),
            _ => {}
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Char('k') | KeyCode::Down => app.overlay_next(),
        KeyCode::Char('j') | KeyCode::Up => app.overlay_prev(),
        KeyCode::Enter => app.overlay_select()?,
        KeyCode::Backspace => app.overlay_go_up(),
        KeyCode::Esc | KeyCode::Char('q') => app.overlay_close(),
        _ => {}
    }
    Ok(())
}

fn handle_normal_input(
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

        // Mode switching (gated by feature flags)
        KeyCode::Char('1') if app.config.features.view_branch => {
            app.tab_mut().set_mode(DiffMode::Branch);
            return Ok(());
        }
        KeyCode::Char('2') if app.config.features.view_unstaged => {
            app.tab_mut().set_mode(DiffMode::Unstaged);
            return Ok(());
        }
        KeyCode::Char('3') if app.config.features.view_staged => {
            app.tab_mut().set_mode(DiffMode::Staged);
            return Ok(());
        }
        KeyCode::Char('4') if app.config.features.view_history => {
            app.tab_mut().set_mode(DiffMode::History);
            return Ok(());
        }
        KeyCode::Char('5') if app.config.features.view_conflicts => {
            app.tab_mut().set_mode(DiffMode::Conflicts);
            return Ok(());
        }
        // Toggle mtime sort (works in any mode)
        KeyCode::Char('m') => {
            let tab = app.tab_mut();
            tab.sort_by_mtime = !tab.sort_by_mtime;
            let _ = tab.refresh_diff();
            let label = if app.tab().sort_by_mtime { "Sort: recent first" } else { "Sort: default" };
            app.notify(label);
            return Ok(());
        }

        // Reload/refresh diff
        KeyCode::Char('R') => {
            app.tab_mut().refresh_diff()?;
            let ai_status = if app.tab().ai.has_data() {
                if app.tab().ai.is_stale { " · AI stale" } else { " · AI synced" }
            } else {
                ""
            };
            app.notify(&format!("Refreshed{}", ai_status));
            return Ok(());
        }

        // Toggle watch mode
        KeyCode::Char('w') => {
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
            app.tab().open_in_editor()?;
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
        // Delete focused comment (after J/K jump) — only if deletable
        KeyCode::Char('d') if key.modifiers == KeyModifiers::NONE => {
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
        KeyCode::Char('t') => {
            app.open_worktree_picker()?;
            return Ok(());
        }
        KeyCode::Char('o') => {
            app.open_directory_browser();
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

    // ── History mode: route to dedicated handler ──
    if app.tab().mode == DiffMode::History {
        return handle_history_input(app, key);
    }

    // ── Normal mode keys ──

    match key.code {
        // File navigation
        KeyCode::Char('j') => app.tab_mut().prev_file(),
        KeyCode::Char('k') => app.tab_mut().next_file(),

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
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_down(10);
            } else {
                app.tab_mut().scroll_down(10);
            }
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_up(10);
            } else {
                app.tab_mut().scroll_up(10);
            }
        }
        KeyCode::PageDown => {
            if app.tab().panel_focus && app.tab().panel.is_some() {
                app.tab_mut().panel_scroll_down(20);
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

        // Search
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Search;
            app.tab_mut().search_query.clear();
        }

        // Stage current hunk (S)
        KeyCode::Char('S') => {
            app.stage_current_hunk()?;
        }

        // Filter
        KeyCode::Char('f') => {
            app.input_mode = InputMode::Filter;
            // Pre-populate with current expression for editing
            app.tab_mut().filter_input = app.tab().filter_expr.clone();
        }

        // Filter history
        KeyCode::Char('F') => {
            app.open_filter_history();
        }

        // Stage/unstage file (or update snapshot for watched files)
        KeyCode::Char('s') => {
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
        }

        // Open settings overlay
        KeyCode::Char(',') => {
            app.open_settings();
        }

        // Toggle reviewed
        KeyCode::Char(' ') => {
            app.toggle_reviewed()?;
        }

        // Toggle unreviewed-only filter
        KeyCode::Char('u') => {
            app.toggle_unreviewed_filter();
        }

        // Expand/compact toggle for compacted files
        KeyCode::Enter => {
            let is_compacted = app.tab().selected_diff_file().map_or(false, |f| f.compacted);
            if is_compacted {
                app.tab_mut().toggle_compacted()?;
            }
        }

        // Yank hunk to clipboard
        KeyCode::Char('y') => {
            app.yank_hunk()?;
        }

        // Copy rich context to clipboard (for agent terminal)
        KeyCode::Char('A') => {
            app.copy_context()?;
        }

        // In Staged mode, c = commit; otherwise c = GitHub comment
        KeyCode::Char('c') => {
            if app.tab().mode == DiffMode::Staged {
                app.start_commit();
            } else {
                app.start_comment(crate::ai::CommentType::GitHubComment);
            }
        }
        KeyCode::Char('C') => {
            app.tab_mut().toggle_layer_comments();
            let on = app.tab().layers.show_github_comments;
            app.notify(if on { "Comments: visible" } else { "Comments: hidden" });
        }

        // Toggle question layer visibility (Q)
        KeyCode::Char('Q') => {
            app.tab_mut().toggle_layer_questions();
            let on = app.tab().layers.show_questions;
            app.notify(if on { "Questions: visible" } else { "Questions: hidden" });
        }

        // Tab: toggle split pane focus when split diff is active; otherwise toggle panel focus
        KeyCode::Tab => {
            if app.split_diff_active(&app.config.clone()) {
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
        }

        // GitHub comment sync (pull)
        KeyCode::Char('G') => {
            sync_github_comments(app)?;
        }

        // Push all comments to GitHub
        KeyCode::Char('P') => {
            push_all_comments_to_github(app)?;
        }

        // Toggle AI findings layer (a)
        KeyCode::Char('a') => {
            app.tab_mut().toggle_layer_ai();
            let on = app.tab().layers.show_ai_findings;
            app.notify(if on { "AI findings: ON" } else { "AI findings: OFF" });
        }
        // Toggle context panel (p) — cycles through panel states
        KeyCode::Char('p') => {
            app.tab_mut().toggle_panel();
        }

        // Clear search first, then filter (innermost → outermost)
        KeyCode::Esc => {
            if !app.tab().search_query.is_empty() {
                app.tab_mut().search_query.clear();
            } else if !app.tab().filter_expr.is_empty() {
                app.tab_mut().clear_filter();
                app.notify("Filter cleared");
            }
        }

        _ => {}
    }
    Ok(())
}

fn handle_ai_review_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Navigation within focused column
        KeyCode::Char('k') | KeyCode::Down => {
            app.tab_mut().review_next();
        }
        KeyCode::Char('j') | KeyCode::Up => {
            app.tab_mut().review_prev();
        }

        // Switch focus between left/right columns
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right
        | KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
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
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ai_review_scroll(app, 10, true);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
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

fn handle_history_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Commit navigation (left panel)
        KeyCode::Char('k') => {
            // Check if at the end and need to load more
            let at_end = app.tab().history.as_ref()
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
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().history_scroll_down(10);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().history_scroll_up(10);
        }
        KeyCode::PageDown => app.tab_mut().history_scroll_down(20),
        KeyCode::PageUp => app.tab_mut().history_scroll_up(20),

        // Search (filter commits)
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Search;
            app.tab_mut().search_query.clear();
        }

        // Clear search filter
        KeyCode::Esc => {
            if !app.tab().search_query.is_empty() {
                app.tab_mut().search_query.clear();
            }
        }

        // Toggle AI findings layer
        KeyCode::Char('a') => {
            app.tab_mut().toggle_layer_ai();
            let on = app.tab().layers.show_ai_findings;
            app.notify(if on { "AI findings: ON" } else { "AI findings: OFF" });
        }
        // Toggle context panel
        KeyCode::Char('p') => {
            app.tab_mut().toggle_panel();
        }
        // Toggle panel focus
        KeyCode::Tab => {
            let tab = app.tab_mut();
            if tab.panel.is_some() {
                tab.panel_focus = !tab.panel_focus;
            }
        }

        _ => {}
    }
    Ok(())
}

/// Scroll the focused column in AiSummary panel
fn ai_review_scroll(app: &mut App, amount: u16, down: bool) {
    let tab = app.tab_mut();
    if down {
        tab.panel_scroll = tab.panel_scroll.saturating_add(amount);
    } else {
        tab.panel_scroll = tab.panel_scroll.saturating_sub(amount);
    }
}

fn handle_search_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            if key.code == KeyCode::Esc {
                app.tab_mut().search_query.clear();
            } else {
                // Search confirmed — snap selection to a visible file
                app.tab_mut().snap_to_visible();
            }
        }
        KeyCode::Char(c) => {
            app.tab_mut().search_query.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().search_query.pop();
        }
        _ => {}
    }
}

fn handle_filter_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            let expr = app.tab().filter_input.clone();
            app.tab_mut().apply_filter_expr(&expr);
            app.input_mode = InputMode::Normal;
            if expr.trim().is_empty() {
                app.notify("Filter cleared");
            } else {
                let visible = app.tab().visible_files().len();
                let total = app.tab().files.len();
                app.notify(&format!("Filter: {} ({}/{})", expr.trim(), visible, total));
            }
        }
        KeyCode::Esc => {
            app.tab_mut().filter_input.clear();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char(c) => {
            app.tab_mut().filter_input.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().filter_input.pop();
        }
        _ => {}
    }
}

fn handle_comment_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            app.submit_comment()?;
        }
        KeyCode::Esc => {
            app.cancel_comment();
        }
        // Scroll the diff view while composing a comment
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_down(10);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_up(10);
        }
        KeyCode::PageDown => app.tab_mut().scroll_down(20),
        KeyCode::PageUp => app.tab_mut().scroll_up(20),
        KeyCode::Char(c) => {
            app.tab_mut().comment_input.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().comment_input.pop();
        }
        _ => {}
    }
    Ok(())
}

fn handle_confirm_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('y') => {
            let action = app.input_mode.clone();
            if let InputMode::Confirm(ConfirmAction::DeleteComment { comment_id }) = action {
                app.confirm_delete_comment(&comment_id)?;
            }
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            app.cancel_confirm();
        }
        _ => {} // Ignore all other keys in confirm mode
    }
    Ok(())
}

fn handle_commit_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            app.submit_commit()?;
        }
        KeyCode::Esc => {
            app.cancel_commit();
        }
        KeyCode::Char(c) => {
            app.tab_mut().commit_input.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().commit_input.pop();
        }
        _ => {}
    }
    Ok(())
}

/// Sync GitHub PR comments (pull)
fn sync_github_comments(app: &mut App) -> Result<()> {
    let tab = app.tab();
    let repo_root = tab.repo_root.clone();

    let pr_info = github::get_pr_info(&repo_root);
    let pr_info = match pr_info {
        Ok(info) => info,
        Err(_) => {
            app.notify("No PR found for current branch");
            return Ok(());
        }
    };

    let (owner, repo_name, pr_number) = pr_info;

    let gh_comments = match github::gh_pr_comments(&owner, &repo_name, pr_number, &repo_root) {
        Ok(c) => c,
        Err(e) => {
            app.notify(&format!("GitHub sync error: {}", e));
            return Ok(());
        }
    };

    // Load existing .er-github-comments.json
    let comments_path = format!("{}/.er-github-comments.json", repo_root);
    let diff_hash = tab.branch_diff_hash.clone();
    let mut gc: crate::ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| crate::ai::ErGitHubComments {
            version: 1,
            diff_hash: diff_hash.clone(),
            github: None,
            comments: Vec::new(),
        }),
        Err(_) => crate::ai::ErGitHubComments {
            version: 1,
            diff_hash: diff_hash.clone(),
            github: None,
            comments: Vec::new(),
        },
    };

    gc.github = Some(crate::ai::GitHubSyncState {
        pr_number: Some(pr_number),
        owner: owner.clone(),
        repo: repo_name.clone(),
        last_synced: chrono_now(),
    });

    let known_github_ids: std::collections::HashSet<u64> = gc.comments.iter()
        .filter_map(|c| c.github_id)
        .collect();

    let mut remote_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut added = 0u32;
    let mut updated = 0u32;
    let tab_files = &app.tab().files;

    for gh in &gh_comments {
        remote_ids.insert(gh.id);

        if known_github_ids.contains(&gh.id) {
            if let Some(c) = gc.comments.iter_mut().find(|c| c.github_id == Some(gh.id)) {
                if c.comment != gh.body {
                    c.comment = gh.body.clone();
                    updated += 1;
                }
            }
        } else {
            let file_path = gh.path.clone().unwrap_or_default();
            // Extract hunk_index and anchor data from the diff in one pass
            let (hunk_index, anchor_line_content, anchor_ctx_before, anchor_ctx_after, anchor_old_line, anchor_hunk_header) =
                if let Some(line) = gh.line {
                    if let Some(f) = tab_files.iter().find(|f| f.path == file_path) {
                        if let Some((i, hunk)) = f.hunks.iter().enumerate().find(|(_, h)| {
                            line >= h.new_start && line < h.new_start + h.new_count
                        }) {
                            let target_idx = hunk.lines.iter().position(|l| l.new_num == Some(line));
                            let (lc, old_ln) = if let Some(idx) = target_idx {
                                (hunk.lines[idx].content.clone(), hunk.lines[idx].old_num)
                            } else {
                                (String::new(), None)
                            };
                            let ctx_before: Vec<String> = if let Some(idx) = target_idx {
                                let start = idx.saturating_sub(3);
                                hunk.lines[start..idx].iter().map(|l| l.content.clone()).collect()
                            } else {
                                Vec::new()
                            };
                            let ctx_after: Vec<String> = if let Some(idx) = target_idx {
                                let end = (idx + 4).min(hunk.lines.len());
                                hunk.lines[(idx + 1)..end].iter().map(|l| l.content.clone()).collect()
                            } else {
                                Vec::new()
                            };
                            (Some(i), lc, ctx_before, ctx_after, old_ln, hunk.header.clone())
                        } else {
                            (None, String::new(), Vec::new(), Vec::new(), None, String::new())
                        }
                    } else {
                        (None, String::new(), Vec::new(), Vec::new(), None, String::new())
                    }
                } else {
                    (None, String::new(), Vec::new(), Vec::new(), None, String::new())
                };

            let in_reply_to = gh.in_reply_to_id.and_then(|parent_gh_id| {
                gc.comments.iter()
                    .find(|c| c.github_id == Some(parent_gh_id))
                    .map(|c| c.id.clone())
            });

            let comment = crate::ai::GitHubReviewComment {
                id: format!("gh-{}", gh.id),
                timestamp: gh.created_at.clone(),
                file: file_path,
                hunk_index,
                line_start: gh.line,
                line_end: None,
                line_content: anchor_line_content,
                comment: gh.body.clone(),
                in_reply_to,
                resolved: false,
                source: "github".to_string(),
                github_id: Some(gh.id),
                author: gh.user.login.clone(),
                synced: true,
                stale: false,
                context_before: anchor_ctx_before,
                context_after: anchor_ctx_after,
                old_line_start: anchor_old_line,
                hunk_header: anchor_hunk_header,
                anchor_status: "original".to_string(),
                relocated_at_hash: app.tab().diff_hash.clone(),
                finding_ref: None,
            };

            gc.comments.push(comment);
            added += 1;
        }
    }

    let removed = gc.comments.iter()
        .filter(|c| c.source == "github" && c.github_id.is_some() && !remote_ids.contains(&c.github_id.unwrap()))
        .count() as u32;
    gc.comments.retain(|c| {
        if c.source == "github" {
            c.github_id.map_or(true, |id| remote_ids.contains(&id))
        } else {
            true
        }
    });

    let json = serde_json::to_string_pretty(&gc)?;
    let tmp_path = format!("{}.tmp", comments_path);
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &comments_path)?;

    app.tab_mut().reload_ai_state();
    app.notify(&format!("GitHub sync: +{} ~{} -{}", added, updated, removed));
    Ok(())
}

/// Push all unpushed local comments to GitHub
fn push_all_comments_to_github(app: &mut App) -> Result<()> {
    let tab = app.tab();
    let repo_root = tab.repo_root.clone();

    let pr_info = match github::get_pr_info(&repo_root) {
        Ok(info) => info,
        Err(_) => {
            app.notify("No PR found for current branch");
            return Ok(());
        }
    };
    let (owner, repo_name, pr_number) = pr_info;

    let comments_path = format!("{}/.er-github-comments.json", repo_root);
    let mut gc: crate::ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(gc) => gc,
            Err(_) => return Ok(()),
        },
        Err(_) => return Ok(()),
    };

    let mut pushed = 0u32;
    let mut failed = 0u32;

    // Push parents first
    let comment_ids: Vec<String> = gc.comments.iter()
        .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.is_none())
        .map(|c| c.id.clone())
        .collect();

    for cid in &comment_ids {
        let comment = gc.comments.iter().find(|c| c.id == *cid).cloned();
        if let Some(comment) = comment {
            let path = &comment.file;
            let line = comment.line_start.unwrap_or(1);
            match github::gh_pr_push_comment(&owner, &repo_name, pr_number, path, line, &comment.comment, &repo_root) {
                Ok(github_id) => {
                    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                        c.github_id = Some(github_id);
                        c.synced = true;
                    }
                    pushed += 1;
                }
                Err(_) => { failed += 1; }
            }
        }
    }

    // Then push replies
    let reply_ids: Vec<String> = gc.comments.iter()
        .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.is_some())
        .map(|c| c.id.clone())
        .collect();

    for cid in &reply_ids {
        let comment = gc.comments.iter().find(|c| c.id == *cid).cloned();
        if let Some(comment) = comment {
            let parent_gh_id = comment.in_reply_to.as_ref()
                .and_then(|rt| gc.comments.iter().find(|c| c.id == *rt))
                .and_then(|c| c.github_id);

            if let Some(parent_gh_id) = parent_gh_id {
                match github::gh_pr_reply_comment(&owner, &repo_name, pr_number, parent_gh_id, &comment.comment, &repo_root) {
                    Ok(github_id) => {
                        if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                            c.github_id = Some(github_id);
                            c.synced = true;
                        }
                        pushed += 1;
                    }
                    Err(_) => { failed += 1; }
                }
            } else {
                failed += 1;
            }
        }
    }

    let json = serde_json::to_string_pretty(&gc)?;
    let tmp_path = format!("{}.tmp", comments_path);
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &comments_path)?;
    app.tab_mut().reload_ai_state();

    if failed > 0 {
        app.notify(&format!("Pushed {} comments ({} failed)", pushed, failed));
    } else {
        app.notify(&format!("Pushed {} comments", pushed));
    }
    Ok(())
}

fn chrono_now() -> String {
    app::chrono_now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ConfirmAction, InputMode, TabState};
    use crate::config::ErConfig;
    use crate::git::{DiffFile, DiffHunk, DiffLine, FileStatus, LineType};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc;

    // ── Test helpers ──

    fn make_app(files: Vec<DiffFile>) -> App {
        App {
            tabs: vec![TabState::new_for_test(files)],
            active_tab: 0,
            input_mode: InputMode::Normal,
            should_quit: false,
            overlay: None,
            watching: false,
            watch_message: None,
            watch_message_ticks: 0,
            ai_poll_counter: 0,
            config: ErConfig::default(),
        }
    }

    fn make_file_with_hunk() -> DiffFile {
        DiffFile {
            path: "src/main.rs".to_string(),
            status: FileStatus::Modified,
            hunks: vec![DiffHunk {
                header: "@@ -1,3 +1,4 @@".to_string(),
                old_start: 1,
                old_count: 3,
                new_start: 1,
                new_count: 4,
                lines: vec![DiffLine {
                    line_type: LineType::Add,
                    content: "let x = 1;".to_string(),
                    old_num: None,
                    new_num: Some(1),
                }],
            }],
            adds: 1,
            dels: 0,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    fn send_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let (tx, _rx) = mpsc::channel::<watch::WatchEvent>();
        let mut watcher: Option<watch::FileWatcher> = None;
        let key = KeyEvent::new(code, modifiers);
        handle_normal_input(app, key, &tx, &mut watcher).unwrap();
    }

    // ── Ctrl+q vs bare q ──

    #[test]
    fn ctrl_q_sets_should_quit() {
        let mut app = make_app(vec![]);
        send_key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(app.should_quit, "Ctrl+q must set should_quit = true");
        assert_eq!(app.input_mode, InputMode::Normal, "Ctrl+q must not change input_mode");
    }

    #[test]
    fn bare_q_starts_comment_mode_when_file_selected() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        send_key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(
            app.input_mode,
            InputMode::Comment,
            "bare q must enter Comment mode"
        );
        assert!(!app.should_quit, "bare q must not set should_quit");
    }

    #[test]
    fn bare_q_does_not_quit() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        send_key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(!app.should_quit);
    }

    // ── Ctrl+d vs bare d ──

    #[test]
    fn ctrl_d_scrolls_diff_when_no_focused_comment() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        assert_eq!(app.tab().diff_scroll, 0);
        send_key(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(
            app.tab().diff_scroll,
            10,
            "Ctrl+d must scroll diff down by 10"
        );
        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "Ctrl+d must not change input_mode"
        );
    }

    #[test]
    fn ctrl_d_does_not_trigger_delete_confirm_even_when_comment_focused() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        app.tab_mut().focused_comment_id = Some("q-123".to_string());
        send_key(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
        // Ctrl+d should scroll, not enter Confirm mode
        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "Ctrl+d must not enter Confirm mode — it scrolls"
        );
    }

    #[test]
    fn bare_d_triggers_delete_confirm_when_comment_focused() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        // Add a question to AI state so find_comment + can_delete succeeds
        app.tab_mut().ai.questions = Some(crate::ai::ErQuestions {
            version: 1,
            diff_hash: String::new(),
            questions: vec![crate::ai::ReviewQuestion {
                id: "q-abc".to_string(),
                timestamp: String::new(),
                file: "test.rs".to_string(),
                hunk_index: Some(0),
                line_start: None,
                line_content: String::new(),
                text: "test".to_string(),
                resolved: false,
                stale: false,
                context_before: vec![],
                context_after: vec![],
                old_line_start: None,
                hunk_header: String::new(),
                anchor_status: "original".to_string(),
                relocated_at_hash: String::new(),
                in_reply_to: None,
                author: "You".to_string(),
            }],
        });
        app.tab_mut().focused_comment_id = Some("q-abc".to_string());
        send_key(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(
            app.input_mode,
            InputMode::Confirm(ConfirmAction::DeleteComment {
                comment_id: "q-abc".to_string()
            }),
            "bare d with focused comment must enter Confirm(DeleteComment)"
        );
    }

    #[test]
    fn bare_d_does_nothing_when_no_comment_focused() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        // focused_comment_id is None by default
        send_key(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "bare d with no focused comment must stay in Normal mode"
        );
    }

    // ── Ctrl+u vs bare u ──

    #[test]
    fn ctrl_u_scrolls_diff_up() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        app.tab_mut().diff_scroll = 20;
        send_key(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(
            app.tab().diff_scroll,
            10,
            "Ctrl+u must scroll diff up by 10"
        );
    }

    #[test]
    fn ctrl_u_does_not_toggle_unreviewed_filter() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        assert!(!app.tab().show_unreviewed_only);
        send_key(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert!(
            !app.tab().show_unreviewed_only,
            "Ctrl+u must not toggle show_unreviewed_only"
        );
    }

    #[test]
    fn bare_u_toggles_unreviewed_filter_on() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        assert!(!app.tab().show_unreviewed_only);
        send_key(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert!(
            app.tab().show_unreviewed_only,
            "bare u must toggle show_unreviewed_only to true"
        );
    }

    #[test]
    fn bare_u_does_not_scroll_diff() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        app.tab_mut().diff_scroll = 20;
        send_key(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(
            app.tab().diff_scroll,
            20,
            "bare u must not change diff_scroll"
        );
    }

    // ── Ctrl+j vs bare j (panel not focused) ──

    #[test]
    fn ctrl_j_calls_prev_finding_not_panel_scroll() {
        // Without any AI findings loaded, prev_finding is a no-op on selection
        // but it must NOT change panel_scroll (which bare j would do in panel mode).
        let mut app = make_app(vec![make_file_with_hunk()]);
        app.tab_mut().panel = Some(PanelContent::FileDetail);
        app.tab_mut().panel_focus = false; // panel not focused
        app.tab_mut().panel_scroll = 5;
        send_key(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL);
        // panel_scroll must be unchanged — Ctrl+j navigates findings, not panel
        assert_eq!(
            app.tab().panel_scroll,
            5,
            "Ctrl+j must not scroll panel"
        );
    }

    #[test]
    fn bare_j_navigates_files_not_panel() {
        // bare j calls prev_file when panel is NOT focused
        let files = vec![
            make_file_with_hunk(),
            DiffFile {
                path: "src/lib.rs".to_string(),
                status: FileStatus::Modified,
                hunks: vec![],
                adds: 0,
                dels: 0,
                compacted: false,
                raw_hunk_count: 0,
            },
        ];
        let mut app = make_app(files);
        app.tab_mut().selected_file = 1; // start at second file
        app.tab_mut().panel_focus = false;
        send_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        // prev_file moves selected_file from 1 → 0
        assert_eq!(
            app.tab().selected_file,
            0,
            "bare j must navigate to previous file"
        );
    }

    // ── Modifier isolation: Ctrl+k vs bare k ──

    #[test]
    fn ctrl_k_calls_next_finding_not_panel_scroll() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        app.tab_mut().panel = Some(PanelContent::FileDetail);
        app.tab_mut().panel_focus = false;
        app.tab_mut().panel_scroll = 5;
        send_key(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(
            app.tab().panel_scroll,
            5,
            "Ctrl+k must not scroll panel"
        );
    }

    // ── KeyModifiers::NONE guard is exact ──

    #[test]
    fn d_with_shift_does_not_trigger_delete_or_scroll() {
        // Shift+d has no handler in the current map, so nothing should change
        let mut app = make_app(vec![make_file_with_hunk()]);
        app.tab_mut().focused_comment_id = Some("q-abc".to_string());
        let before_scroll = app.tab().diff_scroll;
        send_key(&mut app, KeyCode::Char('d'), KeyModifiers::SHIFT);
        // Shift+d is not handled — state unchanged
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.tab().diff_scroll, before_scroll);
    }
}
