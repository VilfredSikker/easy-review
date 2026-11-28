//! The card asks: each reads the tab's content and the settings view, shows
//! its pending reply in `jobs`, and lands its answer in the bucket it
//! captured.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{Ack, Command, Jobs, Resource, ResourceKey};
use er_service::{AnnotationTarget, WorkspaceDeps};

use super::arena_resource_tests::{bucket, local_ports, open, wait_until};
use super::diff_command_tests::send;
use super::diff_resource_tests::with_storage;
use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::resources::content_routes;
use crate::route_helpers::DesktopState;
use crate::service::{actor_key, ServiceState};

/// A provider that waits for the gate file, then answers with its own
/// arguments, so the reply names the model it ran. It stops waiting after
/// about thirty seconds, so a test that fails before it opens the gate
/// leaves no shell behind.
fn config(gate: &Path) -> String {
    let gate = gate.display();
    format!(
        r#"
[ai_hub]
default_provider = "echo"
default_model = "first"

[ai_hub.providers.echo]
label = "Echo"
command = "/bin/sh"
args = ["-c", '''n=0; while [ ! -e "{gate}" ] && [ $n -lt 1500 ]; do sleep 0.02; n=$((n+1)); done; printf '%s\n' "$@"''', "echo-agent", "{{prompt}}"]

[[ai_hub.providers.echo.models]]
id = "first"
args = ["mark-first"]

[[ai_hub.providers.echo.models]]
id = "second"
args = ["mark-second"]
"#
    )
}

/// A state whose asks run the gated provider, and the gate.
pub(super) fn asking(dir: &ReviewFlowTestDir) -> (DesktopState, PathBuf) {
    let gate = dir.path().join("gate");
    let path = dir.path().join("settings").join("config.toml");
    std::fs::create_dir_all(dir.path().join("settings")).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&path, config(&gate)).unwrap_or_else(|e| panic!("config: {e}"));
    let mut state = review_flow_test_state(dir);
    let ports = local_ports();
    state.ports = ports.clone();
    let deps = WorkspaceDeps::new(ports);
    state.service = Arc::new(ServiceState::with_deps(deps, |_| {}).with_config_path(path));
    (state, gate)
}

fn release(gate: &Path) {
    std::fs::write(gate, "").unwrap_or_else(|e| panic!("gate: {e}"));
}

fn wire(key: TabKey) -> er_protocol::TabKey {
    er_service::wire_tab_key(key)
}

/// A question on the tab's first file, written through its actor.
pub(super) fn question(state: &DesktopState, (key, tab): &(TabKey, TabDescriptor)) -> String {
    let _ = bucket(state, &(*key, tab.clone()));
    let content = state
        .service
        .content(*key, tab)
        .unwrap_or_else(|| panic!("no content"));
    let written = state.service.send(
        &actor_key(tab),
        er_service::Command::AddQuestion {
            context: content.context.clone(),
            target: AnnotationTarget {
                file: "src/f0.rs".into(),
                hunk_idx: 0,
                line_num: Some(1),
                line_num_end: None,
            },
            body: "Why this change?".into(),
            side: None,
            client_id: None,
            reply_to: None,
            finding_ref: None,
        },
    );
    match written {
        Ok(er_service::Reply::AnnotationWritten { id }) => {
            wait_until("the question in the content", || {
                state.service.content(*key, tab).is_some_and(|c| {
                    c.ai.state
                        .questions
                        .as_ref()
                        .is_some_and(|q| q.questions.iter().any(|q| q.id == id))
                })
            });
            id
        }
        other => panic!("no question: {other:?}"),
    }
}

/// The replies on disk in `bucket` to `question`: author, action, text.
pub(super) fn replies(bucket: &str, question: &str) -> Vec<(String, Option<String>, String)> {
    er_engine::ai::load_ai_state(bucket, "", None)
        .questions
        .map(|q| q.questions)
        .unwrap_or_default()
        .into_iter()
        .filter(|q| q.in_reply_to.as_deref() == Some(question))
        .map(|q| (q.author, q.ai_action, q.text))
        .collect()
}

pub(super) fn jobs(state: &DesktopState) -> Jobs {
    match crate::resources::fetch(state, ResourceKey::Jobs).map(|f| f.value) {
        Ok(Resource::Jobs(jobs)) => jobs,
        other => panic!("not jobs: {other:?}"),
    }
}

pub(super) fn pending(state: &DesktopState) -> Vec<String> {
    jobs(state)
        .ai_replies
        .into_iter()
        .map(|r| r.target)
        .collect()
}

pub(super) fn ask(state: &DesktopState, tab: TabKey, thread: &str) -> Ack {
    let command = Command::AskAi {
        tab: wire(tab),
        thread_id: thread.to_string(),
        prompt: "Explain".into(),
    };
    send(state, "ask_ai", command).unwrap_or_else(|e| panic!("{e:?}"))
}

/// An `er_send` ask shows its reply as pending in
/// `jobs`, named by the `Ack`, until the answer lands; then the pending
/// entry clears and the answer is the AI's reply in the thread.
#[test]
fn an_ask_shows_its_pending_reply_in_jobs_until_the_answer_lands() {
    with_storage(|dir| {
        let (state, gate) = asking(dir);
        let tab = open(&state, dir.path(), "a");
        let asked = question(&state, &tab);
        let ack = ask(&state, tab.0, &asked);
        assert!(
            ack.changed.iter().any(|c| c.key == ResourceKey::Jobs),
            "{ack:?}"
        );
        assert_eq!(pending(&state), std::slice::from_ref(&asked));

        release(&gate);
        let bucket = bucket(&state, &tab);
        wait_until("the answer", || !replies(&bucket, &asked).is_empty());
        wait_until("the pending reply to clear", || pending(&state).is_empty());
        let answers = replies(&bucket, &asked);
        assert_eq!(answers.len(), 1, "{answers:?}");
        assert_eq!(answers[0].0, "ai");
        assert_eq!(answers[0].1.as_deref(), Some("ask"));
    });
}

/// A pick saved through `er_send` runs the next ask with its model: the
/// selection comes from the settings view.
#[test]
fn a_saved_pick_runs_the_next_ask() {
    with_storage(|dir| {
        let (state, gate) = asking(dir);
        release(&gate);
        let tab = open(&state, dir.path(), "a");
        let asked = question(&state, &tab);
        let pick = Command::SetAiSelection {
            provider_id: "echo".into(),
            model_id: Some("second".into()),
            effort: None,
        };
        let picked = send(&state, "set_ai_selection", pick);
        assert!(picked.is_ok(), "{picked:?}");

        let thread = asked.clone();
        let started = super::on_a_thread(&state, "the ask", move |state| {
            crate::resources::ai_asks::ask(state, None, thread, "Explain".into())
        });
        assert!(started.is_ok(), "{:?}", started.map(drop));

        let bucket = bucket(&state, &tab);
        wait_until("the answer", || !replies(&bucket, &asked).is_empty());
        let answer = replies(&bucket, &asked).remove(0).2;
        assert!(answer.contains("mark-second"), "{answer}");
        assert!(!answer.contains("mark-first"), "{answer}");
    });
}

/// An ask on a thread and a validation of a finding each start, land their
/// answers (a reply, a finding response) and clear their pending entries.
#[test]
fn an_ask_and_a_validation_land_their_answers() {
    with_storage(|dir| {
        let (state, gate) = asking(dir);
        let tab = open(&state, dir.path(), "a");
        let asked = question(&state, &tab);
        let bucket = bucket(&state, &tab);
        write_finding(&state, &tab, &bucket);

        let thread = asked.clone();
        let started = super::on_a_thread(&state, "the asks", move |state| {
            let asked = crate::resources::ai_asks::ask(state, None, thread, "Explain".into());
            let validated =
                crate::resources::ai_asks::validate(state, None, None, Some("f-1".into()));
            (asked.is_ok(), validated.is_ok())
        });
        assert_eq!(started, (true, true), "an ask did not start");
        let mut waiting = pending(&state);
        waiting.sort();
        assert_eq!(waiting, ["finding:f-1".to_string(), asked.clone()]);

        release(&gate);
        wait_until("the thread answer", || !replies(&bucket, &asked).is_empty());
        wait_until("the finding answer", || !responses(&bucket).is_empty());
        wait_until("the pending replies to clear", || {
            pending(&state).is_empty()
        });
        assert_eq!(replies(&bucket, &asked)[0].0, "ai");
    });
}

/// A finding in the tab's review, read into its content.
pub(super) fn write_finding(
    state: &DesktopState,
    (key, tab): &(TabKey, TabDescriptor),
    bucket: &str,
) {
    er_engine::test_support::write_sidecar_json::<er_engine::ai::ErReview>(
        Path::new(bucket),
        serde_json::json!({
            "version": 1,
            "diff_hash": "",
            "files": {
                "src/f1.rs": {
                    "risk": "low",
                    "findings": [{
                        "id": "f-1",
                        "severity": "low",
                        "title": "A defect",
                        "description": "It breaks.",
                        "hunk_index": 0,
                        "line_start": 1
                    }]
                }
            }
        }),
    );
    let reload = er_service::Command::ReloadSidecars { key: *key };
    let reloaded = state.service.send(&actor_key(tab), reload);
    assert!(reloaded.is_ok(), "{reloaded:?}");
    wait_until("the finding in the content", || {
        state
            .service
            .content(*key, tab)
            .is_some_and(|c| c.ai.state.review.is_some())
    });
}

fn responses(bucket: &str) -> Vec<String> {
    er_engine::ai::load_ai_state(bucket, "", None)
        .review
        .iter()
        .flat_map(|review| review.files.values())
        .flat_map(|file| file.findings.iter())
        .flat_map(|finding| finding.responses.iter().map(|r| r.text.clone()))
        .collect()
}

/// An AI-assisted finding reply creates the finding's thread through the
/// workspace and shows the reply being written on it.
#[test]
fn an_ai_assisted_finding_reply_creates_the_thread() {
    with_storage(|dir| {
        let (state, gate) = asking(dir);
        let tab = open(&state, dir.path(), "a");
        let bucket = bucket(&state, &tab);
        write_finding(&state, &tab, &bucket);

        let replied = super::on_a_thread(&state, "the finding reply", |state| {
            crate::resources::ai_asks::ask_on_finding(
                state,
                None,
                None,
                "f-1".into(),
                "Is this right?".into(),
                None,
            )
        });
        assert!(replied.is_ok(), "{:?}", replied.map(drop));
        let root = finding_thread(&bucket).unwrap_or_else(|| panic!("no finding thread"));
        assert_eq!(pending(&state), std::slice::from_ref(&root));

        release(&gate);
        wait_until("the pending reply to clear", || pending(&state).is_empty());
        let rows = comments(&bucket);
        assert!(
            rows.iter()
                .any(|c| c.in_reply_to.as_deref() == Some(root.as_str()) && c.author == "ai"),
            "{rows:?}"
        );
    });
}

fn comments(bucket: &str) -> Vec<er_engine::ai::GithubComment> {
    er_engine::ai::load_ai_state(bucket, "", None)
        .github_comments
        .map(|c| c.comments)
        .unwrap_or_default()
}

fn finding_thread(bucket: &str) -> Option<String> {
    comments(bucket)
        .into_iter()
        .find(|c| c.finding_ref.as_deref() == Some("f-1") && c.in_reply_to.is_none())
        .map(|c| c.id)
}

/// An ask whose tab closes before the answer still writes the answer into
/// the bucket it asked from, and clears its pending entry.
#[test]
fn a_closed_tab_still_gets_its_answer_in_the_captured_bucket() {
    with_storage(|dir| {
        let (state, gate) = asking(dir);
        let tab = open(&state, dir.path(), "a");
        let _other = open(&state, dir.path(), "b");
        let asked = question(&state, &tab);
        let bucket = bucket(&state, &tab);
        ask(&state, tab.0, &asked);
        let closed = content_routes::close_key(&state, tab.0);
        assert!(closed.is_ok(), "{closed:?}");
        assert!(
            !state
                .service
                .app_registry()
                .tabs_view()
                .tabs
                .iter()
                .any(|t| t.key == tab.0),
            "the tab is still listed"
        );

        release(&gate);
        wait_until("the pending reply to clear", || pending(&state).is_empty());
        let answers = replies(&bucket, &asked);
        assert_eq!(answers.len(), 1, "{answers:?}");
    });
}

/// An answer lands in the bucket its ask captured: here the registry's
/// active tab moves to another tab with another bucket before it lands.
#[test]
fn a_late_answer_lands_in_the_asking_tab_after_the_focus_moves() {
    with_storage(|dir| {
        let (state, gate) = asking(dir);
        let tab = open(&state, dir.path(), "a");
        let asked = question(&state, &tab);
        ask(&state, tab.0, &asked);
        let other = open(&state, dir.path(), "b");
        let (asking, active) = (bucket(&state, &tab), bucket(&state, &other));
        assert_ne!(asking, active, "the fixture's two buckets are one");
        assert_eq!(
            state.service.app_registry().tabs_view().active,
            Some(other.0)
        );

        release(&gate);
        wait_until("the pending reply to clear", || pending(&state).is_empty());
        assert_eq!(replies(&asking, &asked).len(), 1);
        assert!(replies(&active, &asked).is_empty());
    });
}

/// An answer lands in the bucket its ask captured: here the asking tab moves
/// from the branch view to the unstaged one, whose bucket differs, before it
/// lands.
#[test]
fn a_late_answer_lands_in_the_view_the_ask_was_made_on() {
    with_storage(|dir| {
        let (state, gate) = asking(dir);
        let tab = open(&state, dir.path(), "a");
        let asked = question(&state, &tab);
        let branch = bucket(&state, &tab);
        assert!(state
            .service
            .content(tab.0, &tab.1)
            .is_some_and(|c| c.tab.mode == DiffMode::Branch));
        ask(&state, tab.0, &asked);
        let moved =
            content_routes::set_tab_mode(&state, tab.0, "unstaged", DiffMode::Unstaged, None);
        assert!(moved.is_ok(), "{moved:?}");
        wait_until("the unstaged view", || {
            state
                .service
                .content(tab.0, &tab.1)
                .is_some_and(|c| c.tab.mode == DiffMode::Unstaged)
        });
        let unstaged = bucket(&state, &tab);
        assert_ne!(branch, unstaged, "the fixture's two buckets are one");

        release(&gate);
        wait_until("the pending reply to clear", || pending(&state).is_empty());
        assert_eq!(replies(&branch, &asked).len(), 1);
        assert!(replies(&unstaged, &asked).is_empty());
    });
}

/// The asks name the App nowhere: each reads the content and the settings
/// view, so none waits on the App lock or reads its session pick.
#[test]
fn the_ask_module_names_no_app() {
    for (name, source) in [
        (
            "er-service card_asks/run.rs",
            include_str!("../../../../er-service/src/card_asks/run.rs"),
        ),
        (
            "resources/ai_asks.rs",
            include_str!("../../resources/ai_asks.rs"),
        ),
    ] {
        for app in [
            "er_engine::app::App",
            "AppState",
            ".app.lock(",
            "current_ai_",
            "sync_ai_selection",
        ] {
            assert!(!source.contains(app), "{name} names {app}");
        }
    }
}

/// The card asks are planned and run in er-service (ADR 0074): no desktop
/// module plans or runs a card job itself.
#[test]
fn no_desktop_module_plans_a_card_ask() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{e}")) {
            let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    // Built at run time, so this test's own source does not match it.
    let needle = ["card", "_job"].concat();
    let mut files = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    for file in files {
        let source = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{e}"));
        let code: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains(&needle),
            "{} plans a card ask ({needle})",
            file.display()
        );
    }
}
