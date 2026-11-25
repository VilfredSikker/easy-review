//! The review-flow driver's behaviour on the service host: the stage
//! sequence, leases and cancellation, re-runs, recovery on load, the results
//! projection and guarded acceptance. These ran on the App until P77; each
//! test here drives `ReviewFlows` through a real [`ServiceFlowHost`], with no
//! actor around it, so a test can reach the host's leases, stage tasks and
//! arena controls. Stage jobs run on the stage's worker (`LocalJobSink`), the
//! way the actor's sink runs them on an effect thread.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::Result;
use er_engine::ai::review_flow::ReviewFlow;
use er_engine::arena::ArenaRunControls;
use er_engine::config::ErConfig;
use er_engine::jobs::{JobSink, LocalJobSink};
use er_engine::ports::Ports;
use er_engine::review_flow_driver::ReviewFlowInputs;

use super::{FlowRunner, Flows};
use crate::actor::WorkspaceDeps;
use crate::handle::ActorMsg;
use crate::protocol::FlowContext;

mod driver;
mod producers;
mod record;
mod results;
mod views;

/// A flow runner on its own host, whose actor messages nobody reads.
pub(super) struct Harness {
    runner: FlowRunner,
    _actor: Receiver<ActorMsg>,
}

impl Harness {
    pub(super) fn new(ports: Ports) -> Self {
        Self::with_deps(WorkspaceDeps::new(ports))
    }

    /// A runner whose flows see `tasks`, the app's task host.
    pub(super) fn with_tasks(ports: Ports, tasks: crate::tasks::TaskHost) -> Self {
        Self::with_deps(WorkspaceDeps::new(ports).with_tasks(tasks))
    }

    fn with_deps(deps: WorkspaceDeps) -> Self {
        let (tx, actor) = mpsc::channel();
        let sink: Arc<dyn JobSink> = Arc::new(LocalJobSink);
        let arena = ArenaRunControls::with_jobs(Arc::clone(&sink));
        let runner = FlowRunner::new(&deps, sink, arena, &tx);
        Self {
            runner,
            _actor: actor,
        }
    }

    /// The flows and their host, as an effect holds them.
    pub(super) fn flows(&self) -> MutexGuard<'_, Flows> {
        self.runner.lock()
    }

    /// The load-time recovery of `buckets`.
    pub(super) fn reconcile(&self, buckets: &[String]) -> crate::protocol::Reply {
        self.runner.reconcile(buckets)
    }

    /// The settings and the view the next command carries.
    pub(super) fn set_context(&self, config: &ErConfig, view: ReviewFlowInputs) {
        self.flows().host.take_context(FlowContext {
            config: Arc::new(config.clone()),
            effort: None,
            view: Ok(view),
        });
    }

    pub(super) fn start(&self, scope: &str) -> Result<()> {
        let mut guard = self.flows();
        let Flows { flows, host } = &mut *guard;
        flows.start_review_flow(host, scope)
    }

    pub(super) fn confirm(&self, er_dir: &str) -> Result<()> {
        let mut guard = self.flows();
        let Flows { flows, host } = &mut *guard;
        flows.confirm_review_flow(host, er_dir)
    }

    pub(super) fn rerun(&self, er_dir: &str) -> Result<()> {
        let mut guard = self.flows();
        let Flows { flows, host } = &mut *guard;
        flows.rerun_review_flow(host, er_dir)
    }

    pub(super) fn cancel(&self, er_dir: &str) -> bool {
        let mut guard = self.flows();
        let Flows { flows, host } = &mut *guard;
        flows.cancel_review_flow(host, er_dir)
    }

    /// One advance of every flow, as a stage worker's wake-up queues.
    pub(super) fn advance(&self) {
        let mut guard = self.flows();
        let Flows { flows, host } = &mut *guard;
        flows.advance_review_flows(host);
    }

    pub(super) fn flow(&self, er_dir: &str) -> ReviewFlow {
        self.flows()
            .flows
            .review_flow_for(er_dir)
            .cloned()
            .expect("the flow is shown")
    }

    pub(super) fn is_draining(&self, er_dir: &str) -> bool {
        self.flows().flows.review_flow_is_draining(er_dir)
    }

    /// Advances until `done` holds for the flow, failing after `limit`.
    pub(super) fn advance_until(
        &self,
        er_dir: &str,
        limit: Duration,
        what: &str,
        done: impl Fn(&ReviewFlow) -> bool,
    ) {
        let deadline = Instant::now() + limit;
        loop {
            self.advance();
            if done(&self.flow(er_dir)) {
                return;
            }
            assert!(Instant::now() < deadline, "{what}: {:?}", self.flow(er_dir));
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Advances until no cancelled writer still runs in the bucket.
    pub(super) fn drain(&self, er_dir: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while self.is_draining(er_dir) {
            self.advance();
            assert!(Instant::now() < deadline, "cancelled writer did not drain");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn run_until_terminal(&self, er_dir: &str) {
        self.advance_until(
            er_dir,
            Duration::from_secs(30),
            "review flow did not reach a terminal state",
            |flow| !flow.is_running(),
        );
    }
}
