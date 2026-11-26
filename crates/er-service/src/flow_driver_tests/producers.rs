//! A review flow and the app's task host share buckets (#764). A host task
//! queued or running in a bucket keeps a flow from taking its lease and from
//! letting it go, and a run cannot queue into a bucket a flow holds. A tour
//! is the one task that runs beside a flow: it writes only `tour.json`.

use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::agent_prompt::AgentPromptRun;
use er_engine::config::{AiSelection, ErConfig};
use er_engine::jobs::AgentRunHandle;
use er_engine::ports::Ports;
use er_engine::review_flow_driver::{ReviewFlowHost, ReviewFlowLease};
use er_engine::task_target::BackgroundTaskTarget;

use super::Harness;
use crate::tasks::{self, Launch, TaskHooks, TaskHost};

fn target(bucket: &str) -> BackgroundTaskTarget {
    BackgroundTaskTarget {
        repo_root: "/repo".into(),
        er_dir: bucket.into(),
        branch_label: "feature".into(),
        base_branch: "main".into(),
        scope: "branch".into(),
        pr_number: None,
        remote_repo: None,
        managed_local: true,
    }
}

/// Hooks that tell nothing, with a cap of one running task, so a task held
/// elsewhere keeps every queued one waiting and no agent ever launches.
fn hooks() -> TaskHooks {
    let mut config = ErConfig::default();
    config.ai_hub.max_concurrent_reviews = 1;
    let config = Arc::new(config);
    TaskHooks {
        ports: Ports::for_test(),
        config: Arc::new(move || Arc::clone(&config)),
        notify: Arc::new(|_, _| {}),
        finished: Arc::new(|_| {}),
    }
}

/// A launcher task named `kind` on `bucket` that runs until the answer is
/// sent on the sender this returns.
fn held(tasks: &TaskHost, bucket: &str, kind: &str) -> Sender<anyhow::Result<()>> {
    hold(tasks, bucket, kind).unwrap()
}

fn hold(tasks: &TaskHost, bucket: &str, kind: &str) -> Result<Sender<anyhow::Result<()>>, String> {
    let (done, result) = mpsc::channel();
    tasks::hold_agent(tasks, &hooks(), target(bucket), kind, |_| {
        Ok(AgentPromptRun {
            run: AgentRunHandle::new(),
            result,
        })
    })?;
    Ok(done)
}

/// Queues a run of `kind` on `bucket`. The caller holds a task elsewhere,
/// so it waits in the queue.
fn queue(tasks: &TaskHost, bucket: &str, kind: &str) -> Result<String, String> {
    let launch = Launch {
        kind: kind.into(),
        command_name: kind.into(),
        target: target(bucket),
        prompt: "Review the change".into(),
        prepared_diff: false,
        host_write_diagram: None,
        scoped_stamp: None,
        config: (hooks().config)(),
        selection: AiSelection::default(),
        quiet: true,
    };
    tasks::enqueue(tasks, &hooks(), launch)
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn finished(tasks: &TaskHost, kind: &str) -> bool {
    tasks
        .rows()
        .iter()
        .any(|row| row.kind == kind && row.status == "done")
}

/// Another bucket beside `bucket`, for the task that fills the cap.
fn elsewhere(bucket: &str) -> String {
    format!("{bucket}-elsewhere")
}

/// A test storage root with one bucket in it.
fn in_bucket(test: impl FnOnce(&str)) {
    let root = tempfile::tempdir().unwrap();
    let bucket = root.path().join("bucket").to_string_lossy().into_owned();
    er_engine::storage::with_test_storage_root(root.path(), || test(&bucket));
}

#[test]
fn a_host_task_running_in_the_bucket_refuses_the_flows_lease() {
    in_bucket(|bucket| {
        let tasks = TaskHost::default();
        let harness = Harness::with_tasks(Ports::cli(), tasks.clone());
        let done = held(&tasks, bucket, "validate");

        let refused = harness.flows().host.acquire_review_flow_lease(bucket);
        assert!(
            refused.is_err(),
            "the flow took a bucket a host task writes"
        );

        done.send(Ok(())).unwrap();
        wait_until("the host task to finish", || finished(&tasks, "validate"));
        let lease = harness.flows().host.acquire_review_flow_lease(bucket);
        assert!(lease.is_ok(), "{lease:?}");
    });
}

#[test]
fn a_host_task_queued_in_the_bucket_refuses_the_flows_lease() {
    in_bucket(|bucket| {
        let tasks = TaskHost::default();
        let harness = Harness::with_tasks(Ports::cli(), tasks.clone());
        let _elsewhere = held(&tasks, &elsewhere(bucket), "summary");
        queue(&tasks, bucket, "review").unwrap();

        let refused = harness.flows().host.acquire_review_flow_lease(bucket);
        assert!(
            refused.is_err(),
            "the flow took a bucket a run is queued for"
        );
    });
}

/// The lease is put in place by hand: with the fix no host task can start in
/// a bucket a flow holds, so this is the state a task from before the fix,
/// or one that slipped in some other way, leaves.
#[test]
fn a_host_task_running_in_the_bucket_keeps_the_flows_lease() {
    in_bucket(|bucket| {
        let tasks = TaskHost::default();
        let harness = Harness::with_tasks(Ports::cli(), tasks.clone());
        let done = held(&tasks, bucket, "summary");
        let mut flows = harness.flows();
        let host = &mut flows.host;
        let scope = er_store::Scope::dir(bucket).unwrap();
        let guard = er_engine::store_registry::store()
            .lease(&scope, Duration::ZERO, "flow-1")
            .unwrap();
        let lease = ReviewFlowLease::new(bucket.to_owned(), 1, guard.holder().clone());
        host.leases.insert(bucket.to_owned(), guard);

        assert!(
            host.release_review_flow_lease(&lease).is_err(),
            "the flow let its lease go while a host task still writes"
        );
        done.send(Ok(())).unwrap();
        wait_until("the host task to finish", || finished(&tasks, "summary"));
        assert!(host.release_review_flow_lease(&lease).is_ok());
    });
}

#[test]
fn a_run_cannot_queue_into_a_bucket_a_flow_holds() {
    in_bucket(|bucket| {
        let tasks = TaskHost::default();
        let harness = Harness::with_tasks(Ports::cli(), tasks.clone());
        let _elsewhere = held(&tasks, &elsewhere(bucket), "summary");
        let _lease = harness
            .flows()
            .host
            .acquire_review_flow_lease(bucket)
            .unwrap();

        let queued = queue(&tasks, bucket, "review");
        assert!(
            queued.is_err(),
            "a run queued under a flow's lease: {queued:?}"
        );
    });
}

#[test]
fn a_tour_does_not_refuse_the_flows_lease() {
    in_bucket(|bucket| {
        let tasks = TaskHost::default();
        let harness = Harness::with_tasks(Ports::cli(), tasks.clone());
        let _tour = held(&tasks, bucket, tasks::TOUR_TASK_KIND);

        let lease = harness.flows().host.acquire_review_flow_lease(bucket);
        assert!(lease.is_ok(), "{lease:?}");
    });
}

#[test]
fn a_tour_queues_beside_a_flow_and_does_not_keep_its_lease() {
    in_bucket(|bucket| {
        let tasks = TaskHost::default();
        let harness = Harness::with_tasks(Ports::cli(), tasks.clone());
        let _elsewhere = held(&tasks, &elsewhere(bucket), "summary");
        let mut flows = harness.flows();
        let host = &mut flows.host;
        let lease = host.acquire_review_flow_lease(bucket).unwrap();

        let queued = queue(&tasks, bucket, tasks::TOUR_TASK_KIND);
        assert!(queued.is_ok(), "{queued:?}");
        assert!(host.release_review_flow_lease(&lease).is_ok());
    });
}

/// Summary, Questions, Notes, the named commands and the desktop's send-note
/// all start through `hold_agent`.
#[test]
fn a_tab_command_cannot_start_in_a_bucket_a_flow_holds() {
    in_bucket(|bucket| {
        let tasks = TaskHost::default();
        let harness = Harness::with_tasks(Ports::cli(), tasks.clone());
        let lease = harness
            .flows()
            .host
            .acquire_review_flow_lease(bucket)
            .unwrap();

        let refused = hold(&tasks, bucket, "summary").map(drop);
        assert_eq!(
            refused,
            Err("A review flow is already using this review bucket".to_string())
        );

        harness
            .flows()
            .host
            .release_review_flow_lease(&lease)
            .unwrap();
        let done = hold(&tasks, bucket, "summary").unwrap();
        done.send(Ok(())).unwrap();
    });
}

/// A flow that could not release while a host task wrote is advanced when
/// that task ends, with no stage of its own left to tick it.
#[test]
fn a_host_task_that_ends_wakes_the_flows() {
    in_bucket(|bucket| {
        let tasks = TaskHost::default();
        let harness = Harness::with_tasks(Ports::cli(), tasks.clone());
        let done = held(&tasks, bucket, "summary");
        while harness.actor_message().is_some() {}

        done.send(Ok(())).unwrap();
        wait_until("the host task to finish", || finished(&tasks, "summary"));
        let mut woken = false;
        wait_until("the flows to be woken", || {
            woken |= std::iter::from_fn(|| harness.actor_message())
                .any(|message| matches!(message, crate::handle::ActorMsg::TasksChanged));
            woken
        });
    });
}
