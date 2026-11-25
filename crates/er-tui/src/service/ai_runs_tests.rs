//! The flag-on AI runs (P74.2): each AI hub and Verify row plans its run on
//! the service host, a rerun clears first and runs once the clear lands, a
//! pick moves the default the next run reads, and the Log panel and the
//! status bar draw the host's bucket log and running rows as the App draws
//! its own. Every agent is the gated `/bin/sh` fake, which ends on its own.

use std::path::Path;
use std::time::Duration;

use crossterm::event::KeyCode;
use er_engine::ai::PanelContent;
use er_engine::config::ErConfig;
use er_service::test_support::{open_gate, Agent};

use super::ai_runs::Rerun;
use super::shell::Shell;
use super::tests::{
    app_frame, app_on, fixture, key, loaded, path_of, rows, settle, settle_within, shell,
    shell_frame, SOURCES,
};
use super::threads_tests::{active_key, args, er_dir};
use super::tui_view::{Ask, Mode, Overlay};
use crate::ui::highlight::Highlighter;

/// The gated agent with `body` before its wait: `$1` is the prompt and `$2`
/// the picked model's argument. `extra` is appended to its config.
fn agent_with(root: &Path, body: &str, extra: &str) -> Agent {
    let agent = Agent::in_dir(root);
    let (gate, settings) = (agent.first.display(), agent.settings.display());
    let script = format!(
        r#"printf 'line-a\n'; {body}; n=0; while [ ! -e "{gate}" ] && [ -d "{settings}" ] && [ $n -lt 1500 ]; do sleep 0.02; n=$((n+1)); done"#
    );
    let path = agent.write_script_config(&script);
    let models = "args = [\"model-one\"]\n\n[[ai_hub.providers.gated.models]]\nid = \"two\"\nargs = [\"model-two\"]\n";
    let config = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&path, format!("{config}{models}{extra}")).unwrap_or_else(|e| panic!("{e}"));
    agent
}

fn shell_on(repos: &[&tempfile::TempDir], agent: &Agent) -> Shell {
    let paths = repos.iter().map(|repo| path_of(repo)).collect();
    let mut shell = shell(&args(paths), &agent.settings);
    settle(&mut shell, loaded);
    shell
}

/// Opens the hub key `hub` opens, or the picker already open for `' '`, and
/// chooses the row labelled `label`.
pub(super) fn choose(shell: &mut Shell, hub: char, label: &str) {
    if hub != ' ' {
        shell.handle_key(key(KeyCode::Char(hub)));
    }
    let Some(Overlay::Picker(picker)) = shell.view_mut().overlay.as_mut() else {
        panic!("no picker open for {label}");
    };
    let at = picker.rows.iter().position(|row| row.label == label);
    picker.selected = at.unwrap_or_else(|| panic!("no row {label}"));
    shell.handle_key(key(KeyCode::Enter));
}

pub(super) fn kinds(shell: &Shell) -> Vec<String> {
    let rows = shell.host().tasks().rows();
    rows.into_iter().map(|row| row.kind).collect()
}

fn log_of(shell: &Shell, er_dir: &str) -> String {
    let lines = shell.host().tasks().bucket_log(er_dir);
    lines.into_iter().map(|line| line.text + "\n").collect()
}

/// Lets every agent end and waits until no row runs or waits, so no shell
/// outlives the test.
fn release(shell: &mut Shell, agent: &Agent) {
    open_gate(&agent.first);
    let idle = |shell: &Shell| {
        let rows = shell.host().tasks().rows();
        !rows
            .iter()
            .any(|row| row.status == "running" || row.status == "queued")
    };
    let ended = settle_within(shell, Duration::from_secs(20), idle);
    assert!(
        ended,
        "an agent still runs: {:?}",
        shell.host().tasks().rows()
    );
}

/// No flag-on module runs an agent, prepares a diff, aggregates job rows or
/// keeps the App's loop duties: each run goes to the host.
#[test]
fn flag_on_runs_go_through_the_host() {
    let banned = [
        "spawn_agent_prompt",
        "launch_agent_prompt",
        "spawn_command",
        "launch_shell_command",
        "hold_agent",
        "enqueue(",
        "ensure_diff_artifacts",
        "ensure_review_inputs",
        "check_commands",
        "poll_background_tasks",
        "drain_agent_log",
        "job_rows",
        "published_views",
        ".review_flows",
        ".arena_runs",
    ];
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for word in banned {
            assert!(!flat.contains(word), "{name} names {word}");
        }
    }
}

/// Each AI hub run row, the expert picker and both Verify hubs list a row
/// of their kind on the tab's bucket. The host refuses importance for a
/// custom CLI, and its notice says why on the status line.
#[test]
fn each_run_row_runs_its_kind() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let extra = "\n[commands]\ntest = \"printf hi\"\n\n[packages.web]\nlint = \"printf lint\"\n";
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = agent_with(storage.path(), "true", extra);
        let mut shell = shell_on(&[&repo], &agent);
        let rows = [
            ('a', "Triage branch", "triage"),
            ('a', "Review work", "review"),
            ('a', "Generate summary", "summary"),
            ('a', "Generate guided tour", "tour"),
            ('a', "Professor", "professor"),
            ('a', "Propose risk rules", "importance"),
            ('v', "Run tests", "test"),
        ];
        for (hub, label, kind) in rows {
            choose(&mut shell, hub, label);
            let listed = |shell: &Shell| kinds(shell).iter().any(|k| k == kind);
            let found = settle_within(&mut shell, Duration::from_secs(20), listed);
            assert!(found, "{label} listed {:?}, not {kind}", kinds(&shell));
            if kind == "importance" {
                settle(&mut shell, |shell| told(shell, "custom CLI"));
            }
        }
        choose(&mut shell, 'a', "Generate summary");
        settle(&mut shell, |shell| told(shell, "summary already running"));
        choose(&mut shell, 'a', "Specialized review");
        let expert = er_engine::ai::EXPERTS[0];
        choose(&mut shell, ' ', expert.label);
        choose(&mut shell, 'v', "web");
        choose(&mut shell, ' ', "Run linter");
        let expected = [format!("expert:{}", expert.id), "web/lint".into()];
        let listed = |shell: &Shell| expected.iter().all(|kind| kinds(shell).contains(kind));
        let found = settle_within(&mut shell, Duration::from_secs(20), listed);
        assert!(found, "{:?}", kinds(&shell));
        let bucket = er_dir(&shell);
        let rows = shell.host().tasks().rows();
        let elsewhere: Vec<_> = rows.iter().filter(|row| row.er_dir != bucket).collect();
        assert!(elsewhere.is_empty(), "{elsewhere:?}");
        release(&mut shell, &agent);
    });
}

/// With a review in the bucket, `Review work` asks first. `y` sends the
/// review clear and plans the run only once the clear lands, on the tab it
/// was asked on even after a tab switch, so the agent finds no
/// `review.json`. `k` keeps the review and runs.
#[test]
fn a_rerun_clears_first_then_runs_on_its_tab() {
    let (first, second) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let found = format!(
            r#"if [ -n "$(find "{}" -name review.json)" ]; then echo review-kept; else echo review-gone; fi"#,
            storage.path().display()
        );
        let agent = agent_with(storage.path(), &found, "");
        let mut shell = shell_on(&[&first, &second], &agent);
        let asked_on = er_dir(&shell);
        let seed = || {
            std::fs::create_dir_all(&asked_on).unwrap_or_else(|e| panic!("{e}"));
            er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
                Path::new(&asked_on),
                serde_json::json!({"version": 1, "diff_hash": "h", "files": {
                    "src/f000.rs": {"risk": "high", "findings": []}
                }}),
            );
        };
        let has_review = |shell: &Shell| {
            let content = shell.active_content();
            content.is_some_and(|content| content.ai.state.review.is_some())
        };
        seed();
        settle(&mut shell, has_review);

        choose(&mut shell, 'a', "Review work");
        let asked = Mode::Confirm(Ask::Rerun(Rerun::Review));
        assert_eq!(shell.view().input_mode, asked);
        shell.handle_key(key(KeyCode::Char('y')));
        let early = shell.runs_in_flight();
        assert_eq!(early, 0, "the run went out before the clear landed");
        shell.handle_key(key(KeyCode::Char(']')));
        settle(&mut shell, |shell| {
            log_of(shell, &asked_on).contains("review-")
        });
        let log = log_of(&shell, &asked_on);
        assert!(log.contains("review-gone"), "{log}");
        settle(&mut shell, loaded);
        let other = log_of(&shell, &er_dir(&shell));
        assert!(
            other.is_empty(),
            "the run took the switched-to tab: {other}"
        );
        release(&mut shell, &agent);

        shell.handle_key(key(KeyCode::Char('[')));
        seed();
        settle(&mut shell, has_review);
        choose(&mut shell, 'a', "Review work");
        shell.handle_key(key(KeyCode::Char('k')));
        settle(&mut shell, |shell| {
            log_of(shell, &asked_on).contains("review-kept")
        });
        release(&mut shell, &agent);
    });
}

/// `Answer questions` over an answered question asks first, and `y` sends
/// the answers' clear before the run: Claude's answer leaves the file.
#[test]
fn a_questions_rerun_clears_the_answers_first() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = agent_with(storage.path(), "true", "");
        let mut shell = shell_on(&[&repo], &agent);
        let dir = er_dir(&shell);
        let question = |id: &str, reply: Option<&str>, author: &str| {
            serde_json::json!({"id": id, "file": "src/f000.rs", "hunk_index": 0,
                "line_start": 4, "text": id, "in_reply_to": reply, "author": author})
        };
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        er_engine::test_support::write_sidecar_json::<er_engine::ai::ErQuestions>(
            Path::new(&dir),
            serde_json::json!({"version": 1, "diff_hash": "h", "questions": [
                question("q-1", None, "You"),
                question("q-2", None, "You"),
                question("a-1", Some("q-1"), "Claude"),
            ]}),
        );
        settle(&mut shell, |shell| {
            let content = shell.active_content();
            content.is_some_and(|c| c.ai.state.has_unanswered_questions())
        });
        choose(&mut shell, 'a', "Answer questions");
        let asked = Mode::Confirm(Ask::Rerun(Rerun::Questions));
        assert_eq!(shell.view().input_mode, asked);
        shell.handle_key(key(KeyCode::Char('y')));
        settle(&mut shell, |shell| {
            kinds(shell).iter().any(|k| k == "questions")
        });
        let file = std::fs::read_to_string(Path::new(&dir).join("questions.json"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!file.contains("a-1"), "the answer stayed: {file}");
        release(&mut shell, &agent);
    });
}

/// A pick saves the default through the settings actor, and the next run
/// reads it: the agent gets the picked model's argument.
#[test]
fn a_pick_moves_the_model_the_next_run_uses() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = agent_with(storage.path(), r#"printf '%s\n' "$2""#, "");
        let mut shell = shell_on(&[&repo], &agent);
        choose(&mut shell, 'a', "Change agent/model");
        choose(&mut shell, ' ', "Gated");
        choose(&mut shell, ' ', "two");
        settle(&mut shell, |shell| {
            let view = shell.host().settings().view();
            view.config.ai_hub.default_model.as_deref() == Some("two")
        });
        choose(&mut shell, 'a', "Generate summary");
        let dir = er_dir(&shell);
        settle(&mut shell, |shell| log_of(shell, &dir).contains("model-"));
        let log = log_of(&shell, &dir);
        assert!(log.contains("model-two"), "{log}");
        release(&mut shell, &agent);
    });
}

/// One review run of the gated agent: its output reaches the Log panel
/// while it runs, and the panel and the status bar's running badge draw as
/// the App draws the same lines and the same running command. Once the tab
/// closes, its bucket's log goes.
#[test]
fn the_log_panel_and_running_badge_draw_like_the_app() {
    let _golden = super::parity::guard();
    let (app_repo, flag_repo, other) = (fixture(1), fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = agent_with(storage.path(), "true", "");
        let mut shell = shell_on(&[&flag_repo, &other], &agent);
        let tab = active_key(&shell);
        choose(&mut shell, 'a', "Review work");
        let dir = er_dir(&shell);
        settle(&mut shell, |shell| log_of(shell, &dir).contains("line-a"));
        if let Some(state) = shell.view_mut().tab_mut_if_open(tab) {
            state.panel = Some(PanelContent::AgentLog);
        }
        shell.view_mut().notification = None;
        let mut app = app_on(&path_of(&app_repo));
        let app_tab = app.tab_mut();
        app_tab.agent_log = shell.host().tasks().bucket_log(&dir).into();
        let running = er_engine::app::CommandStatus::Running;
        app_tab.command_status.insert("review".into(), running);
        app_tab.panel = Some(PanelContent::AgentLog);
        app.clear_notification();
        let mut hl = Highlighter::new();
        let flag = rows(&shell_frame(&mut shell, &mut hl));
        let app_rows = rows(&app_frame(&app, &mut hl));
        // The flag frame lists two tabs, so its rows sit one lower.
        let panel = |rows: &[String]| -> Vec<String> {
            let top = rows.iter().position(|row| row.contains("[File] [Log]"));
            let rows = &rows[top.unwrap_or_else(|| panic!("no panel"))..rows.len() - 1];
            rows.iter()
                .filter_map(|row| row.rsplit('│').next())
                .map(str::to_string)
                .collect()
        };
        let flag_panel = panel(&flag);
        let app_panel = panel(&app_rows)[..flag_panel.len()].to_vec();
        super::parity::assert_json("log", &flag_panel, &app_panel);
        assert_eq!(
            flag_panel,
            panel(&app_rows)[..flag_panel.len()],
            "the Log panel differs"
        );
        let printed = flag_panel.iter().any(|row| row.contains("] line-a"));
        assert!(printed, "no output line:\n{}", flag.join("\n"));
        let badge = " ◐ Review running… ";
        for drawn in [&flag, &app_rows] {
            assert!(drawn.iter().any(|row| row.contains(badge)), "no badge");
        }
        release(&mut shell, &agent);

        let close = er_service::AppCommand::CloseTab { key: tab };
        let closed = shell.host().registry().send(close);
        assert!(closed.is_ok(), "{closed:?}");
        settle(&mut shell, |shell| log_of(shell, &dir).is_empty());
    });
}

/// Seeds a one-file review into `dir` and waits until the tab shows it.
fn seed_review(shell: &mut Shell, dir: &str) {
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
        Path::new(dir),
        serde_json::json!({"version": 1, "diff_hash": "h", "files": {
            "src/f000.rs": {"risk": "high", "findings": []}
        }}),
    );
    settle(shell, |shell| {
        let content = shell.active_content();
        content.is_some_and(|content| content.ai.state.review.is_some())
    });
    super::tests::let_the_reload_notice_go(shell);
}

pub(super) fn notice(shell: &Shell) -> String {
    shell.view().notification.clone().unwrap_or_default()
}

/// Whether the status line has shown a message holding `text`. A task's
/// notice can replace it in the tick it lands in, so the current message
/// alone can miss it.
fn told(shell: &Shell, text: &str) -> bool {
    shell.told.iter().any(|message| message.contains(text))
}

/// A clear the bucket's lease refuses starts no run: the status line says
/// why and the host lists no task.
#[test]
fn a_refused_clear_starts_no_run() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = agent_with(storage.path(), "true", "");
        let mut shell = shell_on(&[&repo], &agent);
        let dir = er_dir(&shell);
        seed_review(&mut shell, &dir);
        let lease = super::clears_tests::take_lease(&dir);
        choose(&mut shell, 'a', "Review work");
        shell.handle_key(key(KeyCode::Char('y')));
        settle(&mut shell, |shell| shell.in_flight() == 0);
        let told = notice(&shell);
        assert!(told.starts_with("Review not started: "), "{told}");
        assert_eq!(shell.runs_started, 0, "a refused clear asked for its run");
        assert_eq!(kinds(&shell), Vec::<String>::new(), "a refused clear ran");
        drop(lease);
        release(&mut shell, &agent);
    });
}

/// `y` on a review rerun over a diff with no file says so and keeps the
/// review, where clearing first would leave nothing.
#[test]
fn an_empty_diff_keeps_its_review_on_a_rerun() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = agent_with(storage.path(), "true", "");
        let mut shell = shell_on(&[&repo], &agent);
        shell.handle_key(key(KeyCode::Char('2')));
        settle(&mut shell, |shell| {
            let content = shell.active_content();
            content.is_some_and(|c| c.tab.mode == er_engine::tab_identity::DiffMode::Unstaged)
        });
        let dir = er_dir(&shell);
        seed_review(&mut shell, &dir);
        choose(&mut shell, 'a', "Review work");
        shell.handle_key(key(KeyCode::Char('y')));
        settle_within(&mut shell, Duration::from_millis(500), |_| false);
        assert_eq!(notice(&shell), "Review not started: Nothing to review");
        assert!(
            Path::new(&dir).join("review.json").exists(),
            "the review went"
        );
        release(&mut shell, &agent);
    });
}

/// A test build refuses a provider CLI on `PATH`: with the default
/// provider, `claude`, the spawn is refused, and the run's failure replaces
/// the start notice the key showed.
#[test]
fn a_test_build_never_starts_a_real_provider_cli() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let view = shell.host().settings().view();
        let provider = view
            .config
            .ai_hub
            .resolve_default_selection(&view.config.agent);
        assert_eq!(provider.provider_id.as_deref(), Some("claude"));
        let before = er_agent::agent_run::real_cli_refusals();
        choose(&mut shell, 'a', "Generate summary");
        assert_eq!(notice(&shell), "summary started...");
        let refused = |_: &Shell| er_agent::agent_run::real_cli_refusals() > before;
        let seen = settle_within(&mut shell, Duration::from_secs(20), refused);
        assert!(seen, "the spawn was not refused");
        settle(&mut shell, |shell| told(shell, "summary failed"));
    });
}

/// Writes `kind`'s sidecar (`questions` or `notes`) with one open row and,
/// with `answered`, Claude's reply to it.
fn seed_rows(shell: &mut Shell, kind: &str, answered: bool) {
    let dir = er_dir(shell);
    let row = |id: &str, reply: Option<&str>, author: &str| {
        serde_json::json!({"id": id, "file": "src/f000.rs", "hunk_index": 0,
            "line_start": 4, "text": id, "in_reply_to": reply, "author": author})
    };
    let mut rows = vec![row("open-1", None, "You")];
    if answered {
        rows.extend([
            row("done-1", None, "You"),
            row("reply-1", Some("done-1"), "Claude"),
        ]);
    }
    let value = serde_json::json!({"version": 1, "diff_hash": "h", kind: rows});
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    match kind {
        "notes" => er_engine::test_support::write_sidecar_json::<er_engine::ai::ErNotes>(
            Path::new(&dir),
            value,
        ),
        _ => er_engine::test_support::write_sidecar_json::<er_engine::ai::ErQuestions>(
            Path::new(&dir),
            value,
        ),
    }
    settle(shell, |shell| {
        let content = shell.active_content();
        content.is_some_and(|c| {
            let ai = &c.ai.state;
            if kind == "notes" {
                ai.has_unaddressed_notes()
            } else {
                ai.has_unanswered_questions()
            }
        })
    });
}

/// Validate review runs `validate` over a review and says it started; a
/// question with no answer of Claude's runs at once, with no prompt; and
/// the notes rerun clears Claude's reply before it runs.
#[test]
fn validate_questions_and_notes_run_their_kinds() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = agent_with(storage.path(), "true", "");
        let mut shell = shell_on(&[&repo], &agent);
        let dir = er_dir(&shell);
        let listed =
            |kind: &'static str| move |shell: &Shell| kinds(shell).iter().any(|k| k == kind);
        seed_review(&mut shell, &dir);
        choose(&mut shell, 'a', "Validate review");
        settle(&mut shell, listed("validate"));
        assert_eq!(notice(&shell), "Validation started (review)");

        seed_rows(&mut shell, "questions", false);
        choose(&mut shell, 'a', "Answer questions");
        assert_eq!(
            shell.view().input_mode,
            Mode::Normal,
            "a fresh question asked"
        );
        settle(&mut shell, listed("questions"));

        seed_rows(&mut shell, "notes", true);
        choose(&mut shell, 'a', "Address notes");
        let asked = Mode::Confirm(Ask::Rerun(Rerun::Notes));
        assert_eq!(shell.view().input_mode, asked);
        shell.handle_key(key(KeyCode::Char('y')));
        settle(&mut shell, listed("notes"));
        let notes =
            er_engine::test_support::read_sidecar::<er_engine::ai::ErNotes>(Path::new(&dir));
        let replies: Vec<_> = notes
            .notes
            .iter()
            .filter(|n| n.in_reply_to.is_some())
            .collect();
        assert!(replies.is_empty(), "Claude's reply stayed: {replies:?}");
        release(&mut shell, &agent);
    });
}

/// One project at `root` whose review ignores `globs`.
fn ignore_in(root: &str, globs: &[&str]) {
    er_engine::projects_pins::update(|file| {
        file.projects.push(er_engine::projects_pins::ProjectRecord {
            id: "local".into(),
            name: "repo".into(),
            root_path: root.into(),
            review_ignore_globs: globs.iter().map(|glob| (*glob).to_string()).collect(),
            ..Default::default()
        });
        Ok(true)
    })
    .unwrap_or_else(|e| panic!("{e}"));
}

const IGNORE_DIFF: &str = "diff --git a/keep.rs b/keep.rs\n--- a/keep.rs\n+++ b/keep.rs\n@@ -0,0 +1 @@\n+keep\n\ndiff --git a/ignored.rs b/ignored.rs\n--- a/ignored.rs\n+++ b/ignored.rs\n@@ -0,0 +1 @@\n+ignore\n";

/// A run's diff leaves out the files the project's review-ignore globs
/// name before it is prepared and hashed, and is left byte for byte when
/// the project names none: the flag path's twin of the App's
/// `review_ignore_preparation_filters_before_hashing` and
/// `review_ignore_empty_globs_preserve_raw_bytes…`.
#[test]
fn review_ignore_globs_filter_the_diff_before_it_is_hashed() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut shell = shell(&args(vec![path_of(&repo)]), storage.path());
        settle(&mut shell, loaded);
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let kept = er_service::runs::filtered(IGNORE_DIFF.into(), &content);
        assert_eq!(kept, IGNORE_DIFF, "no globs changed the bytes");

        ignore_in(&content.tab.repo_root, &["ignored.rs"]);
        let raw = er_service::runs::filtered(IGNORE_DIFF.into(), &content);
        assert!(
            raw.contains("keep.rs") && !raw.contains("ignored.rs"),
            "{raw}"
        );
        let bucket = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let er_dir = bucket.path().to_string_lossy().into_owned();
        let config = ErConfig::default();
        let hash = er_service::runs::review_inputs(
            &super::tests::test_ports(),
            &content,
            &config,
            &er_dir,
            &raw,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let prepared = std::fs::read_to_string(Path::new(&er_dir).join("diff-tmp"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(prepared.contains("keep.rs") && !prepared.contains("ignored.rs"));
        assert_eq!(
            er_engine::ai::prepared_diff::diff_tmp_hash(&er_dir).as_deref(),
            Some(hash.as_str())
        );
    });
}

/// With every file ignored a review is refused before anything is
/// prepared or started: the flag path's twin of the App's
/// `review_ignore_all_files_returns_before_preparation_or_spawn`.
#[test]
fn a_review_with_every_file_ignored_starts_nothing() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = agent_with(storage.path(), "true", "");
        let mut shell = shell_on(&[&repo], &agent);
        let root = shell
            .active_content()
            .map(|content| content.tab.repo_root.clone())
            .unwrap_or_else(|| panic!("no content"));
        ignore_in(&root, &["*.rs"]);
        let dir = er_dir(&shell);
        choose(&mut shell, 'a', "Review work");
        settle_within(&mut shell, Duration::from_millis(500), |_| false);
        let told = notice(&shell);
        assert!(told.contains("Nothing to review"), "{told}");
        assert!(
            !Path::new(&dir).join("diff-tmp").exists(),
            "a diff was prepared"
        );
        assert_eq!(kinds(&shell), Vec::<String>::new(), "a task started");
        release(&mut shell, &agent);
    });
}
