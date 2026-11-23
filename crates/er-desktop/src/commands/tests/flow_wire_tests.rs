//! The review flows on `er_send` and `jobs`: a control names the tab and
//! the bucket the page painted and runs on that tab's actor, and the flows
//! reach the page through `jobs`.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::ai::review_flow::FlowState;
use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_protocol::{Ack, Command, ErError, Resource, ResourceKey, ReviewFlowAction, ReviewFlowJob};
use er_service::{LiveReviewFlow, WorkspaceDeps};

use super::arena_resource_tests::{local_ports, open, wait_until, HoldingSink};
use super::diff_command_tests::send;
use super::diff_resource_tests::with_storage;
use super::flow_resource_tests::{flow_state, state_with};
use crate::resources::Watched;
use crate::service::actor_key;
use crate::state::DesktopState;

fn jobs(state: &DesktopState) -> (u64, Vec<ReviewFlowJob>) {
    match crate::resources::fetch(state, ResourceKey::Jobs) {
        Ok(fetched) => match fetched.value {
            Resource::Jobs(jobs) => (fetched.version.0, jobs.review_flows),
            other => panic!("not jobs: {other:?}"),
        },
        Err(e) => panic!("{e:?}"),
    }
}

/// The bucket `diff(tab)` paints for the tab, which the page sends back.
fn painted_bucket(state: &DesktopState, tab: TabKey) -> String {
    let key = ResourceKey::Diff {
        tab: er_service::wire_tab_key(tab),
    };
    wait_until("the tab's diff", || {
        matches!(
            crate::resources::fetch(state, key.clone()).map(|f| f.value),
            Ok(Resource::Diff(ref diff)) if !diff.review_bucket_er_dir.is_empty()
        )
    });
    match crate::resources::fetch(state, key).map(|f| f.value) {
        Ok(Resource::Diff(diff)) => diff.review_bucket_er_dir,
        other => panic!("not a diff: {other:?}"),
    }
}

/// Starts a review of the branch view on `tab` through `er_send`.
fn start(state: &DesktopState, tab: TabKey, er_dir: String) -> Result<Ack, ErError> {
    let command = Command::ReviewFlow {
        tab: er_service::wire_tab_key(tab),
        er_dir,
        action: ReviewFlowAction::Start {
            scope: "current".into(),
        },
    };
    send(state, "flow start", command)
}

/// The registry moved to B while the page shows A: an `er_send` start keyed
/// to A, on the bucket `diff(tab)` painted for A, runs on A's actor, and
/// `jobs` lists the flow in that bucket.
#[test]
fn an_er_send_flow_start_runs_on_the_tab_it_names() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = flow_state(dir, local_ports(), Arc::clone(&sink));
        let painted = open(&state, dir.path(), "a");
        let active = open(&state, dir.path(), "b");
        let er_dir = painted_bucket(&state, painted.0);

        let started = start(&state, painted.0, er_dir.clone());
        let ack = started.unwrap_or_else(|e| panic!("{e:?}"));

        assert!(ack
            .changed
            .iter()
            .any(|changed| changed.key == ResourceKey::Jobs));
        let flow_actor = state.service.published_view(&actor_key(&painted.1));
        assert!(
            flow_actor.is_some_and(|view| view.review_flow(&er_dir).is_some()),
            "no flow on the named tab's actor"
        );
        let other = state.service.published_view(&actor_key(&active.1));
        assert!(
            other.is_none_or(|view| view.review_flows.is_empty()),
            "the flow went to the active tab's actor"
        );
        let (_, flows) = jobs(&state);
        assert!(
            flows.iter().any(|flow| flow.er_dir == er_dir),
            "jobs does not list the flow in its bucket: {flows:?}"
        );
        sink.release();
    });
}

/// A flow's start moves `jobs`, which lists the flow as the snapshot the
/// page draws, and the relay names `jobs`.
#[test]
fn a_review_flow_moves_jobs_and_the_relay_names_it() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = flow_state(dir, local_ports(), Arc::clone(&sink));
        let tab = open(&state, dir.path(), "a");
        let er_dir = painted_bucket(&state, tab.0);
        let (before, _) = jobs(&state);
        let mut last = crate::resources::watched(&state);

        let started = start(&state, tab.0, er_dir.clone());
        assert!(started.is_ok(), "{started:?}");
        wait_until("the flow to show in jobs", || {
            jobs(&state).1.iter().any(|flow| flow.er_dir == er_dir)
        });

        let (moved_to, flows) = jobs(&state);
        assert!(moved_to > before, "the flow left jobs at {before}");
        let flow = flows.iter().find(|flow| flow.er_dir == er_dir);
        let snapshot: serde_json::Value =
            serde_json::from_str(&flow.unwrap_or_else(|| panic!("no flow")).json)
                .unwrap_or_else(|e| panic!("{e}"));
        assert!(snapshot["state"].is_string(), "{snapshot}");
        let now = crate::resources::watched(&state);
        let moved = crate::resources::relay::moved(&mut last, now);
        assert!(
            moved.iter().any(|(key, _)| *key == Watched::Jobs),
            "{moved:?}"
        );
        sink.release();
    });
}

/// Two lists that differ only in a flow's tasks, its state the same, move
/// the count; the same list read again does not.
#[test]
fn live_jobs_counts_a_change_of_a_flow_alone() {
    let flow = |task: &str| ReviewFlowJob {
        er_dir: "/bucket".into(),
        json: format!(r#"{{"state":"finding","tasks":[{{"id":"{task}","status":"running"}}]}}"#),
    };
    let live = crate::state::LiveJobs::default();
    let first = live.observe_flows(&[flow("finding:security")]);
    let ticked = live.observe_flows(&[flow("finding:tests")]);
    assert!(ticked > first, "a flow's change left the count at {first}");
    assert_eq!(
        live.observe_flows(&[flow("finding:tests")]),
        ticked,
        "the same flows moved the count"
    );
}

/// The flow wire names the App nowhere outside its tests.
#[test]
fn the_flow_wire_names_no_app() {
    let source = include_str!("../../resources/flows.rs");
    let body = source.split("#[cfg(test)]").next().unwrap_or(source);
    for app in ["er_engine::app", "AppState", ".app.lock(", "state.app"] {
        assert!(!body.contains(app), "resources/flows.rs names {app}");
    }
}

/// Each action names its own control, and acceptance carries its run and
/// findings.
#[test]
fn each_flow_action_names_its_own_control() {
    use crate::flow_service::Control;
    use crate::resources::flows::{step, Step};

    let cases = [
        (
            ReviewFlowAction::Start {
                scope: "current".into(),
            },
            Step::Control(Control::Start("current".into())),
        ),
        (ReviewFlowAction::Confirm, Step::Control(Control::Confirm)),
        (ReviewFlowAction::Decline, Step::Control(Control::Decline)),
        (ReviewFlowAction::Rerun, Step::Control(Control::Rerun)),
        (ReviewFlowAction::Cancel, Step::Control(Control::Cancel)),
        (
            ReviewFlowAction::RunStandard,
            Step::Control(Control::RunStandard),
        ),
        (
            ReviewFlowAction::AcceptFindings {
                run_id: "run-1".into(),
                finding_ids: vec!["f-1".into()],
            },
            Step::Accept {
                run_id: "run-1".into(),
                finding_ids: vec!["f-1".into()],
            },
        ),
    ];
    for (action, expected) in cases {
        let named = format!("{action:?}");
        assert_eq!(step(action), expected, "{named}");
    }
}

/// Two fake providers whose jobs run: triage picks Deep, and each reviewer
/// and the arbiter answer at once with no findings.
fn deep_config() -> String {
    let script = [
        "case \"$1\" in".to_owned(),
        format!(
            "*triage.json*) {}; printf '{{\"version\":1,\"diff_hash\":\"%s\",\"verdict\":{{\"primary\":\"arena\",\"experts\":[\"security\"]}}}}' \"$h\" > \"$out/triage.json\" ;;",
            er_engine::test_support::STAGING_PREAMBLE
        ),
        r#"*) printf '{"findings":[],"ballots":[],"verdicts":[]}' ;;"#.to_owned(),
        "esac".to_owned(),
    ]
    .join("\n");
    let provider = |id: &str| {
        format!(
            "[ai_hub.providers.{id}]\ncommand = \"/bin/sh\"\nargs = [\"-c\", '''{script}''', \"fake-flow\", \"{{prompt}}\"]\n\n[[ai_hub.providers.{id}.models]]\nid = \"fake-model\"\n"
        )
    };
    let model = |id: &str| format!("{{ provider_id = \"{id}\", model_id = \"fake-model\" }}");
    format!(
        "[ai_hub]\ndefault_provider = \"fake\"\nmax_concurrent_reviews = 16\nmax_concurrent_agents = 16\n\n{}\n{}\n[ai_hub.review_models]\nfast = [{fake}]\nstandard = [{fake}]\nadvanced = [{fake}, {second}]\n",
        provider("fake"),
        provider("fake-second"),
        fake = model("fake"),
        second = model("fake-second"),
    )
}

/// Waits until the flow in `er_dir` on `tab`'s actor is `done`; a Deep
/// flow runs triage, two reviewers over its rounds and an arbiter as real
/// processes.
fn flow_until(
    state: &DesktopState,
    tab: &TabDescriptor,
    er_dir: &str,
    what: &str,
    done: impl Fn(&LiveReviewFlow) -> bool,
) -> LiveReviewFlow {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let view = state.service.published_view(&actor_key(tab));
        if let Some(live) = view.as_ref().and_then(|view| view.review_flow(er_dir)) {
            if done(live) {
                return live.clone();
            }
        }
        assert!(Instant::now() < deadline, "{what} within 60s");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Gives a finished run one kept finding, so acceptance has one to take.
fn add_kept_finding(er_dir: &str, run_id: &str) {
    let paths = er_engine::arena::ArenaPaths::for_run(Path::new(er_dir), run_id);
    let mut run = er_engine::arena::load_run(&paths).unwrap_or_else(|e| panic!("{e:#}"));
    let finding = serde_json::json!({
        "id": "kept", "file": "src/f0.rs", "line": 1, "title": "A kept finding",
        "body": "body", "severity_by_round": {}, "raised_by": [], "verdict": "kept",
        "confidence": 0.9, "rationale": "", "rounds": []
    });
    run.findings
        .push(serde_json::from_value(finding).unwrap_or_else(|e| panic!("{e}")));
    er_engine::arena::save_run(&paths, &run).unwrap_or_else(|e| panic!("{e:#}"));
}

/// A Deep flow started and confirmed through `er_send` on tab A reaches its
/// arena run; `review_flow_run`, keyed to tab B, answers that run from A's
/// actor; and an acceptance through `er_send` answers its count in
/// `Ack.created`.
#[test]
fn a_deep_flow_run_is_read_on_er_fetch_from_another_tab_and_accepted_on_er_send() {
    with_storage(|dir| {
        let ports = local_ports();
        let state = state_with(
            dir,
            ports.clone(),
            WorkspaceDeps::new(ports),
            &deep_config(),
        );
        let flow_tab = open(&state, dir.path(), "a");
        let er_dir = painted_bucket(&state, flow_tab.0);
        let other = open(&state, dir.path(), "b");
        let control = |action: ReviewFlowAction| {
            let command = Command::ReviewFlow {
                tab: er_service::wire_tab_key(flow_tab.0),
                er_dir: er_dir.clone(),
                action,
            };
            send(&state, "flow control", command)
        };

        let started = control(ReviewFlowAction::Start {
            scope: "current".into(),
        });
        assert!(started.is_ok(), "{started:?}");
        flow_until(
            &state,
            &flow_tab.1,
            &er_dir,
            "triage asks for confirmation",
            |live| matches!(live.flow.state, FlowState::AwaitingConfirmation),
        );
        let confirmed = control(ReviewFlowAction::Confirm);
        assert!(confirmed.is_ok(), "{confirmed:?}");
        let finished = flow_until(
            &state,
            &flow_tab.1,
            &er_dir,
            "the Deep flow finishes",
            |live| matches!(live.flow.state, FlowState::Done { .. }) && !live.draining,
        );
        let run_id = finished
            .flow
            .arena_run_id
            .unwrap_or_else(|| panic!("the flow names no run"));

        let key = ResourceKey::ReviewFlowRun {
            tab: er_service::wire_tab_key(other.0),
            er_dir: er_dir.clone(),
            run_id: run_id.clone(),
        };
        let fetched = crate::resources::fetch(&state, key).unwrap_or_else(|e| panic!("{e:?}"));
        let Resource::ReviewFlowRun(run) = fetched.value else {
            panic!("not a flow run: {:?}", fetched.value);
        };
        let snapshot: serde_json::Value =
            serde_json::from_str(&run.json).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(snapshot["run"]["id"], run_id.as_str(), "{snapshot}");

        add_kept_finding(&er_dir, &run_id);
        let accepted = control(ReviewFlowAction::AcceptFindings {
            run_id,
            finding_ids: vec!["kept".into()],
        });
        let ack = accepted.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(ack.created.as_deref(), Some("1"), "{ack:?}");
    });
}
