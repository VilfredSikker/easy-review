//! The PR panel and the stack on the flag path (P73), against a scripted
//! GitHub: the panel reads the PR section, the Open hub's Stack rows read the
//! section's stack and refresh through `RefreshStack`, and a PR opens as a
//! listed tab that loads. The App runs beside it where the two must match.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};
use er_engine::app::{App, OverlayData};
use er_engine::tabs_record::TabKind;
use er_github::{BranchPrProbe, FakeGitHub, PrCommentBundle, PrOverviewData};
use ratatui::buffer::Buffer;

use super::github_tests::{
    calls, notice, on_pr_7, overview, ports, settle_replies, type_text, working,
};
use super::shell::Shell;
use super::tests::{assert_parity, fixture, key, path_of, rows, settle, shell_frame, SOURCES};
use super::tui_view::{Mode, Overlay};
use crate::ui::highlight::Highlighter;

fn titled(title: &str) -> PrOverviewData {
    PrOverviewData {
        title: title.into(),
        ..overview("someone")
    }
}

fn no_comments() -> PrCommentBundle {
    PrCommentBundle {
        comments: Vec::new(),
        threads: std::collections::HashMap::new(),
    }
}

fn app_on(path: &str, github: &Arc<FakeGitHub>) -> App {
    let mut app =
        App::new_with_args(&[path.to_string()], ports(github)).unwrap_or_else(|e| panic!("{e:#}"));
    app.config.display.auto_context_threshold = 0;
    app
}

fn press_app(app: &mut App, event: KeyEvent) {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut cache = crate::screen::ViewCache::default();
    crate::handle_key(app, event, &tx, &mut None, &mut cache).unwrap_or_else(|e| panic!("{e}"));
}

/// Presses `code` on both paths and compares the frames once the flag
/// path's replies have landed.
fn step(app: &mut App, shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    app.clear_notification();
    shell.tick(Instant::now() + Duration::from_secs(60));
    press_app(app, key(code));
    shell.handle_key(key(code));
    settle_replies(shell);
    let frame = shell_frame(shell, hl);
    assert_parity(app, &frame, hl, &format!("{code:?}"));
}

/// On a working tab whose branch has PR #7, the PR panel draws the PR
/// section the probe's number filled, as the App draws the probe's overview;
/// after `G` the section is read again, and the panel and the notice match.
#[test]
fn the_pr_panel_reads_the_pr_section_and_a_pull_refreshes_it() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let probe = BranchPrProbe::Present {
        number: 7,
        base: "main".into(),
    };
    let github = Arc::new(
        on_pr_7()
            .with_probe_pr_for_current_branch(Ok(probe))
            .with_overview(Ok(Some(titled("First title"))))
            .with_fetch_pr_comment_bundle(Ok(no_comments()))
            .with_pr_comment_bundle_cached(Ok(Arc::new(no_comments()))),
    );
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo), &github);
        // As the App's start applies the probe's overview.
        app.tab_mut().pr_data = Some(titled("First title"));
        let mut shell = working(&flag_repo, &github, storage.path());
        let filled = |shell: &Shell| {
            let content = shell.active_content();
            content.is_some_and(|content| content.pr.overview.is_some())
        };
        settle(&mut shell, filled);
        let content = shell.active_content().unwrap_or_else(|| panic!("gone"));
        assert_eq!(content.tab.pr_number, None, "the tab became a PR tab");
        let mut hl = Highlighter::new();
        step(&mut app, &mut shell, &mut hl, KeyCode::Char('p'));
        step(&mut app, &mut shell, &mut hl, KeyCode::Char('p'));
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains("First title"), "{frame}");

        github.set_overview(Ok(Some(titled("Second title"))));
        step(&mut app, &mut shell, &mut hl, KeyCode::Char('G'));
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains("Second title"), "{frame}");
        assert_eq!(
            notice(&shell),
            "GitHub sync: 0 from GitHub, 0 local kept, PR status refreshed"
        );
    });
}

/// `gh stack view` with two layers above `main`, or three once `top` is set.
fn stack_json(top: bool) -> String {
    let layer = |name: &str, number: u64| {
        let url = format!("https://github.com/acme/widgets/pull/{number}");
        serde_json::json!({ "name": name, "pr": { "number": number, "url": url, "state": "OPEN" } })
    };
    let mut branches = vec![layer("feature", 7), layer("feature-2", 8)];
    if top {
        branches.push(layer("feature-3", 9));
    }
    serde_json::json!({ "trunk": "main", "currentBranch": "feature", "branches": branches })
        .to_string()
}

/// The App's stack lookup, which its loop runs on a thread with the CLI
/// handle, made here through the scripted one.
fn app_reads_stack(app: &mut App, github: &FakeGitHub) {
    if let Some((tab, source, seq)) = app.take_stack_load_request() {
        let info = er_engine::gh_stack::load_from_with(github, &source);
        app.apply_stack_result(tab, seq, info);
    }
}

/// The Stack section's rows, from its header through "Refresh stack", match
/// in text and style. The rest of the Open hub joins render parity in P75.
fn assert_stack_parity(app: &App, shell: &mut Shell, hl: &mut Highlighter, step: &str) {
    let flag = shell_frame(shell, hl);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30))
        .unwrap_or_else(|e| panic!("{e}"));
    let mut cache = crate::screen::ViewCache::default();
    terminal
        .draw(|f| crate::ui::draw(f, &crate::screen::from_app(app, &mut cache), hl))
        .unwrap_or_else(|e| panic!("{e}"));
    let app_frame: Buffer = terminal.backend().buffer().clone();
    let (app_rows, flag_rows) = (rows(&app_frame), rows(&flag));
    let at = |rows: &[String], text: &str, from: usize| {
        let found = rows.iter().skip(from).position(|row| row.contains(text));
        found.map(|at| at + from)
    };
    let shown = flag_rows.join("\n");
    let start = at(&flag_rows, "── Stack ──", 0).unwrap_or_else(|| panic!("{step}: {shown}"));
    let end = at(&flag_rows, "Refresh stack", start).unwrap_or_else(|| panic!("{step}: {shown}"));
    for row in start..=end {
        assert_eq!(app_rows[row], flag_rows[row], "{step}: row {row}\n{shown}");
        for col in 0..120 {
            let style = |buffer: &Buffer| buffer.content[row * 120 + col].style();
            assert_eq!(style(&app_frame), style(&flag), "{step}: style {row},{col}");
        }
    }
}

fn select_row(app: Option<&mut App>, shell: &mut Shell, label: &str) {
    let Some(Overlay::Open(hub)) = shell.view_mut().overlay.as_mut() else {
        panic!("the Open hub is not up");
    };
    let at = hub.rows.iter().position(|row| row.label == label);
    let at = at.unwrap_or_else(|| panic!("no {label} row"));
    hub.selected = at;
    if let Some(Some(OverlayData::ModalHub { selected, .. })) = app.map(|app| &mut app.overlay) {
        *selected = at;
    }
}

/// The Stack rows read the PR section's stack: the hub opens on the loading
/// row and fills once `RefreshStack` lands; "Refresh stack" reads it again,
/// and the rows follow the section once its revision moves.
#[test]
fn the_stack_rows_follow_the_pr_section() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(FakeGitHub::default().with_stack_view_json(Ok(stack_json(false))));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo), &github);
        let mut shell = working(&flag_repo, &github, storage.path());
        let mut hl = Highlighter::new();
        press_app(&mut app, key(KeyCode::Char('o')));
        app_reads_stack(&mut app, &github);
        shell.handle_key(key(KeyCode::Char('o')));
        settle_replies(&mut shell);
        assert_stack_parity(&app, &mut shell, &mut hl, "two layers");
        let revision = shell.active_content().map(|content| content.pr.revision);

        github.set_stack_view_json(Ok(stack_json(true)));
        select_row(Some(&mut app), &mut shell, "Refresh stack");
        press_app(&mut app, key(KeyCode::Enter));
        app_reads_stack(&mut app, &github);
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let moved = shell.active_content().map(|content| content.pr.revision);
        assert_ne!(moved, revision, "the PR section did not move");
        assert_stack_parity(&app, &mut shell, &mut hl, "three layers");
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains("feature-3"), "{frame}");
        let lookups = calls(&github, "stack_view_json");
        assert_eq!(lookups.len(), 4, "{lookups:?}");
    });
}

/// Whether the remote tab for `number` asked GitHub for anything, which
/// only its load does.
fn remote_load_ran(github: &FakeGitHub, number: u64) -> bool {
    let (pr, num) = (format!("pr={number}"), format!("number={number}"));
    github.calls().iter().any(|call| {
        call.contains("repo=\"widgets\"") && (call.contains(&pr) || call.contains(&num))
    })
}

fn open_hub_row(shell: &mut Shell, label: &str) {
    shell.handle_key(key(KeyCode::Char('o')));
    settle_replies(shell);
    select_row(None, shell, label);
    shell.handle_key(key(KeyCode::Enter));
}

/// "Open remote PR" and a stack row open a PR as a tab the registry lists
/// and the service loads; a bad URL says why and opens nothing, and a PR
/// already open is selected, not listed again.
#[test]
fn a_pr_opens_as_a_listed_tab_that_loads_and_only_once() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(FakeGitHub::default().with_stack_view_json(Ok(stack_json(false))));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        let tabs = |shell: &Shell| shell.host().registry().tabs_view();

        open_hub_row(&mut shell, "Open remote PR");
        assert_eq!(shell.view().input_mode, Mode::RemoteUrl);
        type_text(&mut shell, "not a url");
        shell.handle_key(key(KeyCode::Enter));
        assert_eq!(notice(&shell), "Failed: Invalid GitHub PR URL");
        assert_eq!(tabs(&shell).tabs.len(), 1);

        open_hub_row(&mut shell, "Open remote PR");
        type_text(&mut shell, "https://github.com/acme/widgets/pull/8");
        shell.handle_key(key(KeyCode::Enter));
        let view = tabs(&shell);
        assert_eq!(view.tabs.len(), 2);
        let opened = view
            .active_entry()
            .unwrap_or_else(|| panic!("no active tab"));
        assert_eq!(opened.descriptor.kind, TabKind::RemotePr);
        assert_eq!(opened.descriptor.pr_number, Some(8));
        let opened = opened.key;
        assert_eq!(notice(&shell), "Opened remote: widgets#8");
        settle(&mut shell, |_| remote_load_ran(&github, 8));

        shell.handle_key(key(KeyCode::Char('[')));
        open_hub_row(&mut shell, "feature-2");
        let view = tabs(&shell);
        assert_eq!(view.tabs.len(), 2, "the open PR was listed again");
        assert_eq!(view.active, Some(opened));
        assert_eq!(notice(&shell), "Switched to tab: widgets#8");
    });
}

/// The stack and the PR overview are read on the service only: no flag-on
/// module looks a stack up, reads an overview, or opens a PR the App's way.
/// A lookup on a TUI thread also fails the thread ban in `tests`.
#[test]
fn the_stack_and_the_overview_are_read_only_on_the_service() {
    let banned = [
        "gh_stack::load_from",
        "load_from_with",
        "stack_view_json",
        "open_pr_refs_json",
        "take_stack_load_request",
        "apply_stack_result",
        "open_remote_url",
        "overview_remote",
        "gh_pr_overview",
        ".overview(",
    ];
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for word in banned {
            assert!(!flat.contains(word), "{name} names {word}");
        }
    }
}
