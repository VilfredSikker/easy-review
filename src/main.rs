mod ai;
mod app;
mod config;
mod git;
mod github;
mod ui;
mod watch;

use anyhow::Result;
use app::{App, ConfirmAction, DiffMode, InputMode};
use crate::ai::PanelContent;
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

    // Hint: check for PR base mismatch in background (avoids blocking startup on network)
    let hint_rx = if cli.pr.is_none() && !cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
        let repo_root = app.tab().repo_root.clone();
        let current_base = app.tab().base_branch.clone();
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            if let Some((pr_num, pr_base)) = github::gh_pr_for_current_branch(&repo_root) {
                if pr_base != current_base {
                    let _ = tx.send(format!(
                        "PR #{} targets {} — run: er --pr {}",
                        pr_num, pr_base, pr_num
                    ));
                }
            }
        });
        Some(rx)
    } else {
        None
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
    let result = run_app(&mut terminal, &mut app, &mut highlighter, hint_rx);

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
) -> Result<()> {
    // Channel for file watch events
    let (watch_tx, watch_rx) = mpsc::channel::<WatchEvent>();
    let mut hint_rx = hint_rx;

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
            KeyCode::Char('j') | KeyCode::Down => app.overlay_next(),
            KeyCode::Char('k') | KeyCode::Up => app.overlay_prev(),
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
        KeyCode::Char('j') | KeyCode::Down => app.overlay_next(),
        KeyCode::Char('k') | KeyCode::Up => app.overlay_prev(),
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
        KeyCode::Char('4') => {
            app.tab_mut().set_mode(DiffMode::History);
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

        // Refresh / Reply (r = reply when comment focused, else refresh)
        KeyCode::Char('r') => {
            if app.tab().comment_focus.is_some() {
                app.start_reply();
            } else {
                app.tab_mut().refresh_diff()?;
                let ai_status = if app.tab().ai.has_data() {
                    if app.tab().ai.is_stale { " · AI stale" } else { " · AI synced" }
                } else {
                    ""
                };
                app.notify(&format!("Refreshed{}", ai_status));
            }
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

        // Open in editor
        KeyCode::Char('e') => {
            app.tab().open_in_editor()?;
            return Ok(());
        }

        // Comment jumping across files
        KeyCode::Char(']') => {
            app.next_comment();
            return Ok(());
        }
        KeyCode::Char('[') => {
            app.prev_comment();
            return Ok(());
        }
        KeyCode::Char('x') => {
            app.close_tab();
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

    // ── AiSummary panel focused: route remaining keys to dedicated handler ──
    if app.tab().panel_focus && app.tab().panel == Some(PanelContent::AiSummary) {
        return handle_ai_review_input(app, key);
    }

    // ── History mode: route to dedicated handler ──
    if app.tab().mode == DiffMode::History {
        return handle_history_input(app, key);
    }

    // ── Normal mode keys ──

    match key.code {
        // File navigation
        KeyCode::Char('j') => app.tab_mut().next_file(),
        KeyCode::Char('k') => app.tab_mut().prev_file(),

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
            if app.tab().comment_focus.is_some() {
                app.next_comment();
            } else {
                app.tab_mut().next_line();
            }
        }
        KeyCode::Up => {
            if app.tab().comment_focus.is_some() {
                app.prev_comment();
            } else {
                app.tab_mut().prev_line();
            }
        }

        // Hunk navigation
        KeyCode::Char('n') => app.tab_mut().next_hunk(),
        KeyCode::Char('N') => app.tab_mut().prev_hunk(),

        // Horizontal scroll (for long lines)
        KeyCode::Char('l') | KeyCode::Right => app.tab_mut().scroll_right(8),
        KeyCode::Char('h') | KeyCode::Left => app.tab_mut().scroll_left(8),
        KeyCode::Home => {
            app.tab_mut().h_scroll = 0;
        }

        // Scroll
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_down(10);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_up(10);
        }
        KeyCode::PageDown => app.tab_mut().scroll_down(20),
        KeyCode::PageUp => app.tab_mut().scroll_up(20),

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

        // Tab: toggle panel focus (only when panel is open)
        KeyCode::Tab => {
            let tab = app.tab_mut();
            if tab.panel.is_some() {
                tab.panel_focus = !tab.panel_focus;
            }
        }

        // Delete focused comment
        KeyCode::Char('d') if app.tab().comment_focus.is_some() => {
            app.start_delete_comment();
        }

        // Toggle resolved on focused comment
        KeyCode::Char('R') => {
            if let Some(focus) = app.tab().comment_focus.clone() {
                toggle_comment_resolved(app, &focus.comment_id)?;
            }
        }

        // GitHub comment sync (pull)
        KeyCode::Char('G') => {
            sync_github_comments(app)?;
        }

        // Push comment(s) to GitHub: P on focused comment pushes one, P with no focus pushes all
        KeyCode::Char('P') => {
            if app.tab().comment_focus.is_some() {
                push_comment_to_github(app)?;
            } else {
                push_all_comments_to_github(app)?;
            }
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
        KeyCode::Char('j') | KeyCode::Down => {
            app.tab_mut().review_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.tab_mut().review_prev();
        }

        // Switch focus between left/right columns
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
            ai_review_reset_outgoing_scroll(app);
            app.tab_mut().review_toggle_focus();
        }
        KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
            ai_review_reset_outgoing_scroll(app);
            app.tab_mut().review_toggle_focus();
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
            app.tab_mut().diff_scroll = 0;
        }

        _ => {}
    }
    Ok(())
}

fn handle_history_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Commit navigation (left panel)
        KeyCode::Char('j') => {
            // Check if at the end and need to load more
            let at_end = app.tab().history.as_ref()
                .map(|h| h.selected_commit + 1 >= h.commits.len())
                .unwrap_or(false);
            if at_end {
                app.tab_mut().history_load_more();
            }
            app.tab_mut().history_next_commit();
        }
        KeyCode::Char('k') => {
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

        _ => {}
    }
    Ok(())
}

/// Reset the outgoing column's scroll when switching focus in AiSummary panel
fn ai_review_reset_outgoing_scroll(app: &mut App) {
    use crate::ai::ReviewFocus;
    let tab = app.tab_mut();
    match tab.review_focus {
        ReviewFocus::Files => tab.diff_scroll = 0,
        ReviewFocus::Checklist => tab.ai_panel_scroll = 0,
    }
}

/// Scroll the focused column in AiSummary panel
fn ai_review_scroll(app: &mut App, amount: u16, down: bool) {
    use crate::ai::ReviewFocus;
    let tab = app.tab_mut();
    let scroll = match tab.review_focus {
        ReviewFocus::Files => &mut tab.diff_scroll,
        ReviewFocus::Checklist => &mut tab.ai_panel_scroll,
    };
    if down {
        *scroll = scroll.saturating_add(amount);
    } else {
        *scroll = scroll.saturating_sub(amount);
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

/// Toggle the resolved flag on a comment and persist
fn toggle_comment_resolved(app: &mut App, comment_id: &str) -> Result<()> {
    let tab = app.tab();
    let repo_root = tab.repo_root.clone();
    let is_question = comment_id.starts_with("q-");

    if is_question {
        let path = format!("{}/.er-questions.json", repo_root);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut qs) = serde_json::from_str::<crate::ai::ErQuestions>(&content) {
                let mut toggled = false;
                for q in &mut qs.questions {
                    if q.id == comment_id {
                        q.resolved = !q.resolved;
                        toggled = true;
                        break;
                    }
                }
                if toggled {
                    let json = serde_json::to_string_pretty(&qs)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                    app.tab_mut().reload_ai_state();
                    app.notify("Toggled resolved");
                }
            }
        }
    } else {
        let path = format!("{}/.er-github-comments.json", repo_root);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut gc) = serde_json::from_str::<crate::ai::ErGitHubComments>(&content) {
                let mut toggled = false;
                for c in &mut gc.comments {
                    if c.id == comment_id {
                        c.resolved = !c.resolved;
                        toggled = true;
                        break;
                    }
                }
                if toggled {
                    let json = serde_json::to_string_pretty(&gc)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                    app.tab_mut().reload_ai_state();
                    app.notify("Toggled resolved");
                }
            }
        }
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

    let gh_comments = match github::gh_pr_comments(&owner, &repo_name, pr_number) {
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
            let hunk_index = gh.line.and_then(|line| {
                tab_files.iter()
                    .find(|f| f.path == file_path)
                    .and_then(|f| {
                        f.hunks.iter().enumerate().find(|(_, h)| {
                            line >= h.new_start && line < h.new_start + h.new_count
                        }).map(|(i, _)| i)
                    })
            });

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
                line_content: String::new(),
                comment: gh.body.clone(),
                in_reply_to,
                resolved: false,
                source: "github".to_string(),
                github_id: Some(gh.id),
                author: gh.user.login.clone(),
                synced: true,
                stale: false,
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

/// Push a single focused comment to GitHub
fn push_comment_to_github(app: &mut App) -> Result<()> {
    let tab = app.tab();
    let focus = match &tab.comment_focus {
        Some(f) => f.clone(),
        None => return Ok(()),
    };

    // Questions can't be pushed to GitHub
    if focus.comment_id.starts_with("q-") {
        app.notify("Questions are private — use /er-publish for GitHub comments");
        return Ok(());
    }

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

    let comment = match gc.comments.iter().find(|c| c.id == focus.comment_id) {
        Some(c) => c.clone(),
        None => return Ok(()),
    };

    if comment.synced {
        app.notify("Comment already synced");
        return Ok(());
    }

    if comment.source != "local" {
        app.notify("Only local comments can be pushed");
        return Ok(());
    }

    let result = if let Some(reply_to_id) = comment.in_reply_to.as_ref()
        .and_then(|rt| gc.comments.iter().find(|c| c.id == *rt))
        .and_then(|c| c.github_id)
    {
        github::gh_pr_reply_comment(&owner, &repo_name, pr_number, reply_to_id, &comment.comment)
    } else {
        let path = &comment.file;
        let line = comment.line_start.unwrap_or(1);
        github::gh_pr_push_comment(&owner, &repo_name, pr_number, path, line, &comment.comment)
    };

    match result {
        Ok(github_id) => {
            if let Some(c) = gc.comments.iter_mut().find(|c| c.id == focus.comment_id) {
                c.github_id = Some(github_id);
                c.synced = true;
            }
            let json = serde_json::to_string_pretty(&gc)?;
            let tmp_path = format!("{}.tmp", comments_path);
            std::fs::write(&tmp_path, &json)?;
            std::fs::rename(&tmp_path, &comments_path)?;
            app.tab_mut().reload_ai_state();
            app.notify("Comment pushed to GitHub");
        }
        Err(e) => {
            app.notify(&format!("Push failed: {}", e));
        }
    }
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
            match github::gh_pr_push_comment(&owner, &repo_name, pr_number, path, line, &comment.comment) {
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
                match github::gh_pr_reply_comment(&owner, &repo_name, pr_number, parent_gh_id, &comment.comment) {
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
