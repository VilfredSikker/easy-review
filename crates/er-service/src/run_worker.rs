//! The thread a host plans runs and builds flow commands on. Planning reads
//! git and writes the bucket, so a front end whose loop must never wait asks
//! the host, which answers a receiver at once and does the work here
//! (ADR 0074).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use er_engine::config::ErConfig;
use er_engine::ports::Ports;
use er_protocol::ErError;

use crate::content::TabContent;
use crate::flow_view::{current_review_diff, flow_context};
use crate::handle::{WorkspaceError, WorkspaceHandle};
use crate::protocol::{AcceptanceDiff, Command, FlowCommand, Reply};
use crate::runs::{self, RunTab};
use crate::tab_commands;
use crate::tasks::{refused, TaskHooks, TaskHost};

/// A run a front end asks for on one tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunRequest {
    Review {
        scope: String,
    },
    Expert {
        scope: String,
        expert_id: String,
    },
    Validate {
        scope: String,
    },
    Tour,
    Importance,
    Questions,
    Notes,
    Summary,
    /// A named `[commands]` entry, with the text the config gives it.
    Command {
        name: String,
        shell_cmd: String,
    },
}

/// What a run answers on, once it is queued or refused.
pub type RunReceiver = Receiver<Result<(), ErError>>;

/// A flow command a front end asks for on one tab. The host builds the
/// context or the acceptance diff the command carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowRequest {
    /// `scope` resolved as a review resolves it on the tab's view.
    Start {
        scope: String,
    },
    Confirm {
        er_dir: String,
    },
    Decline {
        er_dir: String,
    },
    Rerun {
        er_dir: String,
    },
    Cancel {
        er_dir: String,
    },
    RunStandard {
        er_dir: String,
    },
    Accept {
        er_dir: String,
        run_id: String,
        finding_ids: Vec<String>,
    },
}

impl FlowRequest {
    /// The bucket a control names, which must still be the tab's; none for
    /// a start, which takes the tab's, or an acceptance, which goes to the
    /// actor holding the flow.
    pub(crate) fn controlled_bucket(&self) -> Option<&str> {
        match self {
            Self::Confirm { er_dir }
            | Self::Decline { er_dir }
            | Self::Rerun { er_dir }
            | Self::Cancel { er_dir }
            | Self::RunStandard { er_dir } => Some(er_dir),
            Self::Start { .. } | Self::Accept { .. } => None,
        }
    }
}

type Job = Box<dyn FnOnce() + Send>;

/// One thread, started on first use, that runs each job in turn.
pub(crate) struct Worker(Sender<Job>);

impl Worker {
    pub(crate) fn start() -> Self {
        let (tx, jobs) = mpsc::channel::<Job>();
        // Unnamed, like every thread that reaches the storage root.
        let _ = std::thread::Builder::new().spawn(move || {
            for job in jobs {
                job();
            }
        });
        Self(tx)
    }

    /// Answers whether the worker took the job.
    fn run(&self, job: Job) -> bool {
        self.0.send(job).is_ok()
    }
}

/// Runs `job` on the worker with `reply`. A panic in it, or a worker that is
/// gone, answers `failed` with the reason, and the worker stays up for the
/// next job.
fn submit<T: Send + 'static>(
    worker: &Worker,
    reply: Sender<T>,
    failed: fn(String) -> T,
    job: impl FnOnce(Sender<T>) + Send + 'static,
) {
    let (panicked, gone) = (reply.clone(), reply.clone());
    let taken = worker.run(Box::new(move || {
        if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(reply))) {
            let text = panic
                .downcast_ref::<&str>()
                .map(ToString::to_string)
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            let _ = panicked.send(failed(format!("planning panicked: {text}")));
        }
    }));
    if !taken {
        let _ = gone.send(failed("the planning worker is not running".into()));
    }
}

/// Plans `request` on `content`'s tab on the worker and queues it on `host`.
pub(crate) fn plan(
    worker: &Worker,
    host: TaskHost,
    hooks: TaskHooks,
    content: Arc<TabContent>,
    request: RunRequest,
) -> RunReceiver {
    let (tx, rx) = mpsc::channel();
    let failed = |message: String| Err(refused(message));
    submit(worker, tx, failed, move |tx| {
        let run = RunTab {
            host: &host,
            hooks: &hooks,
            content: &content,
        };
        let _ = tx.send(planned(&run, request));
    });
    rx
}

fn planned(run: &RunTab<'_>, request: RunRequest) -> Result<(), ErError> {
    match request {
        RunRequest::Review { scope } => runs::review(run, &scope),
        RunRequest::Expert { scope, expert_id } => runs::expert(run, &scope, &expert_id),
        RunRequest::Validate { scope } => runs::validate(run, &scope),
        RunRequest::Tour => runs::tour(run),
        RunRequest::Importance => runs::importance(run),
        RunRequest::Questions => tab_commands::questions(run),
        RunRequest::Notes => tab_commands::notes(run),
        RunRequest::Summary => tab_commands::summary(run),
        RunRequest::Command { name, shell_cmd } => tab_commands::command(run, &name, &shell_cmd),
    }
}

/// What a flow command reads besides its tab.
pub(crate) struct FlowInputs {
    pub ports: Ports,
    pub config: Arc<ErConfig>,
    pub content: Arc<TabContent>,
    /// The actor that runs the flow.
    pub handle: WorkspaceHandle,
    /// Every loaded tab's content, which an acceptance looks its target up in.
    pub contents: Vec<Arc<TabContent>>,
}

/// Builds `request`'s command on the worker and sends it to the flow's actor,
/// whose reply lands on the receiver answered here.
pub(crate) fn flow(
    worker: &Worker,
    inputs: FlowInputs,
    request: FlowRequest,
) -> Receiver<Result<Reply, WorkspaceError>> {
    let (tx, rx) = mpsc::channel();
    let failed = |message: String| Err(WorkspaceError::Failed(message));
    submit(worker, tx, failed, move |tx| {
        match flow_command(&inputs, request) {
            Ok(command) => inputs.handle.send_to(Command::Flow(command), tx),
            Err(error) => {
                let _ = tx.send(Err(WorkspaceError::Failed(error.message)));
            }
        }
    });
    rx
}

fn flow_command(inputs: &FlowInputs, request: FlowRequest) -> Result<FlowCommand, ErError> {
    let FlowInputs {
        ports,
        config,
        content,
        ..
    } = inputs;
    let context = |scope| flow_context(ports, content, Arc::clone(config), scope);
    Ok(match request {
        FlowRequest::Start { scope } => {
            let scope = runs::review_scope(content, &scope)?;
            FlowCommand::Start {
                context: context(Some(&scope)),
                scope,
            }
        }
        FlowRequest::Confirm { er_dir } => FlowCommand::Confirm {
            context: context(None),
            er_dir,
        },
        FlowRequest::Decline { er_dir } => FlowCommand::Decline { er_dir },
        FlowRequest::Rerun { er_dir } => FlowCommand::Rerun {
            context: context(Some("branch")),
            er_dir,
        },
        FlowRequest::Cancel { er_dir } => FlowCommand::Cancel { er_dir },
        FlowRequest::RunStandard { er_dir } => FlowCommand::RunStandard {
            context: context(None),
            er_dir,
        },
        FlowRequest::Accept {
            er_dir,
            run_id,
            finding_ids,
        } => {
            let view = inputs.handle.view();
            let current = view
                .review_flow(&er_dir)
                .and_then(|live| live.run_targets.iter().find(|(id, _)| *id == run_id))
                .map(|(_, target)| AcceptanceDiff {
                    target: target.clone(),
                    diff: current_review_diff(
                        ports,
                        inputs.contents.iter().map(AsRef::as_ref),
                        target,
                    )
                    .map_err(|error| format!("{error:#}")),
                });
            FlowCommand::AcceptFindings {
                er_dir,
                run_id,
                finding_ids,
                current,
            }
        }
    })
}

/// Refused before any work: the tab is not listed or has not loaded.
pub(crate) fn refused_now(message: &str) -> RunReceiver {
    let (tx, rx) = mpsc::channel();
    let _ = tx.send(Err(refused(message)));
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A job that panics answers its caller with the panic's text, and the
    /// next job still runs.
    #[test]
    fn a_panicking_plan_leaves_the_worker_up() -> anyhow::Result<()> {
        let worker = Worker::start();
        let failed = |message: String| Err::<u32, String>(message);
        let (tx, panicked) = mpsc::channel();
        submit(&worker, tx, failed, |_| panic!("the plan broke"));
        let answer = panicked.recv_timeout(std::time::Duration::from_secs(5))?;
        assert!(
            answer.as_ref().is_err_and(|e| e.contains("the plan broke")),
            "{answer:?}"
        );
        let (tx, next) = mpsc::channel();
        submit(&worker, tx, failed, |tx| {
            let _ = tx.send(Ok(1));
        });
        assert_eq!(
            next.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(Ok(1))
        );
        Ok(())
    }
}
