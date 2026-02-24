mod app;
mod git;
mod ui;
mod watch;

use anyhow::Result;
use app::{App, DiffMode, InputMode};
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

fn main() -> Result<()> {
    // Init app state (detects repo, branch, base branch, runs initial diff)
    let mut app = App::new()?;

    // Load syntax highlighting (once, reused for all files)
    let highlighter = ui::highlight::Highlighter::new();

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run event loop
    let result = run_app(&mut terminal, &mut app, &highlighter);

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    hl: &ui::highlight::Highlighter,
) -> Result<()> {
    // Channel for file watch events
    let (watch_tx, watch_rx) = mpsc::channel::<WatchEvent>();
    let mut _watcher: Option<FileWatcher> = None;

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
            let _ = app.tab_mut().refresh_diff();
            app.watch_message = Some(format!(
                "{} file{} changed",
                count,
                if count == 1 { "" } else { "s" }
            ));
            app.watch_message_ticks = 0;
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
    match key.code {
        // Quit
        KeyCode::Char('q') => {
            app.should_quit = true;
        }

        // File navigation
        KeyCode::Char('j') | KeyCode::Down => app.tab_mut().next_file(),
        KeyCode::Char('k') | KeyCode::Up => app.tab_mut().prev_file(),

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

        // Mode switching
        KeyCode::Char('1') => app.tab_mut().set_mode(DiffMode::Branch),
        KeyCode::Char('2') => app.tab_mut().set_mode(DiffMode::Unstaged),
        KeyCode::Char('3') => app.tab_mut().set_mode(DiffMode::Staged),

        // Search
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Search;
            app.tab_mut().search_query.clear();
        }

        // Toggle watch mode
        KeyCode::Char('w') => {
            if app.watching {
                // Stop watching
                *watcher = None;
                app.watching = false;
                app.watch_message = Some("Watch stopped".to_string());
                app.watch_message_ticks = 0;
            } else {
                // Start watching
                let root_str = app.tab().repo_root.clone();
                let root = Path::new(&root_str);
                match FileWatcher::new(root, 500, watch_tx.clone()) {
                    Ok(w) => {
                        *watcher = Some(w);
                        app.watching = true;
                        app.watch_message = Some("Watching for changes...".to_string());
                        app.watch_message_ticks = 0;
                    }
                    Err(e) => {
                        app.watch_message = Some(format!("Watch error: {}", e));
                        app.watch_message_ticks = 0;
                    }
                }
            }
        }

        // Open in editor
        KeyCode::Char('e') => {
            app.tab().open_in_editor()?;
        }

        // Refresh
        KeyCode::Char('r') => {
            app.tab_mut().refresh_diff()?;
            app.watch_message = Some("Refreshed".to_string());
            app.watch_message_ticks = 0;
        }

        // ── Tab keybinds ──

        // Next tab
        KeyCode::Char(']') => {
            app.next_tab();
        }

        // Previous tab
        KeyCode::Char('[') => {
            app.prev_tab();
        }

        // Close tab
        KeyCode::Char('x') => {
            app.close_tab();
        }

        // ── Repo keybinds ──

        // Worktree picker
        KeyCode::Char('t') => {
            app.open_worktree_picker()?;
        }

        // Directory browser
        KeyCode::Char('o') => {
            app.open_directory_browser();
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
