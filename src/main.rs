mod ai;
mod app;
mod git;
mod ui;
mod watch;

use anyhow::Result;
use app::{App, DiffMode, InputMode};
use crate::ai::{PanelTab, ViewMode};
use crate::ai::agent::{self, AgentMessage, MessageRole};
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
                        InputMode::Comment => handle_comment_input(app, key)?,
                        InputMode::AgentPrompt => handle_agent_input(app, key)?,
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
            app.notify(&format!(
                "{} file{} changed",
                count,
                if count == 1 { "" } else { "s" }
            ));
        }

        // Check for .er-* file changes (every tick)
        if app.tab_mut().check_ai_files_changed() {
            app.notify("✓ AI data refreshed");
        }

        // Poll agent child process for streaming output
        poll_agent(app);

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
            app.notify("Refreshed");
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
            let mode = app.tab().ai.view_mode.label();
            app.notify(&format!("View: {}", mode));
        }
        KeyCode::Char('V') => {
            app.tab_mut().ai.cycle_view_mode_prev();
            let mode = app.tab().ai.view_mode.label();
            app.notify(&format!("View: {}", mode));
        }

        // Open agent prompt
        KeyCode::Char('a') => {
            app.tab_mut().ai.view_mode = ViewMode::SidePanel;
            app.tab_mut().ai.panel_tab = PanelTab::Agent;
            app.tab_mut().update_agent_context();
            app.input_mode = InputMode::AgentPrompt;
        }

        // Tab to toggle panel tabs (only in SidePanel mode)
        KeyCode::Tab => {
            if app.tab().ai.view_mode == ViewMode::SidePanel {
                let tab = app.tab_mut();
                tab.ai.panel_tab = match tab.ai.panel_tab {
                    PanelTab::Review => PanelTab::Agent,
                    PanelTab::Agent => PanelTab::Review,
                };
            }
        }

        // Clear search filter
        KeyCode::Esc => {
            if app.tab().ai.view_mode == ViewMode::SidePanel
                && app.tab().ai.panel_tab == PanelTab::Agent
            {
                app.tab_mut().ai.view_mode = ViewMode::Default;
            } else if !app.tab().search_query.is_empty() {
                app.tab_mut().search_query.clear();
            }
        }

        _ => {}
    }
    Ok(())
}

fn handle_ai_review_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Quit
        KeyCode::Char('q') => {
            app.should_quit = true;
        }

        // Navigation within focused column
        KeyCode::Char('j') | KeyCode::Down => {
            app.tab_mut().ai.review_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.tab_mut().ai.review_prev();
        }

        // Switch focus between left/right columns
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
            app.tab_mut().ai.review_toggle_focus();
        }
        KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
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

        // Scroll (for long content)
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_down(10);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_up(10);
        }
        KeyCode::PageDown => app.tab_mut().scroll_down(20),
        KeyCode::PageUp => app.tab_mut().scroll_up(20),

        // Cycle view mode (v forward, V backward)
        KeyCode::Char('v') => {
            app.tab_mut().ai.cycle_view_mode();
            let mode = app.tab().ai.view_mode.label();
            app.notify(&format!("View: {}", mode));
        }
        KeyCode::Char('V') => {
            app.tab_mut().ai.cycle_view_mode_prev();
            let mode = app.tab().ai.view_mode.label();
            app.notify(&format!("View: {}", mode));
        }

        // Esc goes back to default view
        KeyCode::Esc => {
            app.tab_mut().ai.view_mode = ViewMode::Default;
            app.notify("View: DEFAULT");
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

fn handle_agent_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            let config = agent::load_agent_config(&app.tab().repo_root);
            app.tab_mut().spawn_agent(&config)?;
        }
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(ref mut child) = app.tab_mut().ai.agent.child {
                let _ = child.kill();
            }
            app.tab_mut().ai.agent.is_running = false;
            app.tab_mut().ai.agent.child = None;
            app.tab_mut().ai.agent.messages.push(AgentMessage {
                role: MessageRole::System,
                text: "cancelled".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().ai.agent.messages.clear();
            app.tab_mut().ai.agent.scroll = 0;
        }
        KeyCode::Char(c) => {
            app.tab_mut().ai.agent.input.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().ai.agent.input.pop();
        }
        _ => {}
    }
    Ok(())
}

fn poll_agent(app: &mut App) {
    use std::io::Read;

    let agent = &mut app.tab_mut().ai.agent;
    if !agent.is_running {
        return;
    }

    let child = match agent.child.as_mut() {
        Some(c) => c,
        None => return,
    };

    // Try reading stdout
    if let Some(ref mut stdout) = child.stdout {
        let mut buf = [0u8; 4096];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => {
                    finalize_agent_response(agent);
                    return;
                }
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    agent.partial_response.push_str(&chunk);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(_) => {
                    finalize_agent_response(agent);
                    return;
                }
            }
        }
    }

    // Check if process exited
    if let Ok(Some(_status)) = child.try_wait() {
        if let Some(ref mut stdout) = child.stdout {
            let mut remaining = String::new();
            let _ = stdout.read_to_string(&mut remaining);
            agent.partial_response.push_str(&remaining);
        }
        finalize_agent_response(agent);
    }
}

fn finalize_agent_response(agent: &mut crate::ai::agent::AgentState) {
    let text = std::mem::take(&mut agent.partial_response)
        .trim()
        .to_string();

    if !text.is_empty() {
        agent.messages.push(AgentMessage {
            role: MessageRole::Agent,
            text,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }

    agent.is_running = false;
    agent.child = None;
}
