//! The Open hub's folder, worktree and browser rows on the flag path
//! (P75.2): the hub draws as the App's once the service's worktree list
//! lands, a folder or worktree opens as a listed tab or selects the open
//! one, and the browser rows go through the GitHub port.

use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::ports::Ports;
use er_engine::tabs_record::TabKind;
use er_git::{FakeGit, Worktree};
use er_github::FakeGitHub;

use super::github_tests::{calls, notice, on_pr_7, overview, pr_tab, settle_replies, working};
use super::pr_stack_tests::{app_on, app_reads_stack, press_app, select_row, stack_json, step};
use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    fake_github, fixture, git, key, loaded, path_of, plan_on, rows, settle, settle_within,
    shell_frame, shell_on, SOURCES,
};
use super::tui_view::Overlay;
use crate::ui::highlight::Highlighter;

/// A second worktree of `repo` on branch `other`, beside it.
fn add_worktree(repo: &tempfile::TempDir) -> String {
    let path = format!("{}-other", path_of(repo));
    git(
        repo.path(),
        &["worktree", "add", "-q", &path, "-b", "other"],
    );
    path
}

fn known_worktrees(shell: &Shell) -> bool {
    shell.in_flight() == 0
}

fn frame_text(shell: &mut Shell) -> String {
    let mut hl = Highlighter::new();
    rows(&shell_frame(shell, &mut hl)).join("\n")
}

fn open_row(shell: &mut Shell, label: &str) {
    shell.handle_key(key(KeyCode::Char('o')));
    settle_replies(shell);
    select_row(None, shell, label);
    shell.handle_key(key(KeyCode::Enter));
}

/// No flag-on module lists worktrees or reads a directory itself: the
/// list comes from the service, and a folder from the engine's reader.
#[test]
fn open_modules_list_no_worktrees_and_read_no_directory() {
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for needle in ["list_worktrees", "read_dir("] {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
    }
}

/// The whole Open hub draws as the App's: "Switch worktree" live once the
/// list lands on a repo with a second worktree, and disabled on one with
/// none; the Stack rows as P73 drew them.
#[test]
fn the_open_hub_draws_like_the_app() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(FakeGitHub::default().with_stack_view_json(Ok(stack_json(false))));
    for second in [true, false] {
        let (app_repo, flag_repo) = (fixture(1), fixture(1));
        if second {
            add_worktree(&app_repo);
            add_worktree(&flag_repo);
        }
        er_engine::storage::with_process_test_storage_root(storage.path(), || {
            let mut app = app_on(&path_of(&app_repo), &github);
            let mut shell = working(&flag_repo, &github, storage.path());
            let mut hl = Highlighter::new();
            press_app(&mut app, key(KeyCode::Char('o')));
            app_reads_stack(&mut app, &github);
            app.clear_notification();
            shell.handle_key(key(KeyCode::Char('o')));
            settle(&mut shell, known_worktrees);
            let frame = shell_frame(&mut shell, &mut hl);
            super::tests::assert_parity(&app, &frame, &mut hl, &format!("second {second}"));
            let text = frame_text(&mut shell);
            let want = if second {
                "Jump to another worktree"
            } else {
                "No other worktrees"
            };
            assert!(text.contains("Switch worktree"), "{text}");
            select_row(Some(&mut app), &mut shell, "Switch worktree");
            let frame = shell_frame(&mut shell, &mut hl);
            super::tests::assert_parity(&app, &frame, &mut hl, &format!("{want} selected"));
            assert!(frame_text(&mut shell).contains(want));
        });
    }
}

const RAW: &str = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,2 @@\n a-one\n-a-old\n+a-new\n";

fn worktree(path: &str, branch: &str) -> Worktree {
    Worktree {
        path: path.into(),
        branch: branch.into(),
        head: "abc".into(),
    }
}

/// With git's worktree list held, `o` still draws the hub at once, its
/// "Switch worktree" row disabled; the row goes live once the list lands.
#[test]
fn the_worktree_list_never_holds_the_loop() {
    let repo = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let root = path_of(&repo);
    let (release, held) = mpsc::channel::<()>();
    let held = Mutex::new(held);
    let listed = vec![worktree(&root, "feature"), worktree("/elsewhere", "other")];
    let fake = FakeGit::default()
        .with_repo_root_in(Ok(root.clone()))
        .with_is_merge_in_progress(false)
        .with_current_branch(Ok("feature".into()))
        .with_detect_base_branch(Ok("main".into()))
        .with_diff_shortstat((0, 0, 0))
        .with_diff_raw(Ok(RAW.into()))
        .with_list_worktrees(Ok(listed))
        .with_list_worktrees_hook(move || {
            if let Ok(held) = held.lock() {
                let _ = held.recv_timeout(Duration::from_secs(5));
            }
        });
    let ports = Ports {
        git: Arc::new(fake),
        github: fake_github(),
    };
    let args = Args {
        paths: vec![root],
        ..Args::default()
    };
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let start = plan_on(&args, &ports);
        let mut shell = shell_on(ports, &start, storage.path());
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        let pressed = Instant::now();
        shell.handle_key(key(KeyCode::Char('o')));
        let text = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        let took = pressed.elapsed();
        assert!(took < Duration::from_secs(1), "the hub waited {took:?}");
        assert!(text.contains("No other worktrees"), "{text}");
        let _ = release.send(());
        let live = |shell: &Shell| {
            let Some(Overlay::Open(hub)) = shell.view().overlay.as_ref() else {
                return false;
            };
            hub.rows
                .iter()
                .any(|row| row.label == "Switch worktree" && row.enabled)
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !live(&shell) && Instant::now() < deadline {
            shell.tick(Instant::now());
            shell_frame(&mut shell, &mut hl);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(live(&shell), "the row never went live");
    });
}

fn tab_count(shell: &Shell) -> usize {
    shell.host().registry().tabs_view().tabs.len()
}

/// "Switch worktree" opens the picked worktree as a working tab that loads;
/// picking the tab's own worktree selects the tab already open.
#[test]
fn switch_worktree_opens_a_tab_or_selects_the_open_one() {
    let repo = fixture(1);
    let other = add_worktree(&repo);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(FakeGitHub::default());
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        open_row(&mut shell, "Switch worktree");
        assert!(matches!(shell.view().overlay, Some(Overlay::Worktrees(_))));
        shell.handle_key(key(KeyCode::Char('j')));
        shell.handle_key(key(KeyCode::Enter));
        let view = shell.host().registry().tabs_view();
        assert_eq!(view.tabs.len(), 2);
        let opened = view.active_entry().unwrap_or_else(|| panic!("no tab"));
        assert_eq!(opened.descriptor.kind, TabKind::Working);
        assert_eq!(opened.descriptor.repo_root, other);
        assert!(notice(&shell).starts_with("Opened: "), "{}", notice(&shell));
        let on_other = |shell: &Shell| {
            let content = shell.active_content();
            content.is_some_and(|content| content.tab.repo_root == other)
        };
        assert!(settle_within(&mut shell, super::tests::WAIT, on_other));

        open_row(&mut shell, "Switch worktree");
        shell.handle_key(key(KeyCode::Enter));
        assert_eq!(tab_count(&shell), 2, "the open worktree was listed again");
        assert!(
            notice(&shell).starts_with("Switched to tab: "),
            "{}",
            notice(&shell)
        );
        let active = shell
            .active_content()
            .map(|content| content.tab.repo_root.clone());
        assert_eq!(active, Some(path_of(&repo)));
    });
}

/// A parent holding the tab's repo, a plain folder with a repo inside, a
/// file and a hidden folder.
fn browse_fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let parent = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let repo = fixture(1);
    let moved = parent.path().join("alpha");
    std::fs::rename(repo.path(), &moved).unwrap_or_else(|e| panic!("{e}"));
    for dir in ["beta/inner", ".hidden"] {
        std::fs::create_dir_all(parent.path().join(dir)).unwrap_or_else(|e| panic!("{e}"));
    }
    git(
        &parent.path().join("beta/inner"),
        &["init", "-q", "-b", "main"],
    );
    std::fs::write(parent.path().join("notes.txt"), "x").unwrap_or_else(|e| panic!("{e}"));
    (parent, repo)
}

/// The browser draws as the App's on open, after Enter into a plain folder
/// and after Backspace; Enter on a repo opens it as a tab.
#[test]
fn browse_folders_draws_like_the_app_and_opens_a_repo() {
    let (parent, _moved) = browse_fixture();
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(FakeGitHub::default().with_stack_view_json(Ok(stack_json(false))));
    let root = std::fs::canonicalize(parent.path().join("alpha"))
        .unwrap_or_else(|e| panic!("{e}"))
        .to_string_lossy()
        .into_owned();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&root, &github);
        let args = Args {
            paths: vec![root.clone()],
            ..Args::default()
        };
        let ports = super::github_tests::ports(&github);
        let mut shell = shell_on(
            ports,
            &plan_on(&args, &super::github_tests::ports(&github)),
            storage.path(),
        );
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        press_app(&mut app, key(KeyCode::Char('o')));
        app_reads_stack(&mut app, &github);
        shell.handle_key(key(KeyCode::Char('o')));
        settle_replies(&mut shell);
        select_row(Some(&mut app), &mut shell, "Browse folders");
        for code in [
            KeyCode::Enter,
            KeyCode::Char('j'),
            KeyCode::Enter,
            KeyCode::Backspace,
        ] {
            step(&mut app, &mut shell, &mut hl, code);
        }
        let text = frame_text(&mut shell);
        assert!(text.contains("beta"), "{text}");
        assert!(!text.contains(".hidden"), "{text}");

        for code in [KeyCode::Char('j'), KeyCode::Enter, KeyCode::Enter] {
            shell.handle_key(key(code));
        }
        let view = shell.host().registry().tabs_view();
        assert_eq!(view.tabs.len(), 2);
        let opened = view.active_entry().unwrap_or_else(|| panic!("no tab"));
        let opened_root = Path::new(&opened.descriptor.repo_root);
        assert!(opened_root.ends_with("beta/inner"), "{opened_root:?}");
        assert_eq!(notice(&shell), "Opened: inner");
    });
}

/// "Open PR in browser" names the tab's PR and repo; `b` on a stack row
/// names the layer's PR and the repo of its URL.
#[test]
fn the_browser_rows_go_through_the_github_port() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let script = FakeGitHub::default().with_open_pr_in_browser(true);
        let (_repos, mut shell, github) = pr_tab(script, storage.path());
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        open_row(&mut shell, "Open PR in browser");
        let (root, slug) = (&content.tab.repo_root, content.tab.remote_repo.as_deref());
        let want = format!("open_pr_in_browser(dir={root:?}, number=7, repo_slug={slug:?})");
        assert_eq!(calls(&github, "open_pr_in_browser"), [want]);
        assert_eq!(notice(&shell), "Opening PR in browser...");
    });

    let repo = fixture(1);
    let github = Arc::new(
        FakeGitHub::default()
            .with_stack_view_json(Ok(stack_json(false)))
            .with_open_pr_in_browser(true),
    );
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        shell.handle_key(key(KeyCode::Char('o')));
        settle_replies(&mut shell);
        frame_text(&mut shell);
        select_row(None, &mut shell, "feature-2");
        shell.handle_key(key(KeyCode::Char('b')));
        let root = path_of(&repo);
        let want =
            format!("open_pr_in_browser(dir={root:?}, number=8, repo_slug=Some(\"acme/widgets\"))");
        assert_eq!(calls(&github, "open_pr_in_browser"), [want]);
        assert_eq!(notice(&shell), "Opening feature-2 (#8) in browser...");
    });

    // A remote PR tab names its repo.
    let raw = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    let github = Arc::new(
        FakeGitHub::default()
            .with_ensure_installed(Ok(()))
            .with_pr_diff_remote(Ok(raw.to_string()))
            .with_open_pr_in_browser(true),
    );
    let remote = Args {
        paths: vec!["https://github.com/acme/widgets/pull/9".into()],
        remote: true,
        ..Args::default()
    };
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let ports = super::github_tests::ports(&github);
        let start = plan_on(&remote, &ports);
        let mut shell = shell_on(ports, &start, storage.path());
        settle(&mut shell, loaded);
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        assert_eq!(content.tab.remote_repo.as_deref(), Some("acme/widgets"));
        open_row(&mut shell, "Open PR in browser");
        let root = &content.tab.repo_root;
        let want =
            format!("open_pr_in_browser(dir={root:?}, number=9, repo_slug=Some(\"acme/widgets\"))");
        assert_eq!(calls(&github, "open_pr_in_browser"), [want]);
    });
}

/// The hub's box, from the row holding `from` to its bottom border, matches
/// the App's in text and style, inside the box's columns only. Each frame's
/// own `from` row starts the match, as the rows above it may differ.
fn assert_box_from(app: &er_engine::app::App, shell: &mut Shell, from: &str) {
    let mut hl = Highlighter::new();
    let flag = shell_frame(shell, &mut hl);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30))
        .unwrap_or_else(|e| panic!("{e}"));
    let mut cache = crate::screen::ViewCache::default();
    terminal
        .draw(|f| crate::ui::draw(f, &crate::screen::from_app(app, &mut cache), &mut hl))
        .unwrap_or_else(|e| panic!("{e}"));
    let theirs = terminal.backend().buffer().clone();
    let (app_rows, flag_rows) = (rows(&theirs), rows(&flag));
    let shown = format!(
        "app:\n{}\nflag-on:\n{}",
        app_rows.join("\n"),
        flag_rows.join("\n")
    );
    let start_of = |rows: &[String]| rows.iter().position(|row| row.contains(from));
    let (Some(app_start), Some(start)) = (start_of(&app_rows), start_of(&flag_rows)) else {
        panic!("no {from}:\n{shown}");
    };
    let cols: Vec<char> = flag_rows[start].chars().collect();
    let left = cols.iter().position(|c| *c == '│').unwrap_or(0);
    let right = cols.iter().rposition(|c| *c == '│').unwrap_or(119);
    let bottom = (start..flag_rows.len()).find(|&row| flag_rows[row].contains('└'));
    let bottom = bottom.unwrap_or_else(|| panic!("no box bottom:\n{shown}"));
    for offset in 0..=bottom - start {
        for col in left..=right {
            let cell = |buffer: &ratatui::buffer::Buffer, row: usize| {
                let cell = &buffer.content[row * 120 + col];
                (cell.symbol().to_string(), cell.style())
            };
            let (a, b) = (
                cell(&theirs, app_start + offset),
                cell(&flag, start + offset),
            );
            assert_eq!(a, b, "row +{offset}, col {col}\n{shown}");
        }
    }
}

/// The Current PR section draws as the App's: on a `--pr` tab it shows
/// "Open PR in browser"; on a working tab whose probe found a PR, whose PR
/// section the service filled, it shows nothing, as the App's start leaves
/// that tab's PR number unset.
#[test]
fn the_current_pr_rows_draw_like_the_app() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let probe = er_github::BranchPrProbe::Present {
        number: 7,
        base: "main".into(),
    };
    let github = Arc::new(
        on_pr_7()
            .with_probe_pr_for_current_branch(Ok(probe))
            .with_overview(Ok(Some(overview("someone"))))
            .with_stack_view_json(Ok(stack_json(false))),
    );
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo), &github);
        // As the App's start applies the probe's overview.
        app.tab_mut().pr_data = Some(overview("someone"));
        let mut shell = working(&flag_repo, &github, storage.path());
        settle(&mut shell, |shell| {
            let content = shell.active_content();
            content.is_some_and(|content| content.pr.pr_number == Some(7))
        });
        let mut hl = Highlighter::new();
        press_app(&mut app, key(KeyCode::Char('o')));
        app_reads_stack(&mut app, &github);
        app.clear_notification();
        shell.handle_key(key(KeyCode::Char('o')));
        settle_replies(&mut shell);
        let frame = shell_frame(&mut shell, &mut hl);
        super::tests::assert_parity(&app, &frame, &mut hl, "probed working tab");
        assert!(!frame_text(&mut shell).contains("Current PR"));
    });

    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let script = FakeGitHub::default().with_stack_view_json(Ok(stack_json(false)));
        let (dirs, mut shell, github) = pr_tab(script, storage.path());
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let mut app = app_on(&path_of(&dirs[0]), &github);
        app.tab_mut().pr_number = Some(7);
        app.tab_mut()
            .remote_repo
            .clone_from(&content.tab.remote_repo);
        press_app(&mut app, key(KeyCode::Char('o')));
        app_reads_stack(&mut app, &github);
        shell.handle_key(key(KeyCode::Char('o')));
        settle_replies(&mut shell);
        frame_text(&mut shell);
        assert_box_from(&app, &mut shell, "── Current PR ──");
    });
}
