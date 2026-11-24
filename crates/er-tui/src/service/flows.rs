//! The flag-on review flows: the AI hub's Review rows by the tab's flow, the
//! progress and results overlays, and what each says once the holder actor
//! answers. A row only asks: the host builds the flow command on its worker
//! and the actor runs it (ADR 0074), and the results are read there too.

use std::collections::HashSet;

use er_engine::ai::review_flow::{FlowState, ReviewFlow};
use er_engine::review_flow_driver::ReviewFlowResults;
use er_service::{FlowRequest, LiveReviewFlow, Reply, ServiceHost, TabContent, WorkspaceError};

use crate::input::keys::OverlayAction;

/// What a Review row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Start { scope: String },
    Confirm(String),
    Decline(String),
    Cancel(String),
    Progress(String),
    Results { er_dir: String, run_id: String },
    Noop,
}

impl Act {
    /// The request a row sends, and what its answer says; `None` for a row
    /// that opens an overlay or does nothing.
    pub fn request(self) -> Option<(FlowRequest, Ask)> {
        Some(match self {
            Self::Start { scope } => (FlowRequest::Start { scope }, Ask::Start),
            Self::Confirm(er_dir) => (FlowRequest::Confirm { er_dir }, Ask::Confirm),
            Self::Decline(er_dir) => (FlowRequest::Decline { er_dir }, Ask::Decline),
            Self::Cancel(er_dir) => (FlowRequest::Cancel { er_dir }, Ask::Cancel),
            Self::Progress(_) | Self::Results { .. } | Self::Noop => return None,
        })
    }
}

/// The tab's flow as the hub reads it when it opens.
#[derive(Clone)]
pub struct FlowFacts {
    er_dir: String,
    scope: String,
    live: Option<Box<LiveReviewFlow>>,
    has_diff: bool,
}

impl FlowFacts {
    pub fn of(content: &TabContent, live: Option<&LiveReviewFlow>) -> Self {
        Self {
            er_dir: content.context.er_dir.clone(),
            scope: content.tab.mode.fetch_scope().to_string(),
            live: live.cloned().map(Box::new),
            has_diff: !content.diff.files.is_empty(),
        }
    }
}

/// One Review row: label, description, whether it is live, what it does.
pub type Row = (&'static str, String, bool, Act);

/// The Review rows at the top of the AI hub, in the App's order: the row by
/// the flow's state, then the Deep decline, the progress row and the cancel
/// the state adds.
pub fn rows(facts: &FlowFacts) -> Vec<Row> {
    let er_dir = facts.er_dir.clone();
    let start = || {
        let about = format!("Start review flow on the active {} diff", facts.scope);
        let act = Act::Start {
            scope: facts.scope.clone(),
        };
        ("Review", about, facts.has_diff, act)
    };
    let live = facts.live.as_deref();
    let draining = live.is_some_and(|live| live.draining);
    let flow = live.map(|live| &live.flow);
    let running = flow.is_some_and(|flow| {
        matches!(
            flow.state,
            FlowState::Triaging | FlowState::Finding | FlowState::Verifying
        )
    });
    let awaiting = flow.is_some_and(|flow| flow.state == FlowState::AwaitingConfirmation);
    let first = match flow {
        _ if draining => (
            "Review stopping",
            "Owned review work is still stopping".into(),
            false,
            Act::Noop,
        ),
        None => start(),
        Some(flow) if awaiting => {
            let description = format!("{} · Enter confirms Deep", estimate(flow));
            ("Review", description, true, Act::Confirm(er_dir.clone()))
        }
        Some(flow) if running => {
            let description = match flow.state {
                FlowState::Triaging => "Triage is running · open task progress",
                FlowState::Finding => "Finding reviewers are running · open task progress",
                _ => "Deep verification is running · open task progress",
            };
            let act = Act::Progress(er_dir.clone());
            ("Review", description.into(), true, act)
        }
        Some(flow) => match (&flow.state, &flow.arena_run_id) {
            (
                FlowState::Done {
                    awaiting_accept, ..
                },
                Some(run_id),
            ) if *awaiting_accept > 0 => {
                let description =
                    format!("{awaiting_accept} findings waiting to accept · open results");
                let act = Act::Results {
                    er_dir: er_dir.clone(),
                    run_id: run_id.clone(),
                };
                ("Review", description, true, act)
            }
            (
                FlowState::Done {
                    awaiting_accept, ..
                },
                None,
            ) if *awaiting_accept > 0 => {
                let (_, _, enabled, act) = start();
                let about = "Previous review is complete; start a new Review".into();
                ("Review", about, enabled, act)
            }
            _ => start(),
        },
    };
    let mut rows = vec![first];
    if awaiting {
        let about = "Decline the pending Deep review".into();
        rows.push((
            "Cancel Deep review",
            about,
            true,
            Act::Decline(er_dir.clone()),
        ));
    }
    if flow.is_some() && (!running || draining) {
        let about = "Inspect the captured plan, models, and task status".into();
        rows.push((
            "Review task progress",
            about,
            true,
            Act::Progress(er_dir.clone()),
        ));
    }
    if running && !draining {
        let about = "Stop the running review".into();
        rows.push(("Cancel review", about, true, Act::Cancel(er_dir)));
    }
    rows
}

fn estimate(flow: &ReviewFlow) -> String {
    let minutes = |seconds: f64| (seconds / 60.0).ceil() as u64;
    match (flow.estimated_cost, flow.estimated_latency_seconds) {
        (Some(cost), Some(seconds)) => {
            format!("Estimated ${cost:.2} · about {} min", minutes(seconds))
        }
        (Some(cost), None) => format!("Estimated ${cost:.2}"),
        (None, Some(seconds)) => format!("About {} min", minutes(seconds)),
        (None, None) => "Deep review is ready to confirm".into(),
    }
}

/// What a flow request's answer says, as the App's hub says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    Start,
    Confirm,
    Decline,
    Cancel,
    Rerun,
    Accept { er_dir: String, run_id: String },
}

impl Ask {
    /// The status line's message, and the results to read again after an
    /// acceptance.
    pub fn landed(
        &self,
        reply: Result<Reply, WorkspaceError>,
    ) -> (Option<String>, Option<(String, String)>) {
        let said = match (self, reply) {
            (Self::Accept { er_dir, run_id }, Ok(Reply::FlowAccepted { accepted })) => {
                let reread = Some((er_dir.clone(), run_id.clone()));
                return (Some(format!("Accepted {accepted} finding(s)")), reread);
            }
            (Self::Accept { .. }, Ok(_)) => return (None, None),
            (Self::Accept { .. }, Err(error)) => format!("Could not accept findings: {error}"),
            (Self::Start, Ok(_)) => "Review started".into(),
            (Self::Confirm, Ok(_)) => "Deep review started".into(),
            (Self::Decline, Ok(_)) => "Deep review declined".into(),
            (Self::Cancel, Ok(_)) => "Review stopping".into(),
            (Self::Cancel, Err(_)) => "No running review to cancel".into(),
            (Self::Rerun, Ok(_)) => return (None, None),
            (Self::Start | Self::Rerun, Err(error)) => format!("Review: {error}"),
            (Self::Confirm | Self::Decline, Err(error)) => format!("Deep review: {error}"),
        };
        (Some(said), None)
    }
}

/// The progress overlay over a copy of the published flow.
pub struct Progress {
    pub er_dir: String,
    pub flow: ReviewFlow,
    pub selected: usize,
    pub detail_scroll: usize,
}

impl Progress {
    pub fn new(er_dir: String, flow: ReviewFlow) -> Self {
        Self {
            er_dir,
            flow,
            selected: 0,
            detail_scroll: 0,
        }
    }

    /// Takes the newer flow and keeps the selected task by id, as the App's
    /// refresh does.
    pub fn refresh(&mut self, latest: &ReviewFlow) {
        let selected_id = self.flow.tasks.get(self.selected).map(|task| &task.id);
        let next = selected_id
            .and_then(|id| latest.tasks.iter().position(|task| &task.id == id))
            .unwrap_or_else(|| self.selected.min(latest.tasks.len().saturating_sub(1)));
        if next != self.selected {
            self.detail_scroll = 0;
        }
        self.selected = next;
        self.flow = latest.clone();
    }
}

/// The results overlay: the run's surviving findings and the picked ones.
pub struct Results {
    pub found: ReviewFlowResults,
    pub selected: usize,
    pub selected_ids: HashSet<String>,
    pub detail_scroll: usize,
}

impl Results {
    pub fn new(results: ReviewFlowResults) -> Self {
        Self {
            found: results,
            selected: 0,
            selected_ids: HashSet::new(),
            detail_scroll: 0,
        }
    }
}

/// What a key on a flow overlay does next.
pub enum Then {
    Stay,
    Close,
    Notify(&'static str),
    /// Sends the request; `close` closes the overlay first.
    Send {
        request: FlowRequest,
        ask: Ask,
        close: bool,
    },
}

/// Moves `selected` within `len` rows; a move starts the detail at its top.
fn step(selected: &mut usize, detail_scroll: &mut usize, len: usize, action: &OverlayAction) {
    let to = match action {
        OverlayAction::Next if *selected + 1 < len => *selected + 1,
        OverlayAction::Prev if *selected > 0 => *selected - 1,
        OverlayAction::ScrollDetail(delta) => {
            *detail_scroll = if *delta < 0 {
                detail_scroll.saturating_sub(delta.unsigned_abs())
            } else {
                detail_scroll
                    .saturating_add(delta.unsigned_abs())
                    .min(10_000)
            };
            return;
        }
        _ => return,
    };
    *selected = to;
    *detail_scroll = 0;
}

pub fn progress_key(progress: &mut Progress, action: OverlayAction) -> Then {
    match action {
        OverlayAction::RerunReviewFlow => Then::Send {
            request: FlowRequest::Rerun {
                er_dir: progress.er_dir.clone(),
            },
            ask: Ask::Rerun,
            close: true,
        },
        OverlayAction::Close => Then::Close,
        action => {
            let len = progress.flow.tasks.len();
            step(
                &mut progress.selected,
                &mut progress.detail_scroll,
                len,
                &action,
            );
            Then::Stay
        }
    }
}

pub fn results_key(results: &mut Results, action: OverlayAction) -> Then {
    match action {
        OverlayAction::ToggleFlowFinding => match results.found.findings.get(results.selected) {
            Some(finding) if finding.selectable => {
                if !results.selected_ids.remove(&finding.id) {
                    results.selected_ids.insert(finding.id.clone());
                }
                Then::Stay
            }
            Some(_) => Then::Notify("This finding cannot be accepted"),
            None => Then::Notify("There are no findings to select"),
        },
        OverlayAction::AcceptFlowFindings if results.selected_ids.is_empty() => {
            Then::Notify("Select at least one finding to accept")
        }
        OverlayAction::AcceptFlowFindings => {
            let mut finding_ids: Vec<String> = results.selected_ids.iter().cloned().collect();
            finding_ids.sort();
            let (er_dir, run_id) = (&results.found.er_dir, &results.found.run_id);
            Then::Send {
                request: FlowRequest::Accept {
                    er_dir: er_dir.clone(),
                    run_id: run_id.clone(),
                    finding_ids,
                },
                ask: Ask::Accept {
                    er_dir: er_dir.clone(),
                    run_id: run_id.clone(),
                },
                close: false,
            }
        }
        OverlayAction::Close => Then::Close,
        action => {
            let len = results.found.findings.len();
            step(
                &mut results.selected,
                &mut results.detail_scroll,
                len,
                &action,
            );
            Then::Stay
        }
    }
}

/// Asks each listed tab's actor, once per bucket, to show a flow a dead
/// owner left, as the App's start does: it marks records and starts nothing.
pub fn reconcile(host: &ServiceHost, seen: &mut HashSet<String>) {
    for entry in &host.registry().tabs_view().tabs {
        let workspace = entry.descriptor.workspace_key();
        let Some(content) = host.content(&workspace, entry.key) else {
            continue;
        };
        let er_dir = content.context.er_dir.clone();
        if seen.insert(er_dir.clone()) {
            let buckets = vec![er_dir];
            drop(host.post(&workspace, er_service::Command::Reconcile { buckets }));
        }
    }
}
