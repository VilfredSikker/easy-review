//! The flag-on clears (P72.3): `z` and `Z` ask and clear as on the App path;
//! `Z` follows the service's rule and also takes the professor and expert
//! results; a clear is refused while a review flow holds the bucket, holds
//! the bucket's admission while it runs, and says so when the bucket is busy.

use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::ai::{ErNotes, ErQuestions, ErReview};
use er_engine::test_support::write_sidecar_json;
use er_service::{Command, ReviewArtifact};

use super::clears::{self, Clear};
use super::findings_tests::{seed_both, seed_shell};
use super::shell::Shell;
use super::tests::{fixture, key, loaded, path_of, settle, shell, shell_frame, SOURCES, WAIT};
use super::threads_tests::{args, both_on, er_dir, settle_replies, step};
use super::tui_view::Ask;
use crate::ui::highlight::Highlighter;

const DRAFTS: [&str; 4] = [
    "questions.json",
    "notes.json",
    "questions.prev.json",
    "notes.prev.json",
];

/// A question and a note with their backups, and a review of one file.
fn seed_drafts(dir: &str, hash: &str) {
    let path = Path::new(dir);
    std::fs::create_dir_all(path).unwrap_or_else(|e| panic!("{e}"));
    let row = |id: &str| {
        serde_json::json!({
            "id": id, "file": "src/f000.rs", "hunk_index": 0, "line_start": 4, "text": id
        })
    };
    write_sidecar_json::<ErQuestions>(
        path,
        serde_json::json!({"version": 1, "diff_hash": hash, "questions": [row("q-1")]}),
    );
    write_sidecar_json::<ErNotes>(
        path,
        serde_json::json!({"version": 1, "diff_hash": hash, "notes": [row("n-1")]}),
    );
    for backup in ["questions.prev.json", "notes.prev.json"] {
        std::fs::write(path.join(backup), "{}").unwrap_or_else(|e| panic!("{e}"));
    }
    write_sidecar_json::<ErReview>(
        path,
        serde_json::json!({"version": 1, "diff_hash": hash, "files": {
            "src/f000.rs": {"risk": "high", "findings": []}
        }}),
    );
}

/// The review, the professor and expert results, a scoped overlay and the
/// triage, as raw files.
fn seed_reviews(dir: &str, _hash: &str) {
    let path = Path::new(dir);
    std::fs::create_dir_all(path.join("experts")).unwrap_or_else(|e| panic!("{e}"));
    std::fs::create_dir_all(path.join("scoped")).unwrap_or_else(|e| panic!("{e}"));
    for file in [
        "review.json",
        "professor.json",
        "triage.json",
        "experts/sec.json",
        "experts/sec.prev.json",
        "scoped/general.json",
    ] {
        std::fs::write(path.join(file), "{}").unwrap_or_else(|e| panic!("{e}"));
    }
}

fn present(dir: &str, files: &[&str]) -> Vec<String> {
    files
        .iter()
        .filter(|file| Path::new(dir).join(file).exists())
        .map(|file| (*file).to_string())
        .collect()
}

fn notice(shell: &Shell) -> Option<&str> {
    shell.view().notification.as_deref()
}

/// `z` and `Z`, cancelled and then confirmed, draw their prompts and
/// notices as the App's, and leave both buckets without the drafts or the
/// review.
#[test]
fn z_and_shift_z_ask_and_clear_like_the_app() {
    let _golden = super::parity::guard();
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_both(&mut app, &mut shell, seed_drafts);
        let mut hl = Highlighter::new();
        for code in ['z', 'n', 'z', 'y'] {
            step(&mut app, &mut shell, &mut hl, key(KeyCode::Char(code)));
        }
        assert_eq!(notice(&shell), Some("Questions & notes cleared"));
        for code in ['Z', 'n', 'Z', 'y'] {
            step(&mut app, &mut shell, &mut hl, key(KeyCode::Char(code)));
        }
        assert_eq!(notice(&shell), Some("Review cleared"));
        let mut files = DRAFTS.to_vec();
        files.push("review.json");
        for dir in [app.tab().er_dir(), er_dir(&shell)] {
            assert_eq!(present(&dir, &files), Vec::<String>::new(), "{dir}");
        }
    });
}

/// `Z` follows the service's review delete: the professor and expert
/// results and the scoped overlays go, and only the triage stays. The App's
/// `Z` keeps the professor and expert files; that difference is named for
/// P76.
#[test]
fn the_review_clear_takes_the_professor_and_expert_results() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let (mut app, mut shell) = both_on(&app_repo, &flag_repo, storage.path());
        seed_shell(&mut shell, seed_reviews);
        seed_reviews(&app.tab().er_dir(), "");
        app.tab_mut().reload_ai_state();
        for code in ['Z', 'y'] {
            shell.handle_key(key(KeyCode::Char(code)));
            super::tests::press_app(&mut app, KeyCode::Char(code));
        }
        settle_replies(&mut shell);
        let files = [
            "review.json",
            "professor.json",
            "experts/sec.json",
            "experts/sec.prev.json",
            "scoped",
            "triage.json",
        ];
        assert_eq!(present(&er_dir(&shell), &files), ["triage.json"]);
        assert_eq!(
            present(&app.tab().er_dir(), &files),
            [
                "professor.json",
                "experts/sec.json",
                "experts/sec.prev.json",
                "scoped",
                "triage.json"
            ]
        );
    });
}

pub(super) fn take_lease(dir: &str) -> er_store::LeaseGuard {
    let scope = er_store::Scope::dir(dir).unwrap_or_else(|e| panic!("{e}"));
    er_engine::store_registry::store()
        .lease(&scope, Duration::ZERO, "flow-1")
        .unwrap_or_else(|e| panic!("{e}"))
}

/// While a review flow holds the bucket's lease, each clear command is
/// refused and leaves its files, and `z` says why on the status line.
#[test]
fn a_clear_is_refused_while_a_flow_holds_the_bucket() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        seed_shell(&mut shell, seed_drafts);
        let dir = er_dir(&shell);
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let workspace = content.tab.identity().workspace_key();
        let lease = take_lease(&dir);
        let context = || content.context.clone();
        let commands = [
            Command::ClearQuestionsAndNotes { context: context() },
            Command::ClearQuestionAnswers { context: context() },
            Command::ClearNoteReplies { context: context() },
            Command::DeleteReviewArtifact {
                context: context(),
                artifact: ReviewArtifact::Review,
            },
        ];
        for command in commands {
            let sent = format!("{command:?}");
            let reply = shell.host().post(&workspace, command).recv_timeout(WAIT);
            let refused = reply.unwrap_or_else(|e| panic!("{e}"));
            assert!(refused.is_err(), "{sent} was not refused");
        }
        let mut files = DRAFTS.to_vec();
        files.push("review.json");
        assert_eq!(present(&dir, &files), files);

        shell.handle_key(key(KeyCode::Char('z')));
        shell.handle_key(key(KeyCode::Char('y')));
        settle_replies(&mut shell);
        assert_eq!(
            notice(&shell),
            Some("Questions & notes clear skipped: A review flow is already using this review bucket")
        );
        assert_eq!(present(&dir, &DRAFTS), DRAFTS);
        drop(lease);
    });
}

/// Presses `code`, then `y`, with the bucket's write lock held. While the
/// clear waits on the lock it holds the bucket's admission, so an admission
/// check made then is refused and names the clear; once the wait runs out
/// the clear is refused and the status line says `said`.
fn clear_while_busy(shell: &mut Shell, code: char, said: &str) {
    let dir = er_dir(shell);
    let scope = er_store::Scope::dir(&dir).unwrap_or_else(|e| panic!("{e}"));
    let lock = er_engine::store_registry::store()
        .lock_scope(&scope, Duration::ZERO)
        .unwrap_or_else(|e| panic!("{e}"));
    shell.handle_key(key(KeyCode::Char(code)));
    shell.handle_key(key(KeyCode::Char('y')));
    // The clear waits out the lock for seconds; the check only touches the
    // lease, for less time than the clear's own admission waits. Until the
    // clear has taken its lease, the check meets only this test's lock and
    // answers as for a review flow, so it polls on until the clear answers.
    const HELD: &str = "A clear is already using this review bucket";
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut refusal = None;
    while refusal.as_deref() != Some(HELD) && Instant::now() < deadline {
        refusal = er_engine::arena::check_bucket_admission(&dir)
            .err()
            .map(|error| error.to_string());
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        refusal.as_deref(),
        Some(HELD),
        "the {code} clear never held the bucket's admission"
    );
    settle_replies(shell);
    drop(lock);
    let shown = notice(shell).unwrap_or_default().to_string();
    assert!(shown.starts_with(said), "{shown}");
}

/// `z` and `Z` on a busy bucket each hold the bucket's admission through the
/// clear, are refused once the lock wait runs out, keep their files, and
/// say so.
#[test]
fn a_busy_bucket_refuses_the_clear_and_says_so() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        seed_shell(&mut shell, seed_drafts);
        let dir = er_dir(&shell);
        clear_while_busy(&mut shell, 'z', "Questions & notes clear skipped:");
        assert_eq!(present(&dir, &DRAFTS), DRAFTS);
        clear_while_busy(&mut shell, 'Z', "Review clear skipped:");
        assert_eq!(present(&dir, &["review.json"]), ["review.json"]);
    });
}

/// The keys open no clear prompt on a remote tab, as the App's; the hub
/// rows do, as the App's hub does.
#[test]
fn the_keys_open_no_clear_prompt_on_a_remote_tab() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let local = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let mut remote = (*local).clone();
        remote.tab.remote_repo = Some("acme/widgets".into());
        for clear in [Clear::QuestionsAndNotes, Clear::Reviews] {
            assert!(clears::ask(clear, &local, true).is_some());
            assert_eq!(clears::ask(clear, &remote, true), None);
            assert!(clears::ask(clear, &remote, false).is_some());
        }
        assert_eq!(
            clears::ask(Clear::Reviews, &local, true),
            Some(Ask::CleanupReviews { count: 0 })
        );
    });
}

/// The AI hub's two Cleanup rows open their own clears' prompts, with the
/// App's counts, and the frame draws the prompt.
#[test]
fn the_hub_cleanup_rows_ask_before_their_clears() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        seed_shell(&mut shell, seed_drafts);
        let mut hl = Highlighter::new();
        let drafts = shell
            .active_content()
            .map_or(0, |content| content.ai.state.local_draft_count());
        assert_eq!(drafts, 2);
        // The row before last is "Cleanup questions & notes".
        shell.handle_key(key(KeyCode::Char('a')));
        for _ in 0..30 {
            shell.handle_key(key(KeyCode::Char('j')));
        }
        shell.handle_key(key(KeyCode::Char('k')));
        shell.handle_key(key(KeyCode::Enter));
        let mode = shell.view().input_mode;
        let asked = Ask::CleanupQuestions { count: drafts };
        assert_eq!(mode, super::tui_view::Mode::Confirm(asked));
        shell.handle_key(key(KeyCode::Char('n')));
        // The last row is "Cleanup reviews".
        shell.handle_key(key(KeyCode::Char('a')));
        for _ in 0..30 {
            shell.handle_key(key(KeyCode::Char('j')));
        }
        shell.handle_key(key(KeyCode::Enter));
        let mode = shell.view().input_mode;
        assert_eq!(
            mode,
            super::tui_view::Mode::Confirm(Ask::CleanupReviews { count: 1 })
        );
        let frame = super::tests::rows(&shell_frame(&mut shell, &mut hl)).join("\n");
        assert!(frame.contains("Clear 1 review file(s)? (y/n)"), "{frame}");
        shell.handle_key(key(KeyCode::Char('y')));
        settle_replies(&mut shell);
        assert_eq!(
            present(&er_dir(&shell), &["review.json"]),
            Vec::<String>::new()
        );
    });
}

/// No flag-on module calls a clear helper or the admission check itself:
/// every clear goes to the service as a command.
#[test]
fn flag_on_modules_send_every_clear_to_the_service() {
    let banned = [
        "cleanup_",
        "review_cleanup",
        "annotation_cleanup",
        "bucket_admission",
    ];
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for needle in banned {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
    }
}
