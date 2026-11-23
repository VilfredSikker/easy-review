//! The review flows on `er_send` and `jobs`: a control names the tab and
//! the bucket the page painted and runs on that tab's actor with the App
//! locked, and the flows reach the page through `jobs`.

use std::sync::Arc;

use er_engine::TabKey;
use er_protocol::{Ack, Command, ErError, Resource, ResourceKey, ReviewFlowAction, ReviewFlowJob};

use super::arena_resource_tests::{local_ports, open, wait_until, HoldingSink};
use super::diff_command_tests::send;
use super::diff_resource_tests::with_storage;
use super::flow_resource_tests::flow_state;
use crate::commands::AppState;
use crate::resources::Watched;
use crate::service::actor_key;

fn jobs(state: &AppState) -> (u64, Vec<ReviewFlowJob>) {
    match crate::resources::fetch(&state.desktop, ResourceKey::Jobs) {
        Ok(fetched) => match fetched.value {
            Resource::Jobs(jobs) => (fetched.version.0, jobs.review_flows),
            other => panic!("not jobs: {other:?}"),
        },
        Err(e) => panic!("{e:?}"),
    }
}

/// The bucket `diff(tab)` paints for the tab, which the page sends back.
fn painted_bucket(state: &AppState, tab: TabKey) -> String {
    let key = ResourceKey::Diff {
        tab: er_service::wire_tab_key(tab),
    };
    wait_until("the tab's diff", || {
        matches!(
            crate::resources::fetch(&state.desktop, key.clone()).map(|f| f.value),
            Ok(Resource::Diff(ref diff)) if !diff.review_bucket_er_dir.is_empty()
        )
    });
    match crate::resources::fetch(&state.desktop, key).map(|f| f.value) {
        Ok(Resource::Diff(diff)) => diff.review_bucket_er_dir,
        other => panic!("not a diff: {other:?}"),
    }
}

/// Starts a review of the branch view on `tab` through `er_send`; `send`
/// holds the App locked.
fn start(state: &AppState, tab: TabKey, er_dir: String) -> Result<Ack, ErError> {
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
/// to A, on the bucket `diff(tab)` painted for A, runs on A's actor with
/// the App locked, and `jobs` lists the flow in that bucket.
#[test]
fn an_er_send_flow_start_runs_on_the_tab_it_names_with_the_app_locked() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = flow_state(dir, local_ports(), Arc::clone(&sink));
        let painted = open(&state, dir.path(), "a");
        let active = open(&state, dir.path(), "b");
        let er_dir = painted_bucket(&state, painted.0);

        let started = start(&state, painted.0, er_dir.clone());
        let ack = started.unwrap_or_else(|e| panic!("{e:?}"));

        assert!(ack.changed.iter().any(|changed| changed.key == ResourceKey::Jobs));
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
        let mut last = crate::resources::watched(&state.desktop);

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
        let now = crate::resources::watched(&state.desktop);
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
