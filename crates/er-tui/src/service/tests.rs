//! The flag-on path's tests: what the start opens, what each frame draws
//! beside the App path's frame, and that no key waits on an actor. The tests
//! build the App to compare against; the modules they test never name it.

use std::path::{Path, PathBuf};
use std::process::Command as Git;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::ports::Ports;
use er_engine::tabs_record::TabKind;
use er_engine::TabKey;
use er_service::{Command, ServiceHost, WorkspaceError};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

use super::shell::Shell;
use super::startup::{self, Args};
use crate::screen::UNPORTED;
use crate::ui::highlight::Highlighter;

const WAIT: Duration = Duration::from_secs(20);
const WIDTH: u16 = 120;
const HEIGHT: u16 = 30;

/// The flag-on modules, as text.
const SOURCES: [(&str, &str); 4] = [
    ("mod.rs", include_str!("mod.rs")),
    ("shell.rs", include_str!("shell.rs")),
    ("startup.rs", include_str!("startup.rs")),
    ("tui_view.rs", include_str!("tui_view.rs")),
];

/// No flag-on module reaches the App, fetches a PR's refs itself, or waits on
/// an actor: the service owns the domain, and a key never blocks the loop.
#[test]
fn flag_on_modules_name_no_app_and_never_wait() {
    let banned = [
        "er_engine::app",
        "app::",
        "fetch_pr_head",
        "ensure_base_ref_available",
        "send_and_wait",
        "recv_timeout",
    ];
    for (name, text) in SOURCES {
        // Joined into one line, so a grouped import or a split call is seen.
        let flat: String = text.split_whitespace().collect();
        for needle in banned {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
        let grouped = flat
            .split("er_engine::{")
            .skip(1)
            .filter_map(|rest| rest.split('}').next())
            .any(|group| {
                group
                    .split(',')
                    .any(|item| item == "app" || item.starts_with("app::"))
            });
        assert!(!grouped, "{name} imports the App in a group");
    }
}

// The lint gate reads this file alone, without the module's `cfg(test)`.
#[cfg(test)]
fn git(dir: &Path, args: &[&str]) {
    let out = Git::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repo on `feature` over `main`, where each of `files` files changes in
/// two hunks. Past 500 files a diff this size loads with stubs.
fn fixture(files: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    let body = |changed: bool| {
        (0..40)
            .map(|line| match line {
                3 | 30 if changed => format!("let changed_{line} = {line}; // edited\n"),
                _ => format!("let line_{line} = {line};\n"),
            })
            .collect::<String>()
    };
    let write_all = |changed: bool| {
        for idx in 0..files {
            let path = root.join(format!("src/f{idx:03}.rs"));
            std::fs::create_dir_all(root.join("src")).unwrap_or_else(|e| panic!("{e}"));
            std::fs::write(path, body(changed)).unwrap_or_else(|e| panic!("{e}"));
        }
    };
    write_all(false);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "base"]);
    git(root, &["checkout", "-q", "-b", "feature"]);
    write_all(true);
    git(root, &["commit", "-q", "-am", "change"]);
    dir
}

fn path_of(dir: &tempfile::TempDir) -> String {
    let canonical: PathBuf = std::fs::canonicalize(dir.path()).unwrap_or_else(|e| panic!("{e}"));
    canonical.to_string_lossy().into_owned()
}

fn shell(args: &Args, config_dir: &Path) -> Shell {
    let ports = Ports::cli();
    let start = startup::plan(args, &*ports.git).unwrap_or_else(|e| panic!("{e:#}"));
    let (tx, events) = mpsc::channel();
    let host = ServiceHost::new(ports, config_dir.join("config.toml")).with_events(tx);
    Shell::start(host, events, &start).unwrap_or_else(|e| panic!("{e:#}"))
}

/// Ticks the loop until `ready` holds.
fn settle(shell: &mut Shell, ready: impl Fn(&Shell) -> bool) {
    assert!(settle_within(shell, WAIT, ready), "the shell never settled");
}

/// Ticks the loop until `ready` holds or `wait` passes; answers which.
fn settle_within(shell: &mut Shell, wait: Duration, ready: impl Fn(&Shell) -> bool) -> bool {
    let deadline = Instant::now() + wait;
    while !ready(shell) && Instant::now() < deadline {
        shell.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    shell.tick(Instant::now());
    ready(shell)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn shell_frame(shell: &mut Shell, highlighter: &mut Highlighter) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap_or_else(|e| panic!("{e}"));
    terminal
        .draw(|f| shell.draw(f, highlighter))
        .unwrap_or_else(|e| panic!("{e}"));
    terminal.backend().buffer().clone()
}

fn app_frame(app: &er_engine::app::App, highlighter: &mut Highlighter) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap_or_else(|e| panic!("{e}"));
    let mut cache = crate::screen::ViewCache::default();
    terminal
        .draw(|f| crate::ui::draw(f, &crate::screen::from_app(app, &mut cache), highlighter))
        .unwrap_or_else(|e| panic!("{e}"));
    terminal.backend().buffer().clone()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(usize::from(WIDTH))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

/// Every row below the single-tab top bar, which carries the reviewed count
/// the flag-on path leaves out, matches in text and style.
fn assert_parity(app: &Buffer, flag_on: &Buffer, step: &str) {
    const TOP: usize = 2;
    let (app_rows, flag_rows) = (rows(app), rows(flag_on));
    for row in TOP..usize::from(HEIGHT) {
        assert_eq!(
            app_rows[row],
            flag_rows[row],
            "{step}: row {row} differs\napp:\n{}\nflag-on:\n{}",
            app_rows.join("\n"),
            flag_rows.join("\n")
        );
        let at =
            |buffer: &Buffer, col: usize| buffer.content[row * usize::from(WIDTH) + col].style();
        for col in 0..usize::from(WIDTH) {
            assert_eq!(
                at(app, col),
                at(flag_on, col),
                "{step}: style at row {row}, col {col}"
            );
        }
    }
}

/// The App path, on the same repo, with context expansion off: that refetch
/// is a service command no entry has ported yet.
fn app_on(path: &str) -> er_engine::app::App {
    let mut app = er_engine::app::App::new_with_args(&[path.to_string()], Ports::cli())
        .unwrap_or_else(|e| panic!("{e:#}"));
    app.config.display.auto_context_threshold = 0;
    app
}

fn press_app(app: &mut er_engine::app::App, code: KeyCode) {
    let (tx, _rx) = mpsc::channel();
    let mut cache = crate::screen::ViewCache::default();
    crate::handle_key(app, key(code), &tx, &mut None, &mut cache).unwrap_or_else(|e| panic!("{e}"));
}

fn loaded(shell: &Shell) -> bool {
    shell.active_content().is_some()
}

/// The start opens the arguments' tabs and loads the first; the files, the
/// reviewed marks, the hunks and the status bar draw as on the App path, and
/// file, hunk, line and scroll moves land where the App's do.
#[test]
fn flag_on_frames_match_the_app_frames() {
    let repo = fixture(3);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        // A reviewed mark, saved where the service reads it.
        app.toggle_reviewed().unwrap_or_else(|e| panic!("{e}"));
        app.clear_notification();
        let args = Args {
            paths: vec![path.clone()],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        assert_parity(
            &app_frame(&app, &mut hl),
            &shell_frame(&mut shell, &mut hl),
            "start",
        );
        let steps = [
            KeyCode::Char('k'),
            KeyCode::Char('n'),
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Char('N'),
            KeyCode::PageDown,
            KeyCode::Char('U'),
            KeyCode::Char('j'),
        ];
        for code in steps {
            // Each path lets the last step's message go before the next key.
            app.clear_notification();
            shell.tick(Instant::now() + Duration::from_secs(60));
            press_app(&mut app, code);
            shell.handle_key(key(code));
            shell.tick(Instant::now());
            let step = format!("{code:?}");
            assert_parity(
                &app_frame(&app, &mut hl),
                &shell_frame(&mut shell, &mut hl),
                &step,
            );
        }
    });
}

/// A stub the selection reaches parses on the service, and draws its hunks
/// as the App, which parses it in place, draws them.
#[test]
fn a_selected_stub_parses_and_draws_like_the_app() {
    let repo = fixture(520);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path);
        let args = Args {
            paths: vec![path.clone()],
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        settle(&mut shell, loaded);
        let stub = |shell: &Shell, idx: usize| {
            shell
                .active_content()
                .is_some_and(|content| super::tui_view::is_stub(&content.diff.files[idx]))
        };
        assert!(stub(&shell, 1), "the fixture loaded no stub");
        press_app(&mut app, KeyCode::Char('k'));
        shell.handle_key(key(KeyCode::Char('k')));
        // Without a parse the stub never fills; the frames then differ.
        settle_within(&mut shell, Duration::from_secs(5), |shell| !stub(shell, 1));
        let mut hl = Highlighter::new();
        assert_parity(
            &app_frame(&app, &mut hl),
            &shell_frame(&mut shell, &mut hl),
            "stub",
        );
    });
}

/// The start lists every tab and loads the first; a flag with no service
/// route says so, and a load for a key the registry does not list is refused.
#[test]
fn the_start_opens_listed_tabs_and_reports_unported_flags() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let ports = Ports::cli();
        let pr = startup::plan(
            &Args {
                paths: vec![path.clone()],
                pr: Some(7),
                ..Args::default()
            },
            &*ports.git,
        )
        .unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(pr.tabs[0].tab.kind, TabKind::LocalPr);
        assert_eq!(pr.tabs[0].tab.pr_number, Some(7));
        assert!(pr.tabs[0].fetch_pr_refs, "--pr leaves its refs unfetched");
        let remote = startup::plan(
            &Args {
                paths: vec!["https://github.com/acme/widgets/pull/9".into()],
                remote: true,
                ..Args::default()
            },
            &*ports.git,
        )
        .unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!(remote.tabs[0].tab.kind, TabKind::RemotePr);
        assert_eq!(
            remote.tabs[0].tab.workspace_key(),
            er_engine::tab_identity::workspace_key(None, "acme/widgets")
        );

        let args = Args {
            paths: vec![path.clone(), path.clone()],
            target: Some("develop".into()),
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
        let notice = shell.view().notification.clone().unwrap_or_default();
        assert!(
            notice.contains(&format!("--target is {UNPORTED}")),
            "{notice:?}"
        );
        let tabs = shell.host().registry().tabs_view();
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.active, tabs.key_at(0));
        settle(&mut shell, loaded);

        let tab = tabs.tabs[0].descriptor.clone();
        let refused = shell
            .host()
            .post(
                &tab.workspace_key(),
                Command::LoadTab {
                    key: TabKey::next(),
                    tab,
                    mode: None,
                },
            )
            .recv_timeout(WAIT)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            refused,
            Err(WorkspaceError::Failed(
                "the tab closed before its load".into()
            ))
        );
    });
}
