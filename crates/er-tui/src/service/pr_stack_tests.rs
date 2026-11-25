//! The PR panel and the stack on the flag path (P73), against a scripted
//! GitHub: the panel reads the PR section, the Open hub's Stack rows read the
//! section's stack and refresh through `RefreshStack`, and a PR opens as a
//! listed tab that loads. The App runs beside it where the two must match.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::tabs_record::TabKind;
use er_github::{BranchPrProbe, FakeGitHub, PrCommentBundle, PrOverviewData};
use ratatui::buffer::Buffer;

use super::github_tests::{
    calls, notice, on_pr_7, overview, pr_tab, settle_replies, type_text, working,
};
use super::parity::assert_frame;
use super::shell::Shell;
use super::tests::{fixture, key, rows, service_sources, settle, shell_frame};
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

/// Presses `code` and checks the frame against its golden once the replies
/// have landed.
pub(super) fn step(shell: &mut Shell, hl: &mut Highlighter, code: KeyCode) {
    shell.tick(Instant::now() + Duration::from_secs(60));
    shell.handle_key(key(code));
    settle_replies(shell);
    let frame = shell_frame(shell, hl);
    assert_frame(&format!("{code:?}"), &frame);
}

/// On a working tab whose branch has PR #7, the PR panel draws the PR
/// section the probe's number filled, as the App draws the probe's overview;
/// after `G` the section is read again, and the panel and the notice match.
#[test]
fn the_pr_panel_reads_the_pr_section_and_a_pull_refreshes_it() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(1);
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
        let mut shell = working(&flag_repo, &github, storage.path());
        let filled = |shell: &Shell| {
            let content = shell.active_content();
            content.is_some_and(|content| content.pr.overview.is_some())
        };
        settle(&mut shell, filled);
        let content = shell.active_content().unwrap_or_else(|| panic!("gone"));
        assert_eq!(content.tab.pr_number, None, "the tab became a PR tab");
        let mut hl = Highlighter::new();
        step(&mut shell, &mut hl, KeyCode::Char('p'));
        step(&mut shell, &mut hl, KeyCode::Char('p'));
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains("First title"), "{frame}");

        github.set_overview(Ok(Some(titled("Second title"))));
        step(&mut shell, &mut hl, KeyCode::Char('G'));
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains("Second title"), "{frame}");
        assert_eq!(
            notice(&shell),
            "GitHub sync: 0 from GitHub, 0 local kept, PR status refreshed"
        );
    });
}

/// `gh stack view` with two layers above `main`, or three once `top` is set.
pub(super) fn stack_json(top: bool) -> String {
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

/// The Stack section's rows, from its header through "Refresh stack",
/// against the golden.
fn assert_stack_parity(shell: &mut Shell, hl: &mut Highlighter, step: &str) {
    let flag = shell_frame(shell, hl);
    let flag_rows = rows(&flag);
    let at = |rows: &[String], text: &str, from: usize| {
        let found = rows.iter().skip(from).position(|row| row.contains(text));
        found.map(|at| at + from)
    };
    let shown = flag_rows.join("\n");
    let start = at(&flag_rows, "── Stack ──", 0).unwrap_or_else(|| panic!("{step}: {shown}"));
    let end = at(&flag_rows, "Refresh stack", start).unwrap_or_else(|| panic!("{step}: {shown}"));
    // The section alone, cut out of each frame, is what the golden holds.
    let section = |buffer: &Buffer| {
        let rows = u16::try_from(end + 1 - start).unwrap_or_else(|e| panic!("{e}"));
        let mut cut = Buffer::empty(ratatui::layout::Rect::new(0, 0, 120, rows));
        cut.content = buffer.content[start * 120..(end + 1) * 120].to_vec();
        cut
    };
    super::parity::assert_frame(step, &section(&flag));
}

pub(super) fn select_row(shell: &mut Shell, label: &str) {
    let Some(Overlay::Open(hub)) = shell.view_mut().overlay.as_mut() else {
        panic!("the Open hub is not up");
    };
    let at = hub.rows.iter().position(|row| row.label == label);
    let at = at.unwrap_or_else(|| panic!("no {label} row"));
    hub.selected = at;
}

/// The Stack rows read the PR section's stack: the hub opens on the loading
/// row and fills once `RefreshStack` lands; "Refresh stack" reads it again,
/// and the rows follow the section once its revision moves.
#[test]
fn the_stack_rows_follow_the_pr_section() {
    let _golden = super::parity::guard();
    let flag_repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(FakeGitHub::default().with_stack_view_json(Ok(stack_json(false))));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&flag_repo, &github, storage.path());
        let mut hl = Highlighter::new();
        shell.handle_key(key(KeyCode::Char('o')));
        settle_replies(&mut shell);
        assert_stack_parity(&mut shell, &mut hl, "two layers");
        let revision = shell.active_content().map(|content| content.pr.revision);

        github.set_stack_view_json(Ok(stack_json(true)));
        select_row(&mut shell, "Refresh stack");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let moved = shell.active_content().map(|content| content.pr.revision);
        assert_ne!(moved, revision, "the PR section did not move");
        assert_stack_parity(&mut shell, &mut hl, "three layers");
        let frame = rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains("feature-3"), "{frame}");
        let lookups = calls(&github, "stack_view_json");
        assert_eq!(lookups.len(), 2, "{lookups:?}");
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
    select_row(shell, label);
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
    for (name, text) in &service_sources() {
        let flat: String = text.split_whitespace().collect();
        for word in banned {
            assert!(!flat.contains(word), "{name} names {word}");
        }
    }
}

/// A `--pr` tab names its repo, so opening the same PR from the Open hub
/// focuses it: one tab, two actors never write the one PR bucket.
#[test]
fn opening_the_pr_a_pr_tab_holds_focuses_that_tab() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (_repo, mut shell, _github) = pr_tab(on_pr_7(), storage.path());
        let tabs = shell.host().registry().tabs_view();
        let local = tabs.active_entry().unwrap_or_else(|| panic!("no tab"));
        assert_eq!(local.descriptor.pr_owner.as_deref(), Some("acme"));
        let local = local.key;
        open_hub_row(&mut shell, "Open remote PR");
        type_text(&mut shell, "https://github.com/acme/widgets/pull/7");
        shell.handle_key(key(KeyCode::Enter));
        let tabs = shell.host().registry().tabs_view();
        assert_eq!(
            tabs.tabs.len(),
            1,
            "a second tab was listed for the open PR #7"
        );
        assert_eq!(tabs.active, Some(local));
        assert!(
            notice(&shell).starts_with("Switched to tab: "),
            "{}",
            notice(&shell)
        );
    });
}
