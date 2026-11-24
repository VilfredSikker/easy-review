//! The flag-on path's tests: what the start opens, what each frame draws
//! beside the App path's frame, and that no key waits on an actor. The tests
//! build the App to compare against; the modules they test never name it.

use std::path::{Path, PathBuf};
use std::process::Command as Git;
use std::sync::{mpsc, Arc};
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
use super::startup::{self, Args, Start};
use super::tui_view::Overlay;
use crate::ui::highlight::Highlighter;

pub(super) const WAIT: Duration = Duration::from_secs(20);
const WIDTH: u16 = 120;
const HEIGHT: u16 = 30;

/// The flag-on modules, as text.
pub(super) const SOURCES: [(&str, &str); 20] = [
    ("ai_picker.rs", include_str!("ai_picker.rs")),
    ("config_hub.rs", include_str!("config_hub.rs")),
    ("filter.rs", include_str!("filter.rs")),
    ("github.rs", include_str!("github.rs")),
    ("layers.rs", include_str!("layers.rs")),
    ("mod.rs", include_str!("mod.rs")),
    ("modes.rs", include_str!("modes.rs")),
    ("open_hub.rs", include_str!("open_hub.rs")),
    ("pr_probe.rs", include_str!("pr_probe.rs")),
    ("review_hub.rs", include_str!("review_hub.rs")),
    ("reviewed.rs", include_str!("reviewed.rs")),
    ("session.rs", include_str!("session.rs")),
    ("settings.rs", include_str!("settings.rs")),
    ("shell.rs", include_str!("shell.rs")),
    ("startup.rs", include_str!("startup.rs")),
    ("threads.rs", include_str!("threads.rs")),
    ("tui_view.rs", include_str!("tui_view.rs")),
    ("watched.rs", include_str!("watched.rs")),
    ("watcher.rs", include_str!("watcher.rs")),
    ("worktree.rs", include_str!("worktree.rs")),
];

/// No flag-on module reaches the App, fetches a PR's refs itself, writes or
/// discovers settings itself, writes the working tree itself, starts a
/// thread, or waits on an actor: the service owns the domain, and a key never
/// blocks the loop. Two modules may start a thread, and both only read: the
/// watched-file reader makes none of the calls a write takes, and every er-git
/// name it uses is a read; the PR probe's calls are checked in
/// `github_tests`.
#[test]
fn flag_on_modules_name_no_app_and_never_wait() {
    let banned = [
        "er_engine::app",
        "app::",
        "fetch_pr_head",
        "ensure_base_ref_available",
        "send_and_wait",
        "recv_timeout",
        "discover_and_cache",
        "model_discovery::",
        "save_config",
        "apply_config_field",
        "stage_file",
        "unstage_file",
        "stage_all(",
        "git.commit(",
        "git.push(",
    ];
    for (name, text) in SOURCES {
        // Joined into one line, so a grouped import or a split call is seen.
        let flat: String = text.split_whitespace().collect();
        for needle in banned {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
        if name == "watched.rs" {
            for write in WATCHED_WRITES {
                assert!(!flat.contains(write), "{name} names {write}");
            }
            let tokens = code_tokens(text);
            for write in WATCHED_WRITE_NAMES {
                assert!(!tokens.contains(&write), "{name} names {write}");
            }
            for statement in use_statements(text) {
                assert!(
                    !statement.iter().any(|token| token == "as"),
                    "{name} renames an import: {statement:?}"
                );
            }
            for named in er_git_names(&flat) {
                assert!(
                    WATCHED_READS.contains(&named),
                    "{name} names er-git's {named}"
                );
            }
        } else if name != "pr_probe.rs" {
            for spawn in ["thread::spawn", "thread::Builder"] {
                assert!(!flat.contains(spawn), "{name} names {spawn}");
            }
        }
        let code = text
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""));
        let bare_app = code.flat_map(|line| line.split(|c: char| !c.is_alphanumeric() && c != '_'));
        assert!(
            !bare_app.into_iter().any(|word| word == "App"),
            "{name} names App"
        );
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

/// The calls the port's writes with common names take, whatever handle or
/// path comes before them, and the file system, which the reader never
/// names. Matched as calls, so a name that only contains `push` or `commit`
/// passes.
const WATCHED_WRITES: [&str; 12] = [
    ".commit(",
    "::commit(",
    ".push(",
    "::push(",
    ".fetch(",
    "::fetch(",
    "Git::",
    "std::fs",
    "fs::",
    "OpenOptions",
    "File::",
    "remove_dir",
];

/// The writes with names nothing else uses, matched as whole identifiers in
/// the code, so no handle, path or import spelling hides them.
const WATCHED_WRITE_NAMES: [&str; 10] = [
    "save_snapshot",
    "delete_watched_file",
    "diff_watched_file_snapshot",
    "update_ref",
    "shallow_clone",
    "stage_file",
    "unstage_file",
    "stage_all",
    "remove_file",
    "fs",
];

/// A source's identifiers, outside `//` comments.
fn code_tokens(text: &str) -> Vec<&str> {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .flat_map(|line| line.split(|c: char| !c.is_alphanumeric() && c != '_'))
        .filter(|token| !token.is_empty())
        .collect()
}

/// Each `use` statement's identifiers, so an import that renames a write
/// (`as`) is seen whatever its layout.
fn use_statements(text: &str) -> Vec<Vec<String>> {
    let code: Vec<&str> = text
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect();
    let code = code.join(" ");
    code.split(';')
        .filter_map(|piece| {
            let tokens = code_tokens(piece);
            let at = tokens.iter().rposition(|token| *token == "use")?;
            let statement = tokens.get(at + 1..).unwrap_or_default();
            Some(statement.iter().map(ToString::to_string).collect())
        })
        .collect()
}

/// The er-git names the watched-file reader may use.
const WATCHED_READS: [&str; 5] = [
    "Git",
    "WatchedFile",
    "discover_watched_files",
    "read_watched_snapshot_diff",
    "gitignored_paths",
];

/// The names a flattened source takes from er-git: each `er_git::` path or
/// group, and each method called on a `git` handle.
fn er_git_names<'a>(flat: &'a str) -> Vec<&'a str> {
    let ident = |rest: &'a str| -> &'a str {
        rest.split(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or_default()
    };
    let mut names = Vec::new();
    for rest in flat.split("er_git::").skip(1) {
        match rest
            .strip_prefix('{')
            .and_then(|group| group.split('}').next())
        {
            Some(group) => names.extend(group.split(',').map(ident)),
            None => names.push(ident(rest)),
        }
    }
    names.extend(flat.split("git.").skip(1).map(ident));
    names
}

// The lint gate reads this file alone, without the module's `cfg(test)`.
#[cfg(test)]
pub(super) fn git(dir: &Path, args: &[&str]) {
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
pub(super) fn fixture(files: usize) -> tempfile::TempDir {
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

pub(super) fn path_of(dir: &tempfile::TempDir) -> String {
    let canonical: PathBuf = std::fs::canonicalize(dir.path()).unwrap_or_else(|e| panic!("{e}"));
    canonical.to_string_lossy().into_owned()
}

pub(super) fn plan(args: &Args) -> Start {
    startup::plan(args, &*Ports::cli().git).unwrap_or_else(|e| panic!("{e:#}"))
}

pub(super) fn shell(args: &Args, config_dir: &Path) -> Shell {
    shell_on(test_ports(), &plan(args), config_dir)
}

/// A GitHub that answers nothing it was not scripted to: the start's PR
/// probe gets an error, which reads as unknown, and no test reaches `gh`.
pub(super) fn fake_github() -> Arc<dyn er_github::GitHub> {
    Arc::new(er_github::FakeGitHub::default())
}

/// The real git beside [`fake_github`].
pub(super) fn test_ports() -> Ports {
    Ports {
        git: Ports::cli().git,
        github: fake_github(),
    }
}

pub(super) fn shell_on(ports: Ports, start: &Start, config_dir: &Path) -> Shell {
    let (tx, events) = mpsc::channel();
    let git = Arc::clone(&ports.git);
    let host = ServiceHost::new(ports, config_dir.join("config.toml")).with_events(tx);
    let mut shell = Shell::start(host, events, start, git).unwrap_or_else(|e| panic!("{e:#}"));
    // The App these tests compare against runs no watcher.
    shell.watch_mut().on = false;
    shell
}

/// Ticks the loop until `ready` holds.
pub(super) fn settle(shell: &mut Shell, ready: impl Fn(&Shell) -> bool) {
    assert!(settle_within(shell, WAIT, ready), "the shell never settled");
}

/// Ticks the loop until `ready` holds or `wait` passes; answers which.
pub(super) fn settle_within(
    shell: &mut Shell,
    wait: Duration,
    ready: impl Fn(&Shell) -> bool,
) -> bool {
    let deadline = Instant::now() + wait;
    while !ready(shell) && Instant::now() < deadline {
        shell.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    shell.tick(Instant::now());
    ready(shell)
}

pub(super) fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Takes the Git hub's review row out, so the hub draws as the App's,
/// which has no review hub.
pub(super) fn without_review_row(shell: &mut Shell) {
    if let Some(super::tui_view::Overlay::GitHub(hub)) = shell.view_mut().overlay.as_mut() {
        hub.drop_review_row();
    }
}

pub(super) fn shell_frame(shell: &mut Shell, highlighter: &mut Highlighter) -> Buffer {
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

pub(super) fn rows(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(usize::from(WIDTH))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

/// Every cell matches in text and style.
pub(super) fn assert_parity(
    app: &er_engine::app::App,
    flag_on: &Buffer,
    hl: &mut Highlighter,
    step: &str,
) {
    let app = &app_frame(app, hl);
    let (app_rows, flag_rows) = (rows(app), rows(flag_on));
    for row in 0..usize::from(HEIGHT) {
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
pub(super) fn app_on(path: &str) -> er_engine::app::App {
    let mut app = er_engine::app::App::new_with_args(&[path.to_string()], Ports::cli())
        .unwrap_or_else(|e| panic!("{e:#}"));
    app.config.display.auto_context_threshold = 0;
    app
}

pub(super) fn press_app(app: &mut er_engine::app::App, code: KeyCode) {
    let (tx, _rx) = mpsc::channel();
    let mut cache = crate::screen::ViewCache::default();
    crate::handle_key(app, key(code), &tx, &mut None, &mut cache).unwrap_or_else(|e| panic!("{e}"));
}

pub(super) fn loaded(shell: &Shell) -> bool {
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
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "start");
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
            assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, &step);
        }
        // An overlay the view holds draws as the App draws the same one.
        app.open_export_picker();
        shell.view_mut().overlay = Some(Overlay::ExportPicker {
            include_comments: true,
            include_findings: true,
            include_questions: true,
            include_notes: true,
            selected: 0,
        });
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "overlay");
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
        // The first file is selected from the start, so it parses with no key.
        settle_within(&mut shell, Duration::from_secs(5), |shell| !stub(shell, 0));
        let mut hl = Highlighter::new();
        assert_parity(
            &app,
            &shell_frame(&mut shell, &mut hl),
            &mut hl,
            "stub-start",
        );
        press_app(&mut app, KeyCode::Char('k'));
        shell.handle_key(key(KeyCode::Char('k')));
        // Without a parse the stub never fills; the frames then differ.
        settle_within(&mut shell, Duration::from_secs(5), |shell| !stub(shell, 1));
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "stub");
    });
}

/// The start lists every tab and loads the first, and a load for a key the
/// registry does not list is refused.
#[test]
fn the_start_opens_listed_tabs_and_refuses_an_unlisted_load() {
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
            ..Args::default()
        };
        let mut shell = shell(&args, storage.path());
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

/// `--pr` opens a local PR tab, and once its first load lands the service
/// fetches the PR's head and base refs.
#[test]
fn a_pr_start_has_the_service_fetch_the_pr_refs() {
    let repo = fixture(1);
    let origin = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    git(origin.path(), &["init", "-q", "--bare"]);
    let remote = path_of(&origin);
    git(repo.path(), &["remote", "add", "origin", &remote]);
    git(
        repo.path(),
        &["push", "-q", "origin", "main", "feature:refs/pull/7/head"],
    );
    git(repo.path(), &["fetch", "-q", "origin"]);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let raw = "diff --git a/src/f000.rs b/src/f000.rs\n--- a/src/f000.rs\n+++ b/src/f000.rs\n@@ -1 +1 @@\n-a\n+b\n";
        let github = er_github::FakeGitHub::default()
            .with_pr_diff(Ok(raw.to_string()))
            .with_pr_base_branch_and_commits(Ok(("main".to_string(), Vec::new())))
            .with_fetch_pr_base_ref(Ok("origin/main".to_string()));
        let ports = Ports {
            git: Arc::new(er_git::CliGit),
            github: Arc::new(github),
        };
        let args = Args {
            paths: vec![path],
            pr: Some(7),
            ..Args::default()
        };
        let mut shell = shell_on(ports, &plan(&args), storage.path());
        let fetched = |shell: &Shell| {
            shell
                .active_content()
                .is_some_and(|content| content.pr_refs_fetched)
        };
        assert!(
            settle_within(&mut shell, WAIT, fetched),
            "the PR refs were not fetched: {:?}",
            shell.view().notification
        );
    });
}

/// A tab whose first load fails says why in place of the diff, and a start
/// notice stays up while the tab has drawn no content.
#[test]
fn a_failed_first_load_shows_its_error_and_the_notices() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let args = Args {
        paths: vec![path_of(&repo)],
        ..Args::default()
    };
    let mut start = plan(&args);
    // No flag says anything at the start on this build; a notice stands in.
    start.notices.push("A start notice".into());
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell_on(test_ports(), &start, storage.path());
        let mut hl = Highlighter::new();
        let mut text = |shell: &mut Shell| rows(&shell_frame(shell, &mut hl)).join("\n");
        let deadline = Instant::now() + WAIT;
        while !text(&mut shell).contains("did not load") && Instant::now() < deadline {
            shell.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        shell.tick(Instant::now() + Duration::from_secs(60));
        assert!(
            shell.view().notification.is_some(),
            "a message expired while loading"
        );
        let frame = text(&mut shell);
        assert!(frame.contains("This tab did not load"), "{frame}");
        assert!(frame.contains("A start notice"), "{frame}");
    });
}

/// Tabs after the first load when first shown, and the tab keys run on a
/// frame with no content: a failed tab can be left, and shown again it loads
/// again.
#[test]
fn tab_keys_load_later_tabs_and_leave_a_failed_one() {
    let (first, second, broken) = (fixture(1), fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let args = Args {
        paths: vec![path_of(&first), path_of(&second), path_of(&broken)],
        ..Args::default()
    };
    let start = plan(&args);
    std::fs::remove_dir_all(broken.path().join(".git")).unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell_on(test_ports(), &start, storage.path());
        let mut hl = Highlighter::new();
        let mut text = |shell: &mut Shell| rows(&shell_frame(shell, &mut hl)).join("\n");
        let tabs = shell.host().registry().tabs_view();
        let keys: Vec<TabKey> = tabs.tabs.iter().map(|tab| tab.key).collect();
        let active = |shell: &Shell| shell.host().registry().tabs_view().active;
        settle(&mut shell, loaded);

        shell.handle_key(key(KeyCode::Char(']')));
        assert_eq!(active(&shell), Some(keys[1]));
        assert!(
            settle_within(&mut shell, WAIT, loaded),
            "the second tab never loaded"
        );

        shell.handle_key(key(KeyCode::Char(']')));
        let deadline = Instant::now() + WAIT;
        while !text(&mut shell).contains("did not load") && Instant::now() < deadline {
            shell.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        let frame = text(&mut shell);
        assert!(frame.contains("This tab did not load"), "{frame}");

        shell.handle_key(key(KeyCode::Char('[')));
        assert_eq!(
            active(&shell),
            Some(keys[1]),
            "'[' stayed on the failed tab"
        );
        let frame = text(&mut shell);
        assert!(frame.contains("src/f000.rs"), "{frame}");

        // Shown again, the failed tab loads again: until that reply drains,
        // it says it is loading.
        shell.handle_key(key(KeyCode::Char(']')));
        let frame = text(&mut shell);
        assert!(frame.contains("Loading…"), "{frame}");
        let deadline = Instant::now() + WAIT;
        while !text(&mut shell).contains("did not load") && Instant::now() < deadline {
            shell.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        let frame = text(&mut shell);
        assert!(frame.contains("This tab did not load"), "{frame}");

        shell.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert!(shell.should_quit, "Ctrl+Q did not quit a failed tab");
    });
}

/// The service tests, as text.
const TEST_SOURCES: [(&str, &str); 13] = [
    ("config_hub_tests.rs", include_str!("config_hub_tests.rs")),
    ("filter_tests.rs", include_str!("filter_tests.rs")),
    ("github_tests.rs", include_str!("github_tests.rs")),
    ("loop_tests.rs", include_str!("loop_tests.rs")),
    ("modes_tests.rs", include_str!("modes_tests.rs")),
    ("review_hub_tests.rs", include_str!("review_hub_tests.rs")),
    ("reviewed_tests.rs", include_str!("reviewed_tests.rs")),
    ("settings_tests.rs", include_str!("settings_tests.rs")),
    (
        "thread_writes_tests.rs",
        include_str!("thread_writes_tests.rs"),
    ),
    ("threads_tests.rs", include_str!("threads_tests.rs")),
    ("watched_tests.rs", include_str!("watched_tests.rs")),
    ("worktree_tests.rs", include_str!("worktree_tests.rs")),
    ("tests.rs", include_str!("tests.rs")),
];

/// No service test hands the shell the real GitHub: the start's PR probe
/// would run `gh` in every fixture.
#[test]
fn service_tests_never_reach_the_real_github() {
    let needles = [
        concat!("Ports::cli()", ".github"),
        concat!("shell_on(", "Ports::cli()"),
    ];
    for (name, text) in TEST_SOURCES {
        let flat: String = text.split_whitespace().collect();
        for needle in needles {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
    }
}
