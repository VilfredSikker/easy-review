//! The flag-on GitHub writes (P68.1) against a scripted GitHub: the pull,
//! the approval, the GitHub composer and the PR-wide comment, and the start's
//! PR probe. The App runs beside the flag path where the two must match.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};
use er_engine::ai::ErGithubComments;
use er_engine::github_post::{NO_PR_FOR_COMMENT, RETRY_TOO_EARLY};
use er_engine::ports::Ports;
use er_engine::test_support::{read_sidecar, write_sidecar_json};
use er_engine::TabKey;
use er_github::{BranchPrProbe, CreatedComment, FakeGitHub, GhError, PrLookup, PrOverviewData};
use er_service::{EARLIER_VERSION_POSTED, OWN_PR_APPROVAL_REFUSED};

use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    assert_parity, fixture, git, key, loaded, path_of, plan, settle, settle_within, shell_frame,
    shell_on, SOURCES, WAIT,
};
use super::tui_view::{Ask, Mode, PrKnown, TabViewState};
use crate::ui::highlight::Highlighter;

/// A fake that finds acme/widgets#7 for the branch and can post a line
/// comment on it.
fn on_pr_7() -> FakeGitHub {
    FakeGitHub::default()
        .with_lookup_pr_for_current_branch(Ok(PrLookup::Present {
            owner: "acme".into(),
            repo: "widgets".into(),
            number: 7,
        }))
        .with_pr_head_oid(Ok("abc123".into()))
}

fn created(id: u64) -> CreatedComment {
    CreatedComment {
        id,
        created_at: "2026-01-01T00:00:00Z".into(),
        author: "you".into(),
    }
}

fn unknown_outcome() -> GhError {
    GhError::Network {
        message: "connection reset".into(),
    }
}

fn ports(github: &Arc<FakeGitHub>) -> Ports {
    let github: Arc<dyn er_github::GitHub> = github.clone();
    Ports {
        git: Ports::cli().git,
        github,
    }
}

/// The flag path on one working tab of `repo`, loaded.
fn working(repo: &tempfile::TempDir, github: &Arc<FakeGitHub>, config: &Path) -> Shell {
    let args = Args {
        paths: vec![path_of(repo)],
        ..Args::default()
    };
    let mut shell = shell_on(ports(github), &plan(&args), config);
    settle(&mut shell, loaded);
    shell
}

fn active_key(shell: &Shell) -> TabKey {
    let tabs = shell.host().registry().tabs_view();
    tabs.active.unwrap_or_else(|| panic!("no active tab"))
}

fn state(shell: &Shell) -> &TabViewState {
    let key = active_key(shell);
    shell
        .view()
        .tab(key)
        .unwrap_or_else(|| panic!("no view state"))
}

fn known(shell: &Shell) -> PrKnown {
    let content = shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"));
    shell.view().pr_known(active_key(shell), &content)
}

fn notice(shell: &Shell) -> String {
    shell.view().notification.clone().unwrap_or_default()
}

fn settle_replies(shell: &mut Shell) {
    settle(shell, |shell| shell.in_flight() == 0);
}

fn type_text(shell: &mut Shell, text: &str) {
    for c in text.chars() {
        shell.handle_key(key(KeyCode::Char(c)));
    }
}

fn calls(github: &FakeGitHub, name: &str) -> Vec<String> {
    let prefix = format!("{name}(");
    github
        .calls()
        .into_iter()
        .filter(|call| call.starts_with(&prefix))
        .collect()
}

/// Opens the Git hub and runs its row at `row`.
fn git_hub_row(shell: &mut Shell, row: usize) {
    shell.handle_key(key(KeyCode::Char('g')));
    for _ in 0..row {
        shell.handle_key(key(KeyCode::Down));
    }
    shell.handle_key(key(KeyCode::Enter));
}

const PULL_ROW: usize = 4;
const COMMENT_ROW: usize = 5;
const APPROVE_ROW: usize = 6;

/// `G` pulls the PR's comments through the actor, past the cached bundle,
/// once while a pull is in flight; the AI section moves and the counts show.
#[test]
fn a_pull_reads_github_and_moves_the_ai_section() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let listed: er_github::GitHubComment = serde_json::from_value(serde_json::json!({
        "id": 42, "body": "from github", "path": "src/f000.rs", "line": 4,
        "original_line": 4, "side": "RIGHT", "user": { "login": "reviewer" },
        "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
    }))
    .unwrap_or_else(|e| panic!("{e}"));
    let bundle = er_github::PrCommentBundle {
        comments: vec![listed],
        threads: std::collections::HashMap::new(),
    };
    let github = Arc::new(on_pr_7().with_pr_comment_bundle_cached(Ok(Arc::new(bundle))));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        let before = shell.active_content().map_or(0, |c| c.ai.revision);
        shell.handle_key(key(KeyCode::Char('G')));
        shell.handle_key(key(KeyCode::Char('G')));
        settle_replies(&mut shell);
        let content = shell.active_content().unwrap_or_else(|| panic!("gone"));
        assert!(content.ai.revision > before, "the AI section did not move");
        let rows = read_sidecar::<ErGithubComments>(Path::new(&content.context.comments_dir));
        assert_eq!(rows.comments.len(), 1, "{:?}", rows.comments);
        assert_eq!(notice(&shell), "GitHub sync: 1 from GitHub, 0 local kept");
        assert_eq!(known(&shell), PrKnown::Present);
        let read = calls(&github, "pr_comment_bundle_cached");
        assert_eq!(read.len(), 1, "a pull in flight was sent again");
        let order: Vec<String> = github
            .calls()
            .into_iter()
            .filter(|call| {
                call.starts_with("invalidate_comments") || call.starts_with("pr_comment")
            })
            .collect();
        assert!(
            order
                .first()
                .is_some_and(|call| call.starts_with("invalidate")),
            "the pull read the cached bundle: {order:?}"
        );
    });
}

/// A pull GitHub answers with "no PR" marks the tab, and the GitHub
/// composer then refuses on it.
#[test]
fn a_pull_with_no_pr_marks_the_tab_absent() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github =
        Arc::new(FakeGitHub::default().with_lookup_pr_for_current_branch(Ok(PrLookup::Absent)));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        git_hub_row(&mut shell, PULL_ROW);
        settle_replies(&mut shell);
        assert_eq!(notice(&shell), NO_PR_FOR_COMMENT);
        assert_eq!(known(&shell), PrKnown::Absent);
        shell.handle_key(key(KeyCode::Char('c')));
        assert_eq!(shell.view().input_mode, Mode::Normal);
    });
}

/// A local PR tab on #7 of acme/widgets, its refs fetched. Answers the repo
/// and its origin, which must outlive the shell.
fn pr_tab(script: FakeGitHub, config: &Path) -> ([tempfile::TempDir; 2], Shell, Arc<FakeGitHub>) {
    let repo = fixture(1);
    let origin = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    git(origin.path(), &["init", "-q", "--bare"]);
    git(repo.path(), &["remote", "add", "origin", &path_of(&origin)]);
    git(
        repo.path(),
        &["push", "-q", "origin", "main", "feature:refs/pull/7/head"],
    );
    git(repo.path(), &["fetch", "-q", "origin"]);
    let raw = "diff --git a/src/f000.rs b/src/f000.rs\n--- a/src/f000.rs\n+++ b/src/f000.rs\n@@ -1 +1 @@\n-a\n+b\n";
    let github = Arc::new(
        script
            .with_pr_diff(Ok(raw.into()))
            .with_pr_base_branch_and_commits(Ok(("main".into(), Vec::new())))
            .with_fetch_pr_base_ref(Ok("origin/main".into()))
            .with_get_repo_info(Ok(("acme".into(), "widgets".into()))),
    );
    let args = Args {
        paths: vec![path_of(&repo)],
        pr: Some(7),
        ..Args::default()
    };
    let mut shell = shell_on(ports(&github), &plan(&args), config);
    let fetched = |shell: &Shell| {
        shell
            .active_content()
            .is_some_and(|content| content.pr_refs_fetched)
    };
    assert!(settle_within(&mut shell, WAIT, fetched), "no PR refs");
    settle_replies(&mut shell);
    shell.tick(Instant::now() + Duration::from_secs(60));
    ([repo, origin], shell, github)
}

fn overview(author: &str) -> PrOverviewData {
    PrOverviewData {
        number: 7,
        title: "T".into(),
        body: String::new(),
        state: "OPEN".into(),
        author: author.into(),
        url: String::new(),
        base_branch: "main".into(),
        head_branch: "feature".into(),
        checks: Vec::new(),
        reviewers: Vec::new(),
    }
}

/// "Approve PR", then `y`: one Approve review goes to GitHub through the
/// actor, a second confirm while it is in flight sends nothing, and the
/// App's notice shows.
#[test]
fn approve_sends_one_approve_decision() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let script = FakeGitHub::default()
            .with_viewer_login(Ok("me".into()))
            .with_overview_no_checks(Ok(Some(overview("someone"))))
            .with_submit_review(Ok(()));
        let (_dirs, mut shell, github) = pr_tab(script, storage.path());
        for _ in 0..2 {
            git_hub_row(&mut shell, APPROVE_ROW);
            assert_eq!(shell.view().input_mode, Mode::Confirm(Ask::ApprovePr));
            shell.handle_key(key(KeyCode::Char('y')));
        }
        settle_replies(&mut shell);
        let submitted = calls(&github, "submit_review");
        assert_eq!(submitted.len(), 1, "{submitted:?}");
        assert!(submitted[0].contains("event=\"APPROVE\""), "{submitted:?}");
        assert_eq!(notice(&shell), "PR approved");
    });
}

/// Approving your own PR shows the service's refusal, once, and posts no
/// review.
#[test]
fn approving_your_own_pr_shows_the_refusal() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let script = FakeGitHub::default()
            .with_viewer_login(Ok("me".into()))
            .with_overview_no_checks(Ok(Some(overview("me"))))
            .with_submit_review(Ok(()));
        let (_dirs, mut shell, github) = pr_tab(script, storage.path());
        git_hub_row(&mut shell, APPROVE_ROW);
        shell.handle_key(key(KeyCode::Char('y')));
        settle_replies(&mut shell);
        assert_eq!(notice(&shell), OWN_PR_APPROVAL_REFUSED);
        assert_eq!(shell.settings().times_shown(OWN_PR_APPROVAL_REFUSED), 1);
        assert!(calls(&github, "submit_review").is_empty());
    });
}

/// Presses `event` on both paths and compares the frames once the flag
/// path's replies have landed.
fn step(app: &mut er_engine::app::App, shell: &mut Shell, hl: &mut Highlighter, event: KeyEvent) {
    app.clear_notification();
    shell.tick(Instant::now() + Duration::from_secs(60));
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut cache = crate::screen::ViewCache::default();
    crate::handle_key(app, event, &tx, &mut None, &mut cache).unwrap_or_else(|e| panic!("{e}"));
    shell.handle_key(event);
    settle_replies(shell);
    let frame = shell_frame(shell, hl);
    assert_parity(app, &frame, hl, &format!("{event:?}"));
}

fn app_on(path: &str, github: &Arc<FakeGitHub>) -> er_engine::app::App {
    let mut app = er_engine::app::App::new_with_args(&[path.to_string()], ports(github))
        .unwrap_or_else(|e| panic!("{e:#}"));
    app.config.display.auto_context_threshold = 0;
    app
}

/// On a tab the probe found PR-less, `c` and "Comment on PR" refuse with
/// the App's message and draw as the App's held state draws.
#[test]
fn a_tab_known_to_have_no_pr_refuses_the_github_composer() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github =
        Arc::new(FakeGitHub::default().with_probe_pr_for_current_branch(Ok(BranchPrProbe::Absent)));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo), &github);
        app.tab_mut().pr_absent = true;
        let mut shell = working(&flag_repo, &github, storage.path());
        settle(&mut shell, |shell| known(shell) == PrKnown::Absent);
        let mut hl = Highlighter::new();
        step(&mut app, &mut shell, &mut hl, key(KeyCode::Char('c')));
        assert_eq!(notice(&shell), NO_PR_FOR_COMMENT);
        let mut hub = vec![key(KeyCode::Char('g'))];
        hub.extend(vec![key(KeyCode::Down); COMMENT_ROW]);
        hub.push(key(KeyCode::Enter));
        for event in hub {
            step(&mut app, &mut shell, &mut hl, event);
        }
        assert_eq!(shell.view().input_mode, Mode::Normal);
        assert_eq!(notice(&shell), NO_PR_FOR_COMMENT);
    });
}

/// A probe that fails leaves the PR unknown, so `c` opens the composer; a
/// PR on another base says how to open it.
#[test]
fn the_probe_marks_only_a_no_pr_answer_absent() {
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let repo = fixture(1);
        let failing = Arc::new(
            FakeGitHub::default().with_probe_pr_for_current_branch(Err(unknown_outcome())),
        );
        let mut shell = working(&repo, &failing, storage.path());
        let probed = |_: &Shell| !calls(&failing, "probe_pr_for_current_branch").is_empty();
        settle(&mut shell, probed);
        settle_replies(&mut shell);
        assert_eq!(known(&shell), PrKnown::Unknown);
        shell.handle_key(key(KeyCode::Char('c')));
        assert_eq!(shell.view().input_mode, Mode::Comment);

        let repo = fixture(1);
        let present = Arc::new(FakeGitHub::default().with_probe_pr_for_current_branch(Ok(
            BranchPrProbe::Present {
                number: 7,
                base: "develop".into(),
            },
        )));
        let mut shell = working(&repo, &present, storage.path());
        settle(&mut shell, |shell| known(shell) == PrKnown::Present);
        assert_eq!(notice(&shell), "PR #7 targets develop — run: er --pr 7");
    });
}

/// A GitHub comment whose outcome is unknown keeps the composer and its
/// text; sent again, it carries the same client id, so the service waits
/// for GitHub instead of posting a second copy.
#[test]
fn a_refused_github_comment_keeps_its_text_and_its_client_id() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(on_pr_7().with_post_line_comment(Err(unknown_outcome())));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        for _ in 0..4 {
            shell.handle_key(key(KeyCode::Down));
        }
        shell.handle_key(key(KeyCode::Char('c')));
        type_text(&mut shell, "Look here");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        assert_eq!(shell.view().input_mode, Mode::Comment);
        assert_eq!(state(&shell).draft.textarea.lines().join("\n"), "Look here");
        let shown = notice(&shell);
        assert!(shown.contains("connection reset"), "{shown:?}");
        assert_eq!(shell.settings().times_shown(&shown), 1);

        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        assert!(
            notice(&shell).ends_with(RETRY_TOO_EARLY),
            "{}",
            notice(&shell)
        );
        let posts = calls(&github, "post_line_comment");
        assert_eq!(posts.len(), 1, "a second copy was posted: {posts:?}");
    });
}

/// "Comment on PR" posts PR-wide with a client id, so a resend after an
/// unknown outcome posts no second copy.
#[test]
fn comment_on_pr_posts_pr_wide_once() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(on_pr_7().with_post_general(Err(unknown_outcome())));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        git_hub_row(&mut shell, COMMENT_ROW);
        assert_eq!(shell.view().input_mode, Mode::Comment);
        type_text(&mut shell, "Ship it");
        for _ in 0..2 {
            shell.handle_key(key(KeyCode::Enter));
            settle_replies(&mut shell);
        }
        let general = calls(&github, "post_general");
        assert_eq!(general.len(), 1, "{:?}", github.calls());
        assert!(calls(&github, "post_line_comment").is_empty());
        assert!(
            notice(&shell).ends_with(RETRY_TOO_EARLY),
            "{}",
            notice(&shell)
        );
    });
}

/// A GitHub line comment on a deleted line posts and records as the App's
/// does, and the frames match.
#[test]
fn a_github_line_comment_posts_like_the_app() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let script = || Arc::new(on_pr_7().with_post_line_comment(Ok(created(901))));
    let (app_github, flag_github) = (script(), script());
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo), &app_github);
        let mut shell = working(&flag_repo, &flag_github, storage.path());
        let mut hl = Highlighter::new();
        let mut keys = vec![key(KeyCode::Down); 4];
        keys.push(key(KeyCode::Char('c')));
        keys.extend("Why gone?".chars().map(|c| key(KeyCode::Char(c))));
        keys.push(key(KeyCode::Enter));
        for event in keys {
            step(&mut app, &mut shell, &mut hl, event);
        }
        let rows = |dir: &str| {
            let rows = read_sidecar::<ErGithubComments>(Path::new(dir)).comments;
            serde_json::to_value(rows).unwrap_or_else(|e| panic!("{e}"))
        };
        let content = shell.active_content().unwrap_or_else(|| panic!("gone"));
        let flag_rows = rows(&content.context.comments_dir);
        assert_eq!(flag_rows, rows(&app.tab().github_comments_dir()));
        assert_eq!(flag_rows[0]["side"], "LEFT", "{flag_rows}");
        assert_eq!(calls(&flag_github, "post_line_comment").len(), 1);
    });
}

/// A reply drafted under a posted comment goes to its GitHub thread.
#[test]
fn a_reply_under_a_posted_comment_goes_to_its_thread() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let github = Arc::new(on_pr_7().with_post_reply(Ok(created(12))));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = working(&repo, &github, storage.path());
        let content = shell.active_content().unwrap_or_else(|| panic!("gone"));
        write_sidecar_json::<ErGithubComments>(
            Path::new(&content.context.comments_dir),
            serde_json::json!({
                "version": 1, "diff_hash": content.context.branch_diff_hash,
                "github": {"pr_number": 7, "owner": "acme", "repo": "widgets"},
                "comments": [{
                    "id": "gh-11", "timestamp": "2026-01-01T00:00:00Z",
                    "file": "src/f000.rs", "hunk_index": 0, "line_start": 4,
                    "line_content": "c", "comment": "posted", "author": "reviewer",
                    "source": "github", "github_id": 11, "side": "RIGHT"
                }]
            }),
        );
        let posted = |shell: &Shell| {
            shell
                .active_content()
                .is_some_and(|c| c.ai.state.find_comment("gh-11").is_some())
        };
        let deadline = Instant::now() + WAIT;
        let mut ahead = 0;
        while !posted(&shell) && Instant::now() < deadline {
            ahead += 2;
            shell.tick(Instant::now() + Duration::from_secs(ahead));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(posted(&shell), "the posted comment never loaded");
        let key_of = active_key(&shell);
        if let Some(state) = shell.view_mut().tab_mut_if_open(key_of) {
            state.draft.file = "src/f000.rs".into();
            state.draft.reply_to = Some("gh-11".into());
            state.draft.kind = er_engine::ai::CommentType::GitHubComment;
        }
        shell.view_mut().input_mode = Mode::Comment;
        type_text(&mut shell, "Agreed");
        shell.handle_key(key(KeyCode::Enter));
        settle_replies(&mut shell);
        let replies = calls(&github, "post_reply");
        assert_eq!(replies.len(), 1, "{:?}", github.calls());
        assert!(replies[0].contains("in_reply_to=11"), "{replies:?}");
        assert!(calls(&github, "post_line_comment").is_empty());
        assert_eq!(notice(&shell), "Comment posted to GitHub");
    });
}

/// A refusal that adopted an earlier post drops the draft's client id, so
/// the next send is a new comment; any other refusal keeps it.
#[test]
fn an_adopted_earlier_version_drops_the_client_id() {
    let mut state = TabViewState::default();
    for (message, kept) in [("GitHub is down", true), (EARLIER_VERSION_POSTED, false)] {
        state.draft.client_id = Some("c-1".into());
        state.sent = Some("text".into());
        super::github::refused(&mut state, message);
        assert!(state.sent.is_none());
        assert_eq!(state.draft.client_id.is_some(), kept, "{message}");
    }
}

/// Every GitHub write goes through the service: no flag-on module calls the
/// port's writes or the App's approve, and the PR probe is the one GitHub
/// read a module makes, in its own module, which calls nothing else.
#[test]
fn github_writes_reach_github_only_through_the_service() {
    let banned = [
        "gh_pr_approve",
        "gh_pending_review",
        ".approve(",
        ".delete_comment(",
        ".post_reply(",
        ".post_general(",
        ".post_line_comment(",
        ".submit_review(",
        "pending_review_submit",
    ];
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for word in banned {
            assert!(!flat.contains(word), "{name} names {word}");
        }
        let probes = flat.contains("probe_pr_for_current_branch");
        assert_eq!(probes, name == "pr_probe.rs", "{name} probes");
        if name == "pr_probe.rs" {
            for call in flat.split("github.").skip(1) {
                assert!(
                    call.starts_with("probe_pr_for_current_branch("),
                    "{name} calls github.{}",
                    call.chars().take(30).collect::<String>()
                );
            }
            for word in ["Command::", ".post(", "ServiceHost"] {
                assert!(!flat.contains(word), "{name} names {word}");
            }
        }
    }
}
