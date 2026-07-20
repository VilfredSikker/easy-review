mod input;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    cursor::Show,
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use er_engine::app::{self, App, InputMode};
use er_engine::{github, uninstall, watch};
use input::{
    handle_comment_input, handle_commit_input, handle_confirm_input, handle_filter_input,
    handle_normal_input, handle_overlay_input, handle_remote_url_input, handle_search_input,
};
use ratatui::prelude::*;
use std::io::{self, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use watch::{FileWatcher, WatchEvent};

/// Terminal UI for reviewing git diffs
#[derive(Parser)]
#[command(name = "er", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Repository paths to open (defaults to current directory)
    paths: Vec<String>,

    /// Open a specific PR by number (from the current repo)
    #[arg(long)]
    pr: Option<u64>,

    /// Pre-apply a file filter expression (e.g. '+*.rs,-*.lock')
    #[arg(long)]
    filter: Option<String>,

    /// Review a PR from any directory without a local clone
    #[arg(long)]
    remote: bool,

    /// Override the base branch to diff against (useful for stacked branches)
    #[arg(long)]
    target: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Remove Easy Review config, review data, and installed apps from this machine
    Uninstall {
        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Print what would be removed without deleting anything
        #[arg(long)]
        dry_run: bool,
        /// Keep managed review storage (`…/easy-review`)
        #[arg(long)]
        keep_data: bool,
        /// Keep `~/.config/er`
        #[arg(long)]
        keep_config: bool,
        /// Keep the `er` binary and desktop app bundle
        #[arg(long)]
        keep_apps: bool,
    },
}

/// Restore the terminal before the default panic handler runs, so a panic
/// inside the event loop doesn't leave the shell in raw mode with no cursor.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
        default_hook(info);
    }));
}

fn run_uninstall(
    yes: bool,
    dry_run: bool,
    keep_data: bool,
    keep_config: bool,
    keep_apps: bool,
) -> Result<()> {
    let opts = uninstall::UninstallOptions {
        remove_config: !keep_config,
        remove_data: !keep_data,
        remove_cache: true,
        remove_binaries: !keep_apps,
        remove_desktop_app: !keep_apps,
    };

    let plan = uninstall::plan(&opts);
    let existing: Vec<_> = plan.iter().filter(|t| t.exists).cloned().collect();

    println!("Easy Review uninstall");
    println!();
    if plan.is_empty() {
        println!("Nothing selected to remove.");
        return Ok(());
    }

    for t in &plan {
        let mark = if t.exists { "•" } else { "·" };
        let status = if t.exists { "" } else { " (not present)" };
        println!("  {mark} {}{status}", t.description());
    }
    println!();

    if existing.is_empty() {
        println!("Nothing installed to remove.");
        return Ok(());
    }

    if dry_run {
        println!("Dry run — no changes made.");
        return Ok(());
    }

    if !yes {
        print!("Type uninstall to confirm: ");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if line.trim() != "uninstall" {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let report = uninstall::execute(&existing);
    if !report.deferred.is_empty() {
        uninstall::schedule_deferred_removal(&report.deferred)
            .map_err(|e| anyhow::anyhow!("Could not schedule removal of in-use paths: {e}"))?;
    }
    for line in report.summary_lines() {
        println!("{line}");
    }

    if !report.deferred.is_empty() {
        println!();
        println!("Some paths are in use by this process and will be removed after it exits.");
    }

    if !report.is_success() {
        anyhow::bail!("Uninstall completed with errors");
    }

    println!();
    println!("Done. Easy Review data and apps have been removed.");
    Ok(())
}

fn main() -> Result<()> {
    er_engine::env_path::init_cli_path();
    install_panic_hook();
    let cli = Cli::parse();

    if let Some(Commands::Uninstall {
        yes,
        dry_run,
        keep_data,
        keep_config,
        keep_apps,
    }) = cli.command
    {
        return run_uninstall(yes, dry_run, keep_data, keep_config, keep_apps);
    }

    // Reject conflicting --pr and PR URL arguments
    if cli.pr.is_some() && cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
        anyhow::bail!("Cannot use --pr together with a PR URL argument");
    }

    // Validate --remote flag
    if cli.remote {
        if cli.pr.is_some() {
            anyhow::bail!("Cannot use --remote together with --pr");
        }
        if !cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
            anyhow::bail!("--remote requires a GitHub PR URL argument");
        }
    }

    // --target conflicts with PR-driven base resolution
    if cli.target.is_some() {
        if cli.pr.is_some() {
            anyhow::bail!("Cannot use --target together with --pr");
        }
        if cli.remote {
            anyhow::bail!("Cannot use --target together with --remote");
        }
        if cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
            anyhow::bail!("Cannot use --target together with a PR URL argument");
        }
    }

    // Remote mode: review PR(s) from GitHub API without local clone
    if cli.remote {
        github::ensure_gh_installed()?;
        let urls: Vec<&String> = cli
            .paths
            .iter()
            .filter(|p| github::is_github_pr_url(p))
            .collect();

        let first_url = urls[0];
        let pr_ref = github::parse_github_pr_url(first_url)
            .ok_or_else(|| anyhow::anyhow!("Invalid GitHub PR URL: {}", first_url))?;
        let tab = app::TabState::new_remote(&pr_ref)?;
        let pr_data = github::gh_pr_overview_remote(&pr_ref.owner, &pr_ref.repo, pr_ref.number);
        let mut app = App::new_remote(tab, pr_data);
        app.tab_mut().reload_remote_comments();

        // Open additional remote PR tabs
        for url in &urls[1..] {
            if let Err(e) = app.open_remote_url(url) {
                eprintln!("Warning: failed to open {}: {}", url, e);
            }
        }

        // Apply --filter flag if provided
        if let Some(ref filter_expr) = cli.filter {
            app.tab_mut().apply_filter_expr(filter_expr);
        }

        let mut highlighter = ui::highlight::Highlighter::new();

        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = run_app(&mut terminal, &mut app, &mut highlighter, None, None);

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        if let Err(err) = result {
            eprintln!("Error: {:?}", err);
        }

        // Print resume hint for remote sessions
        print_resume_hint(&app);

        return Ok(());
    }

    // Init app state (detects repo, branch, base branch, runs initial diff)
    let mut app = App::new_with_args(&cli.paths)?;

    // Initialize theme from config
    ui::themes::set_theme_by_name(&app.config.display.theme);

    // Apply --target flag: override base branch for all local tabs
    if let Some(ref target) = cli.target {
        for tab in &mut app.tabs {
            tab.base_branch = target.clone();
            tab.refresh_diff()?;
        }
    }

    // Handle --pr flag: fetch PR head ref and diff against it without checkout
    if let Some(pr_number) = cli.pr {
        github::ensure_gh_installed()?;
        let repo_root = app.tab().repo_root.clone();
        let head_ref = github::fetch_pr_head(pr_number, &repo_root)?;
        let base = github::gh_pr_base_branch(pr_number, &repo_root)?;
        let base = github::ensure_base_ref_available(&repo_root, &base)?;
        let head_branch = github::gh_pr_head_branch_name(pr_number, &repo_root)
            .unwrap_or_else(|_| format!("pr/{}", pr_number));
        let tab = app.tab_mut();
        tab.base_branch = base;
        tab.pr_head_ref = Some(head_ref);
        tab.pr_number = Some(pr_number);
        tab.current_branch = head_branch;
        tab.refresh_diff()?;
    }

    // Apply --filter flag if provided
    if let Some(ref filter_expr) = cli.filter {
        app.tab_mut().apply_filter_expr(filter_expr);
    }

    // Restore previous session if diff hash matches
    for tab in &mut app.tabs {
        tab.restore_session();
    }

    // Hint + PR data: check for PR in background (avoids blocking startup on network)
    let (hint_rx, pr_data_rx) =
        if cli.pr.is_none() && !cli.paths.iter().any(|p| github::is_github_pr_url(p)) {
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
                    if let Some(data) = github::gh_pr_overview(&repo_root, Some(pr_num)) {
                        let _ = pr_tx.send(data);
                    }
                }
            });
            (Some(hint_rx), Some(pr_rx))
        } else {
            // For --pr flag or PR URL, fetch PR data synchronously (already in the right state)
            let repo_root = app.tab().repo_root.clone();
            let pr_number_for_data = app.tab().pr_number;
            let pr_data = github::gh_pr_overview(&repo_root, pr_number_for_data);
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
    let result = run_app(
        &mut terminal,
        &mut app,
        &mut highlighter,
        hint_rx,
        pr_data_rx,
    );

    // Cleanup (the panic hook covers the panic path)
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {:?}", err);
    }

    print_resume_hint(&app);

    Ok(())
}

/// Print a dim `er <args>` hint so the user can quickly reopen the same session.
fn print_resume_hint(app: &App) {
    let has_remote = app.tabs.iter().any(|t| t.remote_repo.is_some());
    if app.tabs.len() > 1 || has_remote {
        let has_local = app.tabs.iter().any(|t| t.remote_repo.is_none());
        let args: Vec<String> = app
            .tabs
            .iter()
            .map(|t| {
                if let (Some(slug), Some(n)) = (&t.remote_repo, t.pr_number) {
                    format!("https://github.com/{}/pull/{}", slug, n)
                } else {
                    t.repo_root.clone()
                }
            })
            .collect();
        // Add --remote flag when all tabs are remote
        let prefix = if has_remote && !has_local {
            "er --remote"
        } else {
            "er"
        };
        eprintln!("\x1b[2m{} {}\x1b[0m", prefix, args.join(" "));
    }
}

fn run_app<B: Backend<Error: Send + Sync + 'static>>(
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

    // Session auto-save: debounced at ~2 seconds
    let mut session_dirty = false;
    let mut session_save_deadline = Instant::now();

    // Start watching by default (disabled in remote mode — no local files to watch)
    let root_str = app.tab().repo_root.clone();
    let root = std::path::Path::new(&root_str);
    // If FileWatcher::new fails, watch mode is silently disabled (app.watching stays false).
    let mut _watcher: Option<FileWatcher> = if app.tab().is_remote() {
        None
    } else {
        match FileWatcher::new(root, 500, watch_tx.clone()) {
            Ok(w) => {
                app.watching = true;
                Some(w)
            }
            Err(_) => None,
        }
    };

    loop {
        // Update terminal width for resize calculations
        if let Ok(size) = terminal.size() {
            app.last_terminal_width = size.width;
        }

        // Draw
        terminal.draw(|f| ui::draw(f, app, hl))?;

        // Poll for events with a timeout (lets us process watch events too)
        if event::poll(Duration::from_millis(50))? {
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
                        InputMode::RemoteUrl => handle_remote_url_input(app, key)?,
                        InputMode::Normal => {
                            handle_normal_input(app, key, &watch_tx, &mut _watcher)?
                        }
                    }
                }
            }

            // Mark session dirty after any key input
            session_dirty = true;
            session_save_deadline = Instant::now() + Duration::from_secs(2);
        }

        // Check for file watch events (non-blocking) — debounced
        // Drain all pending events each tick to avoid accumulation under rapid changes.
        while let Ok(WatchEvent::FilesChanged(paths)) = watch_rx.try_recv() {
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
            let unmark_count = std::mem::replace(&mut app.tab_mut().pending_unmark_count, 0);
            if unmark_count > 0 {
                app.notify(&format!(
                    "{} reviewed file{} auto-unmarked (diff changed)",
                    unmark_count,
                    if unmark_count == 1 { "" } else { "s" },
                ));
            } else {
                app.notify(&format!(
                    "{} file{} changed",
                    count,
                    if count == 1 { "" } else { "s" },
                ));
            }
        }

        // Check for .er-* file changes (throttled: every 10 ticks ≈ 1s)
        app.ai_poll_counter = app.ai_poll_counter.wrapping_add(1);
        if app.ai_poll_counter.is_multiple_of(10) && app.tab_mut().check_ai_files_changed() {
            app.notify("✓ AI data refreshed");
        }

        // Poll background commands for completion
        app.check_commands();

        // Drain agent log entries from background threads
        app.drain_agent_log();

        // Rescan watched files (every 50 ticks ≈ 5s)
        if !app.tab().is_remote() && app.ai_poll_counter.is_multiple_of(50) {
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

        // Debounced session auto-save (~2s after last change)
        if !app.tab().is_remote() && session_dirty && Instant::now() >= session_save_deadline {
            session_dirty = false;
            app.tab().save_session();
        }

        // Tick — used for auto-clearing notifications
        app.tick();

        if app.should_quit {
            // Save session on quit
            if !app.tab().is_remote() {
                app.tab().save_session();
            }
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use er_engine::ai::PanelContent;
    use er_engine::app::{ConfirmAction, InputMode};
    use er_engine::git::{DiffFile, DiffHunk, DiffLine, FileStatus, LineType};
    use std::sync::mpsc;

    // ── Test helpers ──

    fn make_app(files: Vec<DiffFile>) -> App {
        App::new_for_test(files)
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
        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "Ctrl+q must not change input_mode"
        );
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

    // ── d scrolls down (bare and Ctrl+d) ──

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
    fn bare_d_scrolls_diff_when_no_focused_comment() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        assert_eq!(app.tab().diff_scroll, 0);
        send_key(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(
            app.tab().diff_scroll,
            10,
            "bare d must scroll diff down by 10"
        );
        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "bare d must not change input_mode"
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
    fn bare_x_triggers_delete_confirm_when_comment_focused() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        // Add a question to AI state so find_comment + can_delete succeeds
        app.tab_mut().ai.questions = Some(er_engine::ai::ErQuestions {
            version: 1,
            diff_hash: String::new(),
            questions: vec![er_engine::ai::ReviewQuestion {
                id: "q-abc".to_string(),
                timestamp: String::new(),
                file: "test.rs".to_string(),
                hunk_index: Some(0),
                line_start: None,
                line_end: None,
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
                promoted_to: None,
                finding_ref: None,
            }],
        });
        app.tab_mut().focused_comment_id = Some("q-abc".to_string());
        send_key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(
            app.input_mode,
            InputMode::Confirm(ConfirmAction::DeleteComment {
                comment_id: "q-abc".to_string()
            }),
            "bare x with focused comment must enter Confirm(DeleteComment)"
        );
    }

    #[test]
    fn bare_x_closes_tab_when_no_comment_focused() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        // focused_comment_id is None by default — x falls through to close_tab
        // With only one tab, close_tab is a no-op (doesn't crash)
        send_key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "bare x with no focused comment must stay in Normal mode"
        );
    }

    // ── u scrolls up (bare and Ctrl+u) ──

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
    fn bare_u_scrolls_diff_up() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        app.tab_mut().diff_scroll = 20;
        send_key(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(
            app.tab().diff_scroll,
            10,
            "bare u must scroll diff up by 10"
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
    fn bang_toggles_unreviewed_filter_on() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        assert!(!app.tab().show_unreviewed_only);
        send_key(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
        assert!(
            app.tab().show_unreviewed_only,
            "! must toggle show_unreviewed_only to true"
        );
    }

    #[test]
    fn bang_does_not_scroll_diff() {
        let mut app = make_app(vec![make_file_with_hunk()]);
        app.tab_mut().diff_scroll = 20;
        send_key(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
        assert_eq!(app.tab().diff_scroll, 20, "! must not change diff_scroll");
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
        assert_eq!(app.tab().panel_scroll, 5, "Ctrl+j must not scroll panel");
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
        assert_eq!(app.tab().panel_scroll, 5, "Ctrl+k must not scroll panel");
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
