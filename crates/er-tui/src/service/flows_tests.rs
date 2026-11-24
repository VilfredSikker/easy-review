//! The flag-on review flows (P74.3): the AI hub's Review rows send flow
//! requests through the host and say what the holder actor answered, the
//! hub draws as the App's in each flow state, and the arbiter's drops list
//! as on the App. Every agent is the gated `/bin/sh` fake, which answers a
//! Deep flow by its prompt and ends on its own.

use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use er_engine::ai::accepted_findings::AcceptedFindings;
use er_engine::ai::review_flow::FlowState;
use er_engine::ai::PanelContent;
use er_engine::test_support::{read_sidecar, STAGING_PREAMBLE, WRITE_TRIAGE_SIDECAR};
use er_service::test_support::{open_gate, Agent};
use er_service::LiveReviewFlow;

use super::ai_runs_tests::{choose, notice};
use super::shell::Shell;
use super::tests::{
    app_on, assert_parity, fixture, key, loaded, path_of, press_app, settle, settle_within, shell,
    shell_frame, SOURCES,
};
use super::thread_writes_tests::hub_to;
use super::threads_tests::{active_key, args, er_dir, step};
use super::tui_view::Overlay;
use crate::ui::highlight::Highlighter;

/// A Deep flow runs triage, two reviewers over three rounds and an arbiter.
const FLOW_WAIT: Duration = Duration::from_secs(60);

/// Triage (staged, after WAIT) picks the arena; round one raises two
/// findings; the cross-checks keep, and the arbiter keeps, each finding the
/// prompt names.
const SCRIPT: &str = r#"ids=$(printf '%s' "$1" | grep -oE '"id": ?"[0-9a-f]{40}"' | grep -oE '[0-9a-f]{40}' | sort -u)
case "$1" in
*"arena arbiter"*) v=""; for i in $ids; do v="$v${v:+,}{\"finding_id\":\"$i\",\"verdict\":\"kept\",\"confidence\":0.9,\"rationale\":\"Both agree.\"}"; done; printf '{"verdicts":[%s]}' "$v" ;;
*cross-check*) b=""; for i in $ids; do b="$b${b:+,}{\"finding_id\":\"$i\",\"vote\":\"keep\",\"note\":\"Agreed.\"}"; done; printf '{"ballots":[%s]}' "$b" ;;
*triage.json*) WAIT PREAMBLE; TRIAGE ;;
*"code review arena"*) printf '%s' '{"findings":[{"file":"src/f000.rs","line":4,"title":"First flow finding","body":"Line four changed.","severity":"high"},{"file":"src/f000.rs","line":31,"title":"Second flow finding","body":"Line thirty-one changed.","severity":"medium"}]}' ;;
*) true ;;
esac"#;

/// The flow fake in `storage`'s config, read by both paths; with `gated`,
/// triage waits for the agent's first gate.
fn flow_agent(storage: &Path, gated: bool) -> Agent {
    let agent = Agent::in_dir(storage);
    let (gate, settings) = (agent.first.display(), agent.settings.display());
    let wait = if gated {
        format!(
            r#"n=0; while [ ! -e "{gate}" ] && [ -d "{settings}" ] && [ $n -lt 1500 ]; do sleep 0.02; n=$((n+1)); done;"#
        )
    } else {
        String::new()
    };
    let script = SCRIPT
        .replace("WAIT", &wait)
        .replace("PREAMBLE", STAGING_PREAMBLE)
        .replace("TRIAGE", WRITE_TRIAGE_SIDECAR);
    let path = agent.write_script_config(&script);
    let config = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
    let config = config.replace(
        "max_concurrent_reviews = 50",
        "max_concurrent_reviews = 50\nmax_concurrent_agents = 16",
    );
    let one = r#"{ provider_id = "gated", model_id = "one" }"#;
    let two = r#"{ provider_id = "gated", model_id = "two" }"#;
    let models = format!(
        "\n[[ai_hub.providers.gated.models]]\nid = \"two\"\n\n[ai_hub.review_models]\nfast = [{one}]\nstandard = [{one}]\nadvanced = [{one}, {two}]\n"
    );
    std::fs::write(storage.join("config.toml"), format!("{config}{models}"))
        .unwrap_or_else(|e| panic!("{e}"));
    agent
}

fn flow_shell(repo: &tempfile::TempDir, storage: &Path) -> Shell {
    let mut shell = shell(&args(vec![path_of(repo)]), storage);
    settle(&mut shell, loaded);
    shell
}

/// The active tab's flow as its actor published it.
fn live(shell: &Shell) -> Option<LiveReviewFlow> {
    let key = active_key(shell);
    let tabs = shell.host().registry().tabs_view();
    let entry = tabs.tabs.iter().find(|entry| entry.key == key)?;
    let view = shell
        .host()
        .published_view(&entry.descriptor.workspace_key())?;
    view.review_flow(&er_dir(shell)).cloned()
}

fn flow_until(shell: &mut Shell, what: &str, done: impl Fn(&LiveReviewFlow) -> bool) {
    let reached = settle_within(shell, FLOW_WAIT, |shell| {
        live(shell).is_some_and(|l| done(&l))
    });
    assert!(reached, "{what}: {:?}", live(shell).map(|l| l.flow.state));
}

fn done(live: &LiveReviewFlow) -> bool {
    matches!(live.flow.state, FlowState::Done { .. }) && !live.draining
}

fn app_until(app: &mut er_engine::app::App, what: &str, done: impl Fn(&FlowState, bool) -> bool) {
    let deadline = Instant::now() + FLOW_WAIT;
    loop {
        let er_dir = app.tab().er_dir();
        let draining = app.review_flow_is_draining(&er_dir);
        if app
            .review_flow_for(&er_dir)
            .is_some_and(|flow| done(&flow.state, draining))
        {
            return;
        }
        assert!(Instant::now() < deadline, "the App never reached {what}");
        app.poll_background_tasks();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Opens the AI hub on both paths and compares the frames. "Copy context to
/// clipboard" is P75.1's (#743); until it lands, the flag row reads as the
/// App's.
fn hub_parity(app: &mut er_engine::app::App, shell: &mut Shell, hl: &mut Highlighter, at: &str) {
    app.clear_notification();
    shell.view_mut().notification = None;
    press_app(app, KeyCode::Char('a'));
    shell.handle_key(key(KeyCode::Char('a')));
    if let Some(Overlay::Picker(picker)) = shell.view_mut().overlay.as_mut() {
        let copy = picker.rows.iter_mut();
        if let Some(row) = copy
            .into_iter()
            .find(|row| row.label.starts_with("Copy context"))
        {
            row.description = "Copy diff context for agent terminal".into();
            row.enabled = true;
        }
    }
    assert_parity(app, &shell_frame(shell, hl), hl, at);
}

fn enter_both(app: &mut er_engine::app::App, shell: &mut Shell) {
    press_app(app, KeyCode::Enter);
    shell.handle_key(key(KeyCode::Enter));
}

/// No flag-on module drives a flow on the App's driver or reads a run on
/// the loop: the host builds each request and reads the results.
#[test]
fn flag_on_flows_go_through_the_host() {
    let banned = [
        "with_review_flows",
        "start_review_flow",
        "review_flow_inputs(",
        "current_review_diff(",
        "flow_run_results",
        "toggle_show_dropped",
    ];
    for (name, text) in SOURCES {
        let flat: String = text.split_whitespace().collect();
        for word in banned {
            assert!(!flat.contains(word), "{name} names {word}");
        }
    }
}

/// A Deep flow end to end on the flag path: Review starts it, the row then
/// confirms Deep, the arena runs, the row opens its results, and accepting
/// both findings is answered by the actor holding the flow: its
/// `FlowAccepted { accepted: 2 }` is the only reply that says "Accepted 2",
/// and the bucket's accepted findings then hold both.
#[test]
fn an_accepted_flow_lands_both_findings_in_the_review() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let _agent = flow_agent(storage.path(), false);
        let mut shell = flow_shell(&repo, storage.path());
        choose(&mut shell, 'a', "Review");
        flow_until(&mut shell, "Deep asks to confirm", |live| {
            live.flow.state == FlowState::AwaitingConfirmation
        });
        assert_eq!(notice(&shell), "Review started");
        choose(&mut shell, 'a', "Review");
        flow_until(&mut shell, "the flow is done", done);
        assert_eq!(notice(&shell), "Deep review started");

        choose(&mut shell, 'a', "Review");
        let open = |shell: &Shell| matches!(shell.view().overlay, Some(Overlay::FlowResults(_)));
        settle(&mut shell, open);
        for code in [KeyCode::Char(' '), KeyCode::Char('j'), KeyCode::Char(' ')] {
            shell.handle_key(key(code));
        }
        shell.handle_key(key(KeyCode::Char('a')));
        let accepted = settle_within(&mut shell, FLOW_WAIT, |shell| {
            notice(shell).starts_with("Accepted") || notice(shell).starts_with("Could not")
        });
        assert!(accepted, "no answer: {}", notice(&shell));
        assert_eq!(notice(&shell), "Accepted 2 finding(s)");
        let dir = er_dir(&shell);
        let stored = read_sidecar::<AcceptedFindings>(Path::new(&dir));
        let accepted = serde_json::to_string(&stored).unwrap_or_else(|e| panic!("{e}"));
        for title in ["First flow finding", "Second flow finding"] {
            assert!(accepted.contains(title), "{title} is not in {accepted}");
        }
    });
}

/// Cancel review stops a running flow, and Cancel Deep review declines one
/// that waits for its confirmation; each says what the actor answered.
#[test]
fn the_flow_rows_cancel_and_decline() {
    let repo = fixture(1);
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = flow_agent(storage.path(), true);
        let mut shell = flow_shell(&repo, storage.path());
        choose(&mut shell, 'a', "Review");
        flow_until(&mut shell, "triage runs", |live| {
            live.flow.state == FlowState::Triaging
        });
        choose(&mut shell, 'a', "Cancel review");
        settle(&mut shell, |shell| notice(shell) != "Review started");
        assert_eq!(notice(&shell), "Review stopping");
        open_gate(&agent.first);
        let over = |shell: &Shell| live(shell).is_none_or(|live| !live.draining);
        assert!(
            settle_within(&mut shell, FLOW_WAIT, over),
            "the cancel never drained"
        );

        choose(&mut shell, 'a', "Review");
        flow_until(&mut shell, "Deep asks to confirm", |live| {
            live.flow.state == FlowState::AwaitingConfirmation
        });
        choose(&mut shell, 'a', "Cancel Deep review");
        settle(&mut shell, |shell| notice(shell).starts_with("Deep review"));
        assert_eq!(notice(&shell), "Deep review declined");
        let waits = |shell: &Shell| {
            live(shell).is_some_and(|live| live.flow.state == FlowState::AwaitingConfirmation)
        };
        assert!(!waits(&shell), "the flow still waits to confirm");
    });
}

/// The AI hub draws as the App's in each flow state: none, triage running,
/// Deep waiting to confirm, and done with findings to accept; then the
/// results overlay draws as the App's.
#[test]
fn the_ai_hub_draws_like_the_app_in_each_flow_state() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let agent = flow_agent(storage.path(), true);
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = flow_shell(&flag_repo, storage.path());
        let mut hl = Highlighter::new();
        hub_parity(&mut app, &mut shell, &mut hl, "no flow");
        enter_both(&mut app, &mut shell);
        app_until(&mut app, "triage", |state, _| *state == FlowState::Triaging);
        flow_until(&mut shell, "triage", |live| {
            live.flow.state == FlowState::Triaging
        });
        hub_parity(&mut app, &mut shell, &mut hl, "triage running");
        press_app(&mut app, KeyCode::Esc);
        shell.handle_key(key(KeyCode::Esc));

        open_gate(&agent.first);
        let awaiting = |state: &FlowState| *state == FlowState::AwaitingConfirmation;
        app_until(&mut app, "Deep to confirm", |state, _| awaiting(state));
        flow_until(&mut shell, "Deep to confirm", |live| {
            awaiting(&live.flow.state)
        });
        hub_parity(&mut app, &mut shell, &mut hl, "awaiting Deep");
        enter_both(&mut app, &mut shell);
        app_until(&mut app, "done", |state, draining| {
            matches!(state, FlowState::Done { .. }) && !draining
        });
        flow_until(&mut shell, "done", done);
        hub_parity(&mut app, &mut shell, &mut hl, "done with findings");
        enter_both(&mut app, &mut shell);
        let open = |shell: &Shell| matches!(shell.view().overlay, Some(Overlay::FlowResults(_)));
        settle(&mut shell, open);
        app.clear_notification();
        shell.view_mut().notification = None;
        // Each path names its own run by its start time.
        let app_run = match &app.overlay {
            Some(er_engine::app::OverlayData::ReviewFlowResults { results, .. }) => {
                results.run_id.clone()
            }
            _ => panic!("the App's results are not open"),
        };
        if let Some(Overlay::FlowResults(results)) = shell.view_mut().overlay.as_mut() {
            results.results.run_id = app_run;
        }
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "results");
    });
}

/// A dead owner's Deep flow shows interrupted on load on both paths: the
/// hub and its progress overlay draw as the App's, and `r` there re-runs
/// it on the holder actor.
#[test]
fn an_interrupted_flow_shows_and_reruns_like_the_app() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let _agent = flow_agent(storage.path(), false);
        for repo in [&app_repo, &flag_repo] {
            let bucket = app_on(&path_of(repo)).tab().er_dir();
            interrupted_record(Path::new(&bucket));
        }
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = flow_shell(&flag_repo, storage.path());
        let interrupted =
            |live: &LiveReviewFlow| matches!(live.flow.state, FlowState::Interrupted { .. });
        flow_until(&mut shell, "the record shows", interrupted);
        let mut hl = Highlighter::new();
        hub_parity(&mut app, &mut shell, &mut hl, "interrupted");
        hub_to(&mut app, &mut shell, "Review task progress");
        enter_both(&mut app, &mut shell);
        // Each path's bucket is under its own repo.
        let Some(Overlay::FlowProgress(progress)) = shell.view_mut().overlay.as_mut() else {
            panic!("the progress is not open");
        };
        let own = std::mem::replace(&mut progress.er_dir, app.tab().er_dir());
        assert_parity(&app, &shell_frame(&mut shell, &mut hl), &mut hl, "progress");
        if let Some(Overlay::FlowProgress(progress)) = shell.view_mut().overlay.as_mut() {
            progress.er_dir = own;
        }
        shell.handle_key(key(KeyCode::Char('r')));
        flow_until(&mut shell, "the re-run starts", |live| !interrupted(live));
        assert!(shell.view().overlay.is_none(), "the progress stayed open");
        flow_until(&mut shell, "the re-run settles", |live| {
            live.flow.state == FlowState::AwaitingConfirmation
        });
    });
}

/// A Deep record whose owner died mid-triage, so its re-run starts over.
fn interrupted_record(bucket: &Path) {
    std::fs::create_dir_all(bucket).unwrap_or_else(|e| panic!("{e}"));
    let record = serde_json::json!({
        "version": 1, "diff_hash": "hash", "started_at": "t0", "kind": "deep",
        "holder": { "pid": 1, "label": "flow-1" },
        "stages": [{ "id": "triage", "stage": "triage", "label": "Triage",
                     "status": "running", "diff_hash": "hash" }]
    });
    er_engine::test_support::write_sidecar_json::<er_engine::ai::workflow_record::WorkflowRecord>(
        bucket, record,
    );
}

/// Show arbiter's drops lists the finding the arbiter dropped in the AI
/// summary, and hides it again, as the App's row does.
#[test]
fn the_drops_row_lists_like_the_app() {
    let (app_repo, flag_repo) = (fixture(1), fixture(1));
    let storage = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    er_engine::storage::with_process_test_storage_root(storage.path(), || {
        let mut app = app_on(&path_of(&app_repo));
        let mut shell = flow_shell(&flag_repo, storage.path());
        let hash = app.tab().branch_diff_hash.clone();
        dropped_review(&app.tab().er_dir(), &hash);
        app.tab_mut().reload_ai_state();
        app.tab_mut().panel = Some(PanelContent::AiSummary);
        let content = shell
            .active_content()
            .unwrap_or_else(|| panic!("no content"));
        let revision = content.ai.revision;
        dropped_review(&content.context.er_dir, &content.context.branch_diff_hash);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut ahead = 3600;
        let reloaded = |shell: &Shell| {
            let content = shell.active_content();
            content.is_some_and(|content| content.ai.revision > revision)
        };
        while !reloaded(&shell) && Instant::now() < deadline {
            ahead += 1;
            shell.tick(Instant::now() + Duration::from_secs(ahead));
        }
        let tab = active_key(&shell);
        if let Some(state) = shell.view_mut().tab_mut_if_open(tab) {
            state.panel = Some(PanelContent::AiSummary);
        }
        let mut hl = Highlighter::new();
        for _ in 0..2 {
            press_app(&mut app, KeyCode::Char('a'));
            shell.handle_key(key(KeyCode::Char('a')));
            hub_to(&mut app, &mut shell, "Show arbiter's drops");
            step(&mut app, &mut shell, &mut hl, key(KeyCode::Enter));
        }
    });
}

/// A review of two findings, one of which the arbiter dropped.
fn dropped_review(dir: &str, hash: &str) {
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
    let finding = |id: &str, line: usize| {
        serde_json::json!({"id": id, "severity": "medium", "title": format!("Finding {id}"),
            "hunk_index": 0, "line_start": line, "confidence": "tentative"})
    };
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
        dir,
        serde_json::json!({"version": 1, "diff_hash": hash, "files": {
            "src/f000.rs": {"risk": "medium", "findings": [finding("f1", 4), finding("f2", 5)]}
        }}),
    );
    let verdict = |id: &str, line: usize, verdict: &str| {
        let key = er_engine::ai::finding_key("src/f000.rs", Some(line), &format!("Finding {id}"));
        serde_json::json!({"id": key, "file": "src/f000.rs", "verdict": verdict,
            "confidence": "tentative"})
    };
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ArbiterReview>(
        dir,
        serde_json::json!({"version": 1, "diff_hash": hash,
            "verdicts": [verdict("f1", 4, "kept"), verdict("f2", 5, "dropped")]}),
    );
}
