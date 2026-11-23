//! The arena on `er_send` and `er_fetch`. A command or a read names the tab
//! the page painted; `arena_service` resolves it in the registry and builds
//! the context from that tab's content, with no App. An estimate, a run and
//! the run list are read on each fetch and never pushed: `jobs` carries the
//! live runs, and the page reads again when it moves.

use er_engine::arena::{
    AgentGroupStart, ArenaBatchStartParams, ArenaProgressState, ArenaRunSummary, ArenaStartParams,
    ReviewerRef, RunStatus,
};
use er_protocol::{
    Ack, ArenaEstimate, ArenaPlan, ArenaReviewer, ArenaRun, ArenaRunJob, ArenaRunProgress,
    ArenaRunRow, ArenaRunStatus, ArenaRuns, Command, ErError, ErErrorKind, Resource,
    ResourceChanged, ResourceKey, ResourceVersion,
};

use super::tabs::Routed;
use crate::arena_commands::{parse_scope, parse_verdict};
use crate::arena_service::{self, ArenaTab, Plan};
use crate::state::DesktopState;

/// Every live arena run the started actors publish, by run id.
pub fn live_runs(desktop: &DesktopState) -> Vec<ArenaRunJob> {
    let mut runs: Vec<ArenaRunJob> = desktop
        .service
        .published_views()
        .iter()
        .flat_map(|view| view.arena_runs.iter())
        .map(|run| ArenaRunJob {
            run_id: run.run_id.clone(),
            er_dir: run.er_dir.clone(),
            status: status(&run.status),
            progress: progress(&run.progress),
        })
        .collect();
    runs.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    runs
}

/// What `plan` would cost on `wire`'s diff.
pub fn estimate(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    plan: &ArenaPlan,
) -> Result<(u64, Resource), ErError> {
    let tab = arena_service::tab(desktop, Some(wire))?;
    let preview = if plan.groups.is_empty() {
        let plan = Plan {
            scope: parse_scope(&plan.scope),
            files: plan.files.clone(),
            reviewers: reviewers(&plan.reviewers),
            rounds: plan.rounds,
            arbiter: plan.arbiter.as_ref().map(reviewer),
        };
        arena_service::estimate(desktop, &tab, &plan)?
    } else {
        arena_service::estimate_batch(desktop, &tab, &batch(plan, false, None))?
    };
    let estimate = ArenaEstimate {
        diff_bytes: u64::try_from(preview.diff_bytes).unwrap_or(u64::MAX),
        cost_usd: preview.cost_usd,
        latency_sec: preview.latency_sec,
        cost_limit_usd: preview.cost_limit_usd,
        max_rounds: preview.max_rounds,
    };
    Ok((0, Resource::ArenaEstimate(estimate)))
}

/// The runs in `wire`'s bucket on its branch, newest first. A listed tab
/// nothing has loaded answers none and is sent a load (A3); the page reads
/// again once its content publishes.
pub fn runs(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<(u64, Resource), ErError> {
    let Some(tab) = arena_service::loaded(desktop, Some(wire))? else {
        return Ok((0, Resource::ArenaRuns(ArenaRuns::default())));
    };
    let runs = arena_service::list(desktop, &tab)?
        .iter()
        .map(row)
        .collect();
    Ok((0, Resource::ArenaRuns(ArenaRuns { runs })))
}

/// One run, from the bucket of the actor running it, else from `wire`'s
/// buckets, with its live status.
pub fn run(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    run_id: &str,
) -> Result<(u64, Resource), ErError> {
    let tab = arena_service::tab(desktop, Some(wire))?;
    let snapshot = arena_service::get(desktop, &tab, run_id)?;
    let json = serde_json::to_string(&snapshot).map_err(|error| ErError {
        kind: ErErrorKind::Internal,
        message: format!("write the run as JSON: {error}"),
    })?;
    Ok((0, Resource::ArenaRun(ArenaRun { json })))
}

/// Applies `command` when it is an arena command.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let applied = match command {
        Command::ArenaStart {
            tab,
            plan,
            title,
            confirm,
            agent_kind,
            effort,
        } => on(desktop, tab, |tab| {
            let run_ids = if plan.groups.is_empty() {
                let params = ArenaStartParams {
                    title,
                    reviewers: reviewers(&plan.reviewers),
                    scope: parse_scope(&plan.scope),
                    files: plan.files,
                    rounds: plan.rounds,
                    arbiter: plan.arbiter.as_ref().map(reviewer),
                    confirm,
                    agent_kind,
                    effort,
                };
                vec![arena_service::start(desktop, tab, params)?]
            } else {
                arena_service::start_batch(desktop, tab, batch(&plan, confirm, effort))?
            };
            Ok(Some(run_ids.join("\n")))
        }),
        Command::ArenaValidateFindings { tab } => on(desktop, tab, |tab| {
            arena_service::validate_findings(desktop, tab)
        }),
        Command::ArenaAcceptFindings {
            tab,
            run_id,
            finding_ids,
        } => on(desktop, tab, |tab| {
            let accepted = arena_service::accept_findings(desktop, tab, run_id, finding_ids)?;
            Ok(Some(accepted.to_string()))
        }),
        Command::ArenaDelete { tab, run_id } => on(desktop, tab, |tab| {
            arena_service::delete(desktop, tab, run_id).map(|()| None)
        }),
        Command::ArenaCancel { tab, run_id } => on(desktop, tab, |tab| {
            arena_service::cancel(desktop, tab, run_id).map(|()| None)
        }),
        Command::ArenaOverride {
            tab,
            run_id,
            finding_id,
            verdict,
            note,
        } => on(desktop, tab, |tab| {
            let verdict = parse_verdict(&verdict);
            arena_service::override_finding(desktop, tab, run_id, finding_id, verdict, note)
                .map(|()| None)
        }),
        other => return Routed::Other(other),
    };
    Routed::Applied(applied)
}

/// Runs `apply` on the tab `wire` names and answers what it created. The
/// `Ack` names `jobs`, which holds the live runs, and the tab's
/// `annotations(tab)`, which an import moves.
fn on(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    apply: impl FnOnce(&ArenaTab) -> Result<Option<String>, ErError>,
) -> Result<Ack, ErError> {
    let tab = arena_service::tab(desktop, Some(wire))?;
    let created = apply(&tab)?;
    let annotations = desktop
        .service
        .content(tab.entry.key, &tab.entry.descriptor)
        .map_or(0, |content| er_service::annotations_version(&content));
    Ok(Ack {
        changed: vec![
            ResourceChanged {
                key: ResourceKey::Jobs,
                version: ResourceVersion(super::jobs::version(desktop)),
            },
            ResourceChanged {
                key: ResourceKey::Annotations { tab: wire },
                version: ResourceVersion(annotations),
            },
        ],
        created,
    })
}

fn reviewer(reviewer: &ArenaReviewer) -> ReviewerRef {
    ReviewerRef {
        provider_id: reviewer.provider_id.clone(),
        model_id: reviewer.model_id.clone(),
        agent_kind: reviewer.agent_kind.clone(),
    }
}

fn reviewers(reviewers: &[ArenaReviewer]) -> Vec<ReviewerRef> {
    reviewers.iter().map(reviewer).collect()
}

fn batch(plan: &ArenaPlan, confirm: bool, effort: Option<String>) -> ArenaBatchStartParams {
    ArenaBatchStartParams {
        scope: parse_scope(&plan.scope),
        files: plan.files.clone(),
        rounds: plan.rounds,
        arbiter: plan.arbiter.as_ref().map(reviewer),
        confirm,
        groups: plan
            .groups
            .iter()
            .map(|group| AgentGroupStart {
                agent_kind: group.agent_kind.clone(),
                models: reviewers(&group.models),
                title: group.title.clone(),
            })
            .collect(),
        effort,
    }
}

fn status(status: &RunStatus) -> ArenaRunStatus {
    match status {
        RunStatus::Queued => ArenaRunStatus::Queued,
        RunStatus::Running { round } => ArenaRunStatus::Running { round: *round },
        RunStatus::Complete => ArenaRunStatus::Complete,
        RunStatus::Cancelled => ArenaRunStatus::Cancelled,
        RunStatus::Failed => ArenaRunStatus::Failed,
    }
}

fn progress(progress: &ArenaProgressState) -> ArenaRunProgress {
    ArenaRunProgress {
        round: progress.round,
        total_rounds: progress.total_rounds,
        phase: progress.phase.clone(),
        thinking: progress.thinking.clone(),
        done: progress.done.clone(),
    }
}

fn row(summary: &ArenaRunSummary) -> ArenaRunRow {
    ArenaRunRow {
        id: summary.id.clone(),
        title: summary.title.clone(),
        branch_ref: summary.branch_ref.clone(),
        status: status(&summary.status),
        created_at: summary.created_at.clone(),
        completed_at: summary.completed_at.clone(),
        reviewer_count: u32::try_from(summary.reviewer_count).unwrap_or(u32::MAX),
        finding_count: u32::try_from(summary.finding_count).unwrap_or(u32::MAX),
    }
}
