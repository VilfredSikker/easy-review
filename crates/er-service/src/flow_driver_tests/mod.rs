//! The review-flow driver's behaviour on the service host: recovery on load,
//! the results projection and guarded acceptance. These ran on the App until
//! P77; each test here drives `ReviewFlows` through a real
//! [`ServiceFlowHost`], with no actor around it, so a test can reach the
//! host's leases and arena controls. Stage jobs run on the submitting worker
//! (`LocalJobSink`), the way the actor's sink runs them on an effect thread.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, MutexGuard};

use er_engine::arena::ArenaRunControls;
use er_engine::jobs::{JobSink, LocalJobSink};
use er_engine::ports::Ports;

use super::{FlowRunner, Flows};
use crate::actor::WorkspaceDeps;
use crate::handle::ActorMsg;

mod record;
mod results;

/// A flow runner on its own host, whose actor messages nobody reads.
pub(super) struct Harness {
    runner: FlowRunner,
    _actor: Receiver<ActorMsg>,
}

impl Harness {
    pub(super) fn new(ports: Ports) -> Self {
        let (tx, actor) = mpsc::channel();
        let sink: Arc<dyn JobSink> = Arc::new(LocalJobSink);
        let arena = ArenaRunControls::with_jobs(Arc::clone(&sink));
        let runner = FlowRunner::new(&WorkspaceDeps::new(ports), sink, arena, &tx);
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
}
