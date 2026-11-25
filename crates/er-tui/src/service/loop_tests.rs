//! The flag-on loop's duties: what wakes it, what it reads again each
//! second, the session it saves and restores, and that no key waits while an
//! actor's effect is held.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::ports::Ports;
use er_engine::session_store::{self, SessionState};
use er_engine::TabKey;
use er_git::{BlameRequest, CliGit, CommitInfo, Git, GitError, LineAttribution, Worktree};
use er_service::{Command, TabContent};

use super::shell::Shell;
use super::startup::Args;
use super::tests::{
    fixture, git, key, loaded, path_of, plan, rows, settle, settle_within, shell, shell_frame,
    shell_on, WAIT,
};
use crate::ui::highlight::Highlighter;

/// How long one key may take to handle, tick and draw.
const FRAME_BOUND: Duration = Duration::from_millis(500);

fn storage() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"))
}

fn args(paths: Vec<String>) -> Args {
    Args {
        paths,
        ..Args::default()
    }
}

fn content(shell: &Shell) -> Arc<TabContent> {
    shell
        .active_content()
        .unwrap_or_else(|| panic!("no content"))
}

/// The active tab's file, hunk and draft client id, as the view holds them.
fn position(shell: &Shell) -> (usize, usize, Option<String>) {
    let key = shell
        .host()
        .registry()
        .tabs_view()
        .active
        .unwrap_or_else(|| panic!("no active tab"));
    let state = shell
        .view()
        .tab(key)
        .unwrap_or_else(|| panic!("no view state"));
    (
        state.selected_file,
        state.current_hunk,
        state.draft.client_id.clone(),
    )
}

fn saved_session(content: &TabContent) -> Option<SessionState> {
    let scope = er_store::Scope::dir(&content.context.er_dir).ok()?;
    session_store::load_session(&er_engine::store_registry::store(), &scope)
}

fn quit(shell: &mut Shell) {
    shell.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert!(shell.should_quit);
}

/// A mark made outside the loop publishes a view; that event alone asks for
/// the draw, with no key, no reply and no timer, and the frame shows it.
#[test]
fn an_actor_event_wakes_the_loop_for_a_redraw() {
    let repo = fixture(2);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        // The clock stops here, so no timer in the loop can come due.
        let frozen = Instant::now();
        let quiet_for = |shell: &mut Shell, wait: Duration| {
            let until = Instant::now() + wait;
            while Instant::now() < until {
                shell.tick(frozen);
                std::thread::sleep(Duration::from_millis(10));
            }
            shell.take_redraw()
        };
        quiet_for(&mut shell, Duration::from_millis(300));
        let before = shell_frame(&mut shell, &mut hl);
        assert!(
            !quiet_for(&mut shell, Duration::from_millis(200)),
            "the loop asked for a draw with nothing new"
        );

        let loaded = content(&shell);
        let toggle_path = loaded.diff.files[1].path.clone();
        let toggle = Command::ToggleReviewed {
            context: loaded.context.clone(),
            path: toggle_path.clone(),
        };
        drop(shell.host().post(&loaded.context.workspace_key(), toggle));
        // A frame is drawn only when the loop asks for one, as `run_loop` does.
        let deadline = Instant::now() + WAIT;
        let mut shown = before.clone();
        while shown == before && Instant::now() < deadline {
            shell.tick(frozen);
            if shell.take_redraw() {
                shown = shell_frame(&mut shell, &mut hl);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let marked = content(&shell).reviewed.marks.contains_key(&toggle_path);
        assert!(marked, "the mark never published");
        assert!(before != shown, "the published mark was never drawn");
    });
}

/// Each second the loop has every loaded tab read its sidecars again, so a
/// question written outside the process reaches the AI section.
#[test]
fn the_reload_tick_takes_an_outside_write() {
    let repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let before = content(&shell);
        let er_dir = &before.context.er_dir;
        std::fs::create_dir_all(er_dir).unwrap_or_else(|e| panic!("{e}"));
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErQuestions>(
            Path::new(er_dir),
            serde_json::json!({
                "version": 1,
                "diff_hash": before.context.branch_diff_hash,
                "questions": [{
                    "id": "q1", "file": "src/f000.rs", "hunk_index": 0, "line_start": 1,
                    "line_content": "c", "text": "outside", "resolved": false
                }]
            }),
        );
        let moved = |shell: &Shell| {
            shell
                .active_content()
                .is_some_and(|content| content.ai.revision > before.ai.revision)
        };
        assert!(
            settle_within(&mut shell, Duration::from_secs(5), moved),
            "the outside write never reached the AI section"
        );
        assert!(content(&shell).ai.state.questions.is_some());
    });
}

/// The session is saved from the view on quit, beside the diff it was taken
/// on; a new start on that diff restores the position, and a start on a
/// changed diff does not.
#[test]
fn a_session_restores_on_the_same_diff_only() {
    let repo = fixture(3);
    let storage = storage();
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut first = shell(&args(vec![path.clone()]), storage.path());
        settle(&mut first, loaded);
        for code in [KeyCode::Char('k'), KeyCode::Char('k'), KeyCode::Char('n')] {
            first.handle_key(key(code));
        }
        assert_eq!(position(&first).0, 2);
        quit(&mut first);
        let loaded = content(&first);
        let saved = saved_session(&loaded).unwrap_or_else(|| panic!("no session saved"));
        assert_eq!(saved.diff_hash, loaded.diff.meta.branch_diff_hash);
        assert_eq!((saved.selected_file, saved.current_hunk), (2, 1));

        let mut again = shell(&args(vec![path.clone()]), storage.path());
        settle(&mut again, loaded_with_view);
        assert_eq!(position(&again).0, 2, "the same diff did not restore");
        assert_eq!(position(&again).1, 1);

        std::fs::write(repo.path().join("src/f000.rs"), "let moved = 1;\n")
            .unwrap_or_else(|e| panic!("{e}"));
        git(repo.path(), &["commit", "-q", "-am", "move"]);
        let mut changed = shell(&args(vec![path]), storage.path());
        settle(&mut changed, loaded_with_view);
        assert_ne!(
            content(&changed).diff.meta.branch_diff_hash,
            saved.diff_hash
        );
        assert_eq!(
            position(&changed).0,
            0,
            "a session restored on a changed diff"
        );
    });
}

fn loaded_with_view(shell: &Shell) -> bool {
    let active = shell.host().registry().tabs_view().active;
    loaded(shell) && active.is_some_and(|key| shell.view().tab(key).is_some())
}

/// A saved draft comes back whole, its client id included, and is saved
/// again with it, so a retried post finds the comment it made (ADR 0053).
#[test]
fn a_restored_draft_keeps_its_client_id() {
    let repo = fixture(1);
    let storage = storage();
    let path = path_of(&repo);
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut first = shell(&args(vec![path.clone()]), storage.path());
        settle(&mut first, loaded);
        let loaded = content(&first);
        let mut session = super::session::capture(
            &loaded,
            first
                .view()
                .tab(loaded.key)
                .unwrap_or_else(|| panic!("no view")),
        );
        session.comment_draft = "first line\nsecond line".into();
        session.comment_draft_file = "src/f000.rs".into();
        session.comment_draft_line = Some(4);
        session.comment_draft_side = Some("RIGHT".into());
        session.comment_draft_line_end = Some(6);
        session.comment_draft_reply_to = Some("thread-1".into());
        session.comment_draft_client_id = Some("client-1".into());
        let scope = er_store::Scope::dir(&loaded.context.er_dir).unwrap_or_else(|e| panic!("{e}"));
        session_store::save_session(&er_engine::store_registry::store(), &scope, &session)
            .unwrap_or_else(|e| panic!("{e}"));

        let mut again = shell(&args(vec![path]), storage.path());
        settle(&mut again, loaded_with_view);
        assert_eq!(
            position(&again).2.as_deref(),
            Some("client-1"),
            "the restored draft lost its client id"
        );
        quit(&mut again);
        let resaved = saved_session(&loaded).unwrap_or_else(|| panic!("no session saved"));
        assert_eq!(resaved.comment_draft, session.comment_draft);
        assert_eq!(resaved.comment_draft_line, Some(4));
        assert_eq!(resaved.comment_draft_side.as_deref(), Some("RIGHT"));
        assert_eq!(resaved.comment_draft_line_end, Some(6));
        assert_eq!(resaved.comment_draft_reply_to.as_deref(), Some("thread-1"));
        assert_eq!(resaved.comment_draft_client_id.as_deref(), Some("client-1"));
    });
}

/// Holds `diff_raw` and `current_branch` while closed, so a tab load's
/// effect stays in flight until the test opens it.
#[derive(Default)]
pub(super) struct Gate {
    closed: Mutex<bool>,
    opened: Condvar,
    /// Makes `diff_raw` panic, as a dying effect worker does.
    pub(super) panics: std::sync::atomic::AtomicBool,
}

impl Gate {
    pub(super) fn set(&self, closed: bool) {
        *self.closed.lock().unwrap_or_else(|e| e.into_inner()) = closed;
        self.opened.notify_all();
    }

    fn pass(&self) {
        let closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .opened
            .wait_timeout_while(closed, Duration::from_secs(60), |closed| *closed);
    }
}

/// The real git, behind the gate.
pub(super) struct GatedGit(pub(super) Arc<Gate>);

macro_rules! pass_through {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $(fn $name(&self, $($arg: $ty),*) -> $ret {
            CliGit.$name($($arg),*)
        })*
    };
}

impl Git for GatedGit {
    fn diff_raw(
        &self,
        mode: &str,
        base: &str,
        repo_root: &str,
        head_ref: Option<&str>,
    ) -> Result<String, GitError> {
        self.0.pass();
        assert!(
            !self.0.panics.load(std::sync::atomic::Ordering::SeqCst),
            "the gate panics"
        );
        CliGit.diff_raw(mode, base, repo_root, head_ref)
    }

    fn current_branch(&self, repo_root: &str) -> Result<String, GitError> {
        self.0.pass();
        CliGit.current_branch(repo_root)
    }

    pass_through! {
        repo_root_cwd() -> Result<String, GitError>;
        repo_root_in(dir: &str) -> Result<String, GitError>;
        repo_root_at(path: &str) -> Result<String, GitError>;
        detect_base_branch(repo_root: &str) -> Result<String, GitError>;
        diff_raw_file(mode: &str, base: &str, repo_root: &str, path: &str, context_lines: Option<usize>, head_ref: Option<&str>) -> Result<String, GitError>;
        diff_raw_range(from: &str, to: &str, repo_root: &str) -> Result<String, GitError>;
        diff_checkout_against_base(root: &str, base: &str) -> Result<String, GitError>;
        diff_against_branch(root: &str, base: &str, branch: &str) -> Result<String, GitError>;
        diff_commit(hash: &str, repo_root: &str) -> Result<String, GitError>;
        diff_conflicts(repo_root: &str) -> Result<String, GitError>;
        unmerged_files(repo_root: &str) -> Result<Vec<String>, GitError>;
        is_merge_in_progress(repo_root: &str) -> bool;
        diff_shortstat(repo_root: &str, staged: bool) -> (usize, usize, usize);
        stage_file(repo_root: &str, file_path: &str) -> Result<(), GitError>;
        unstage_file(repo_root: &str, file_path: &str) -> Result<(), GitError>;
        stage_all(repo_root: &str) -> Result<(), GitError>;
        commit(repo_root: &str, message: &str) -> Result<(), GitError>;
        push(repo_root: &str) -> Result<String, GitError>;
        log_range(from: &str, to: &str, repo_root: &str, limit: usize, skip: usize) -> Result<Vec<CommitInfo>, GitError>;
        log_head(repo_root: &str, limit: usize) -> Result<Vec<CommitInfo>, GitError>;
        commit_shortstats(repo_root: &str, shas: &[&str]) -> HashMap<String, (usize, usize, usize)>;
        unpushed_shas(repo_root: &str, head: &str, pushed_refs: &[&str], limit: usize) -> HashSet<String>;
        list_worktrees(repo_root: &str) -> Result<Vec<Worktree>, GitError>;
        blob_oid(root: &str, spec: &str) -> Result<String, GitError>;
        read_blob(root: &str, spec: &str, max_bytes: usize) -> Result<Vec<u8>, GitError>;
        paths_present_at(root: &str, rev: &str, paths: &[String]) -> Result<Vec<String>, GitError>;
        gitignored_paths(repo_root: &str, paths: &[String]) -> HashSet<String>;
        diff_watched_file_snapshot(repo_root: &str, rel_path: &str, snapshots_dir: &str) -> Result<Option<String>, GitError>;
        blame_range(request: &BlameRequest<'_>) -> Result<LineAttribution, GitError>;
        merge_base(repo_root: &str, a: &str, b: &str) -> Result<String, GitError>;
        rev_parse_commit(repo_root: &str, rev: &str) -> Result<String, GitError>;
        remote_url(root: &str, name: &str) -> Result<String, GitError>;
        rev_parse_verify(root: &str, rev: &str) -> Result<Option<String>, GitError>;
        fetch(root: &str, args: &[&str]) -> Result<(), GitError>;
        update_ref(root: &str, name: &str, target: &str) -> Result<(), GitError>;
        diff_revs(root: &str, revs: &[&str]) -> Result<String, GitError>;
        shallow_clone(url: &str, dest: &str) -> Result<(), GitError>;
        show_ref_verify(root: &str, refname: &str) -> Result<bool, GitError>;
        current_branch_show(root: &str) -> Result<String, GitError>;
        for_each_ref(root: &str, format: &str, sort: Option<&str>, pattern: &str) -> Result<String, GitError>;
        ls_remote(root: &str, remote: &str, refname: &str) -> Result<String, GitError>;
        config_get_regexp(root: &str, pattern: &str) -> Result<String, GitError>;
        rev_list_count(root: &str, range: &str) -> Result<String, GitError>;
        branch_merged(root: &str, base: &str) -> Result<String, GitError>;
    }
}

/// Handles a key, ticks and draws one frame; answers how long that took.
fn frame_after(shell: &mut Shell, hl: &mut Highlighter, event: KeyEvent) -> Duration {
    let started = Instant::now();
    shell.handle_key(event);
    shell.tick(Instant::now());
    shell_frame(shell, hl);
    started.elapsed()
}

/// While a tab's load is held on the git port, every key still handles,
/// ticks and draws within one frame: the switch to that tab says it is
/// loading, and the other tab keeps moving. Once the port opens, the held
/// load lands.
#[test]
fn no_key_waits_while_an_effect_is_held() {
    let (first, second) = (fixture(3), fixture(1));
    let storage = storage();
    let start = plan(&args(vec![path_of(&first), path_of(&second)]));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let gate = Arc::new(Gate::default());
        let ports = Ports {
            git: Arc::new(GatedGit(Arc::clone(&gate))),
            github: super::tests::fake_github(),
        };
        let mut shell = shell_on(ports, &start, storage.path());
        settle(&mut shell, loaded);
        let mut hl = Highlighter::new();
        let keys: Vec<TabKey> = shell
            .host()
            .registry()
            .tabs_view()
            .tabs
            .iter()
            .map(|tab| tab.key)
            .collect();

        gate.set(true);
        let mut slowest = frame_after(&mut shell, &mut hl, key(KeyCode::Char(']')));
        assert!(rows(&shell_frame(&mut shell, &mut hl))
            .join("\n")
            .contains("Loading…"));
        for code in [
            KeyCode::Char('['),
            KeyCode::Char('k'),
            KeyCode::Char('n'),
            KeyCode::Char('j'),
            KeyCode::Char(']'),
        ] {
            slowest = slowest.max(frame_after(&mut shell, &mut hl, key(code)));
        }
        gate.set(false);
        assert!(
            slowest < FRAME_BOUND,
            "a key held the loop for {slowest:?} while a load was held"
        );
        assert_eq!(shell.host().registry().tabs_view().active, Some(keys[1]));
        assert!(
            settle_within(&mut shell, WAIT, loaded),
            "the held load never landed"
        );
    });
}

/// A refusal holds the status line until it has been drawn for its dwell:
/// an ordinary notice told in the meantime waits, then shows.
#[test]
fn a_refusal_holds_until_it_has_been_drawn() {
    let repo = fixture(1);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        shell.land_failed_reply("summary failed (feature): no agent".into());
        // Another task's ordinary notice, landing in the same tick.
        (shell.host().task_hooks().notify)("tour completed (feature)".into(), false);
        shell.tick(Instant::now() + Duration::from_secs(60));
        let shown = |shell: &Shell| shell.view().notification.clone().unwrap_or_default();
        assert!(
            shown(&shell).starts_with("summary failed"),
            "{}",
            shown(&shell)
        );
        shell_frame(&mut shell, &mut Highlighter::new());
        shell.tick(Instant::now() + Duration::from_secs(1));
        assert!(
            shown(&shell).starts_with("summary failed"),
            "{}",
            shown(&shell)
        );
        shell.tick(Instant::now() + Duration::from_secs(60));
        assert_eq!(shown(&shell), "tour completed (feature)");
    });
}

/// A key press ends a refusal's hold: the notice that waited behind it shows
/// at once, where it used to be dropped.
#[test]
fn a_key_press_shows_the_notice_a_refusal_held_back() {
    let repo = fixture(2);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        shell.land_failed_reply("summary failed (feature): no agent".into());
        (shell.host().task_hooks().notify)("tour completed (feature)".into(), false);
        shell.tick(Instant::now());
        let mut highlighter = Highlighter::new();
        shell_frame(&mut shell, &mut highlighter);
        shell.handle_key(key(KeyCode::Char('j')));
        let shown = |shell: &Shell| shell.view().notification.clone().unwrap_or_default();
        assert_eq!(shown(&shell), "tour completed (feature)");
        let start = Instant::now();
        let mut seen = vec![shown(&shell)];
        for secs in [1, 5, 30, 60, 120] {
            shell_frame(&mut shell, &mut highlighter);
            shell.tick(start + Duration::from_secs(secs));
            seen.push(shown(&shell));
        }
        assert!(
            seen.contains(&"tour completed (feature)".to_string()),
            "never shown: {seen:?}"
        );
    });
}

/// A key's own notice replaces a held refusal at once, and the notice that
/// waited behind the refusal with it.
#[test]
fn a_key_notice_replaces_a_held_refusal_at_once() {
    let repo = fixture(2);
    let storage = storage();
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let shown = |shell: &Shell| shell.view().notification.clone().unwrap_or_default();
        shell.land_failed_reply("summary failed (feature): no agent".into());
        shell.handle_key(key(KeyCode::Char('!')));
        assert_eq!(shown(&shell), "Showing unreviewed only");

        shell.land_failed_reply("review failed (feature): no agent".into());
        (shell.host().task_hooks().notify)("tour completed (feature)".into(), false);
        shell.tick(Instant::now());
        shell.handle_key(key(KeyCode::Char('!')));
        assert_eq!(shown(&shell), "Showing all files");
        shell_frame(&mut shell, &mut Highlighter::new());
        shell.tick(Instant::now() + Duration::from_secs(60));
        assert_ne!(shown(&shell), "tour completed (feature)");
    });
}
