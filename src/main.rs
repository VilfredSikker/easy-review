mod ai;
mod app;
mod git;
mod github;
mod ui;
mod watch;

use anyhow::Result;
use app::{App, DiffMode, InputMode};
use crate::ai::ViewMode;
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
use std::time::Duration;
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
    let highlighter = ui::highlight::Highlighter::new();

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run event loop
    let result = run_app(&mut terminal, &mut app, &highlighter, hint_rx);

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
    hl: &ui::highlight::Highlighter,
    hint_rx: Option<mpsc::Receiver<String>>,
) -> Result<()> {
    // Channel for file watch events
    let (watch_tx, watch_rx) = mpsc::channel::<WatchEvent>();
    let mut _watcher: Option<FileWatcher> = None;
    let mut hint_rx = hint_rx;

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
                    match app.input_mode {
                        InputMode::Search => handle_search_input(app, key),
                        InputMode::Comment => handle_comment_input(app, key)?,
                        InputMode::Filter => handle_filter_input(app, key),
                        InputMode::Normal => {
                            handle_normal_input(app, key, &watch_tx, &mut _watcher)?
                        }
                    }
                }
            }
        }

        // Check for file watch events (non-blocking)
        if let Ok(WatchEvent::FilesChanged(paths)) = watch_rx.try_recv() {
            let count = paths.len();
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
        // Quit
        KeyCode::Char('q') => {
            app.should_quit = true;
            return Ok(());
        }

        // Mode switching
        KeyCode::Char('1') => {
            app.tab_mut().set_mode(DiffMode::Branch);
            return Ok(());
        }
        KeyCode::Char('2') => {
            app.tab_mut().set_mode(DiffMode::Unstaged);
            return Ok(());
        }
        KeyCode::Char('3') => {
            app.tab_mut().set_mode(DiffMode::Staged);
            return Ok(());
        }

        // Refresh
        KeyCode::Char('r') => {
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

        // Open in editor
        KeyCode::Char('e') => {
            app.tab().open_in_editor()?;
            return Ok(());
        }

        // Tab navigation
        KeyCode::Char(']') => {
            app.next_tab();
            return Ok(());
        }
        KeyCode::Char('[') => {
            app.prev_tab();
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

        _ => {}
    }

    // ── AiReview mode: route remaining keys to dedicated handler ──
    if app.tab().ai.view_mode == ViewMode::AiReview {
        return handle_ai_review_input(app, key);
    }

    // ── Normal mode keys ──

    match key.code {
        // File navigation
        KeyCode::Char('j') => app.tab_mut().next_file(),
        KeyCode::Char('k') => app.tab_mut().prev_file(),

        // Line navigation (arrow keys navigate within hunks)
        KeyCode::Down => app.tab_mut().next_line(),
        KeyCode::Up => app.tab_mut().prev_line(),

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

        // Stage/unstage file
        KeyCode::Char('s') => {
            app.toggle_stage_file()?;
        }

        // Stage current hunk
        KeyCode::Char('S') => {
            app.stage_current_hunk()?;
        }

        // Toggle reviewed
        KeyCode::Char(' ') => {
            app.toggle_reviewed()?;
        }

        // Toggle unreviewed-only filter
        KeyCode::Char('u') => {
            app.toggle_unreviewed_filter();
        }

        // Yank hunk to clipboard
        KeyCode::Char('y') => {
            app.yank_hunk()?;
        }

        // Comment on current hunk
        KeyCode::Char('c') => {
            app.start_comment();
        }

        // Toggle AI view mode (v forward, V backward)
        KeyCode::Char('v') => {
            app.tab_mut().ai.cycle_view_mode();
            app.tab_mut().diff_scroll = 0;
            app.tab_mut().ai_panel_scroll = 0;
            let mode = app.tab().ai.view_mode.label();
            app.notify(&format!("View: {}", mode));
        }
        KeyCode::Char('V') => {
            app.tab_mut().ai.cycle_view_mode_prev();
            app.tab_mut().diff_scroll = 0;
            app.tab_mut().ai_panel_scroll = 0;
            let mode = app.tab().ai.view_mode.label();
            app.notify(&format!("View: {}", mode));
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
            app.tab_mut().ai.review_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.tab_mut().ai.review_prev();
        }

        // Switch focus between left/right columns
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
            ai_review_reset_outgoing_scroll(app);
            app.tab_mut().ai.review_toggle_focus();
        }
        KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
            ai_review_reset_outgoing_scroll(app);
            app.tab_mut().ai.review_toggle_focus();
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

        // Cycle view mode (v forward, V backward)
        KeyCode::Char('v') => {
            app.tab_mut().ai.cycle_view_mode();
            app.tab_mut().diff_scroll = 0;
            app.tab_mut().ai_panel_scroll = 0;
            let mode = app.tab().ai.view_mode.label();
            app.notify(&format!("View: {}", mode));
        }
        KeyCode::Char('V') => {
            app.tab_mut().ai.cycle_view_mode_prev();
            app.tab_mut().diff_scroll = 0;
            app.tab_mut().ai_panel_scroll = 0;
            let mode = app.tab().ai.view_mode.label();
            app.notify(&format!("View: {}", mode));
        }

        // Esc goes back to default view
        KeyCode::Esc => {
            app.tab_mut().ai.view_mode = ViewMode::Default;
            app.tab_mut().diff_scroll = 0;
            app.notify("View: DEFAULT");
        }

        _ => {}
    }
    Ok(())
}

/// Reset the outgoing column's scroll when switching focus in AiReview
fn ai_review_reset_outgoing_scroll(app: &mut App) {
    use crate::ai::ReviewFocus;
    let tab = app.tab_mut();
    match tab.ai.review_focus {
        ReviewFocus::Files => tab.diff_scroll = 0,
        ReviewFocus::Checklist => tab.ai_panel_scroll = 0,
    }
}

/// Scroll the focused column in AiReview mode
fn ai_review_scroll(app: &mut App, amount: u16, down: bool) {
    use crate::ai::ReviewFocus;
    let tab = app.tab_mut();
    let scroll = match tab.ai.review_focus {
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
