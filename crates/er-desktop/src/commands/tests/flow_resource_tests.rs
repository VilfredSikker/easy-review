//! The review flows with no App: a flow command names its tab and runs on
//! that tab's actor with the App locked, and its context comes from the
//! tab's content and the settings actor's view. Startup recovery reads the
//! registry's list before any tab has loaded.

use std::path::Path;
use std::sync::Arc;

use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;
use er_protocol::{Command, ErError};
use er_service::{AppCommand, WorkspaceDeps};

use super::arena_resource_tests::{bucket, local_ports, open, wait_until, HoldingSink};
use super::diff_command_tests::send;
use super::diff_resource_tests::{listed, with_storage};
use super::{review_flow_test_state, ReviewFlowTestDir};
use crate::commands::{content_routes, AppState};
use crate::flow_service::{self, Control};
use crate::service::{actor_key, ServiceState};

/// A provider on the path in every review-flow group, so a start resolves
/// its models; its jobs go to the test's sink.
const FLOW_CONFIG: &str = r#"
[ai_hub]
default_provider = "fake"

[ai_hub.providers.fake]
command = "/bin/sh"
args = ["-c", "exit 1", "fake", "{prompt}"]

[[ai_hub.providers.fake.models]]
id = "fake-1"
effort_levels = ["low", "high"]

[ai_hub.review_models]
fast = [{ provider_id = "fake", model_id = "fake-1" }]
standard = [{ provider_id = "fake", model_id = "fake-1" }]
advanced = [{ provider_id = "fake", model_id = "fake-1" }]
"#;

/// A state whose actors hand their jobs to `sink` and whose settings actor
/// reads the flow config.
pub(super) fn flow_state(
    dir: &ReviewFlowTestDir,
    ports: er_engine::ports::Ports,
    sink: Arc<HoldingSink>,
) -> AppState {
    let config = dir.path().join("settings").join("config.toml");
    std::fs::create_dir_all(config.parent().unwrap_or_else(|| Path::new("/")))
        .unwrap_or_else(|e| panic!("settings dir: {e}"));
    std::fs::write(&config, FLOW_CONFIG).unwrap_or_else(|e| panic!("config: {e}"));
    let mut state = review_flow_test_state(dir);
    state.desktop.ports = ports.clone();
    let deps = WorkspaceDeps::new(ports).with_jobs(sink);
    state.service = Arc::new(ServiceState::with_deps(deps, |_| {}).with_config_path(config));
    state
}

/// A Deep flow's record whose finding stage was running when its owner
/// died, so recovery shows it interrupted.
fn write_interrupted_deep(bucket: &str) {
    let bucket = Path::new(bucket);
    std::fs::create_dir_all(bucket).unwrap_or_else(|e| panic!("bucket: {e}"));
    let record = serde_json::json!({
        "version": 1, "diff_hash": "hash", "started_at": "t0",
        "holder": { "pid": 1, "label": "flow-1" },
        "kind": "deep",
        "stages": [
            { "id": "triage", "stage": "triage", "label": "Triage",
              "status": "complete", "diff_hash": "hash" },
            { "id": "finding:security", "stage": "finding", "label": "Security",
              "status": "running", "diff_hash": "hash" }
        ]
    });
    er_engine::test_support::write_sidecar_json::<er_engine::ai::workflow_record::WorkflowRecord>(
        bucket, record,
    );
    std::fs::write(bucket.join(".diff-tmp.sha256"), "hash")
        .unwrap_or_else(|e| panic!("prepared diff hash: {e}"));
}

fn descriptor_bucket(state: &AppState, tab: &er_engine::tabs_record::TabDescriptor) -> String {
    er_service::descriptor_bucket(&*state.desktop.ports.git, tab).unwrap_or_else(|e| panic!("{e}"))
}

fn recovered(state: &AppState, tab: &er_engine::tabs_record::TabDescriptor, bucket: &str) -> bool {
    state
        .service
        .published_view(&actor_key(tab))
        .is_some_and(|view| view.review_flow(bucket).is_some())
}

/// Starts a review of the branch view on `tab`, painted on `er_dir`, with
/// the App locked.
fn start(state: &AppState, tab: TabKey, er_dir: String) -> Result<(), ErError> {
    super::while_the_app_is_locked(state, "flow start", move |state| {
        let wire = er_service::wire_tab_key(tab);
        let tab = crate::arena_service::tab(&state.desktop, Some(wire))?;
        flow_service::run(
            &state.desktop,
            &tab,
            er_dir,
            Control::Start("branch".into()),
        )
    })
}

/// The registry moved to B while the page shows A: a start keyed to A, run
/// with the App locked, runs on A's actor over A's bucket, and a start that
/// names a bucket A's content no longer shows is refused.
#[test]
fn a_flow_start_runs_on_the_tab_it_names_with_the_app_locked() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = flow_state(dir, local_ports(), Arc::clone(&sink));
        let painted = open(&state, dir.path(), "a");
        let active = open(&state, dir.path(), "b");
        let (painted_bucket, active_bucket) = (bucket(&state, &painted), bucket(&state, &active));
        assert_ne!(
            actor_key(&painted.1),
            actor_key(&active.1),
            "the fixture's two tabs share an actor"
        );

        let stale = start(&state, painted.0, active_bucket);
        assert!(stale.is_err(), "a start on a stale bucket ran: {stale:?}");
        let started = start(&state, painted.0, painted_bucket.clone());
        assert!(started.is_ok(), "{started:?}");

        assert!(
            recovered(&state, &painted.1, &painted_bucket),
            "no flow on the named tab's actor"
        );
        let other = state.service.published_view(&actor_key(&active.1));
        assert!(
            other.is_none_or(|view| view.review_flows.is_empty()),
            "the flow went to the active tab's actor"
        );
        sink.release();
    });
}

/// With the App locked, a flow's context takes the effort `SetAiEffort`
/// just saved through the settings actor, and the view of the tab it names.
#[test]
fn a_flow_context_takes_the_saved_effort_with_the_app_locked() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = flow_state(dir, local_ports(), Arc::clone(&sink));
        let tab = open(&state, dir.path(), "a");
        let er_dir = bucket(&state, &tab);
        let pick = Command::SetAiEffort {
            effort: Some("high".into()),
        };
        let saved = send(&state, "set_ai_effort", pick);
        assert!(saved.is_ok(), "{saved:?}");

        let wire = er_service::wire_tab_key(tab.0);
        let context = super::while_the_app_is_locked(&state, "flow context", move |state| {
            let tab = crate::arena_service::tab(&state.desktop, Some(wire))?;
            Ok::<_, ErError>(flow_service::context(&state.desktop, &tab, Some("branch")))
        })
        .unwrap_or_else(|e| panic!("{e:?}"));

        assert_eq!(context.effort.as_deref(), Some("high"));
        let view = context.view.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(view.target.er_dir, er_dir);
        sink.release();
    });
}

/// A remote PR tab's flows run on its `remote:` actor, never on the actor
/// of the directory its content roots at: startup recovery over the
/// registry's list recovers its flow there.
#[test]
fn a_review_flow_on_a_remote_pr_tab_keys_through_tabs() {
    with_storage(|dir| {
        let state = super::tab_key_tests::state(dir);
        let ports = super::tab_key_tests::ports();
        let (_, remote) = super::tab_key_tests::kinds(dir)
            .into_iter()
            .find(|(kind, _)| *kind == "remote PR")
            .unwrap_or_else(|| panic!("no remote PR kind"));
        let added = state.service.send_app(AppCommand::NewTab {
            tab: remote.clone(),
        });
        assert!(added.is_ok(), "{added:?}");
        let resolved = er_engine::tab_identity::resolve_descriptor(&*ports.git, &remote)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(
            actor_key(&remote),
            resolved.repo_root,
            "the fixture keys one actor"
        );
        let er_dir =
            er_service::descriptor_bucket(&*ports.git, &remote).unwrap_or_else(|e| panic!("{e}"));
        write_interrupted_deep(&er_dir);

        let tabs = state.service.app_registry().tabs_view();
        flow_service::reconcile_on_load(&tabs, &ports, &state.service);

        wait_until("the remote PR's actor recovers its flow", || {
            recovered(&state, &remote, &er_dir)
        });
    });
}

/// At startup no tab has loaded, and the App, which a restore has already
/// filled, is locked: recovery still finds each listed tab's flow, in the
/// bucket its descriptor resolves to.
#[test]
fn startup_recovery_recovers_each_listed_unloaded_tab_with_the_app_locked() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = flow_state(dir, local_ports(), Arc::clone(&sink));
        let tabs = [listed(&state, dir, "a"), listed(&state, dir, "b")];
        let buckets = tabs.clone().map(|(_, tab)| descriptor_bucket(&state, &tab));
        assert_ne!(buckets[0], buckets[1], "the fixture's two buckets are one");
        for bucket in &buckets {
            write_interrupted_deep(bucket);
        }
        for (key, tab) in &tabs {
            assert!(
                state.service.content(*key, tab).is_none(),
                "a tab has loaded"
            );
        }

        super::while_the_app_is_locked(&state, "startup recovery", |state| {
            let tabs = state.service.app_registry().tabs_view();
            flow_service::reconcile_on_load(&tabs, &state.desktop.ports, &state.service);
        });

        for ((_, tab), bucket) in tabs.iter().zip(&buckets) {
            wait_until("each listed tab's flow is recovered", || {
                recovered(&state, tab, bucket)
            });
        }
        sink.release();
    });
}

/// A loaded tab that left the mode it was restored in shows its flows in its
/// content's bucket: a flow on that bucket is found on its actor while
/// another project's tab is active.
#[test]
fn a_flow_on_a_tab_moved_off_its_restore_mode_keys_by_its_content_bucket() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = flow_state(dir, local_ports(), Arc::clone(&sink));
        let moved = open(&state, dir.path(), "a");
        let staged =
            content_routes::set_tab_mode(&state.desktop, moved.0, "staged", DiffMode::Staged, None);
        assert!(staged.is_ok(), "{staged:?}");
        wait_until("the staged view to load", || {
            state
                .service
                .content(moved.0, &moved.1)
                .is_some_and(|c| c.tab.mode == DiffMode::Staged)
        });
        let content_bucket = bucket(&state, &moved);
        assert_ne!(
            content_bucket,
            descriptor_bucket(&state, &moved.1),
            "the fixture's two buckets are one"
        );
        let other = open(&state, dir.path(), "b");
        let active = flow_service::entry(&state.desktop, None).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(active.key, other.0);

        let key = flow_service::flow_key(&state.desktop, &content_bucket, &active);

        assert_eq!(key, actor_key(&moved.1));
        sink.release();
    });
}

/// A run is read and accepted in the actor that runs its flow after the user
/// switched to a tab of another project, whose actor has no such flow; and
/// still once the flow's own tab is closed.
#[test]
fn a_flow_command_finds_the_flow_actor_after_a_tab_switch() {
    with_storage(|dir| {
        let sink = Arc::new(HoldingSink::default());
        let state = flow_state(dir, local_ports(), Arc::clone(&sink));
        let flow_tab = listed(&state, dir, "a");
        let other = listed(&state, dir, "b");
        let flow_bucket = descriptor_bucket(&state, &flow_tab.1);
        let active =
            || flow_service::entry(&state.desktop, None).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(active().key, other.0);

        // No actor has published the flow yet: the listed tab on the bucket
        // names the actor.
        let key = flow_service::flow_key(&state.desktop, &flow_bucket, &active());
        assert_eq!(key, actor_key(&flow_tab.1));

        write_interrupted_deep(&flow_bucket);
        let tabs = state.service.app_registry().tabs_view();
        flow_service::reconcile_on_load(&tabs, &state.desktop.ports, &state.service);
        wait_until("the recovered flow is published", || {
            recovered(&state, &flow_tab.1, &flow_bucket)
        });
        // The other project's actor is up and holds no flow on the bucket.
        assert!(state
            .service
            .view(&actor_key(&other.1))
            .review_flow(&flow_bucket)
            .is_none());

        // With the flow's tab closed, the published view still names it.
        let closed = state
            .service
            .send_app(AppCommand::CloseTab { key: flow_tab.0 });
        assert!(closed.is_ok(), "{closed:?}");
        let key = flow_service::flow_key(&state.desktop, &flow_bucket, &active());
        assert_eq!(key, actor_key(&flow_tab.1));
        sink.release();
    });
}

/// `flow_service.rs` names the App nowhere outside its tests: every command
/// and read builds its context from the tab's content and the settings view.
#[test]
fn the_flow_module_names_no_app() {
    for (name, source) in [("flow_service.rs", include_str!("../../flow_service.rs"))] {
        let body = source.split("#[cfg(test)]").next().unwrap_or(source);
        for app in ["er_engine::app", "AppState", ".app.lock(", "state.app"] {
            assert!(!body.contains(app), "{name} names {app}");
        }
    }
}
