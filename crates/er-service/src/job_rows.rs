//! The work in flight a front end lists: the task host's rows, and the arena
//! runs and review flows the started actors publish. Built here once, so the
//! desktop's `jobs` and the TUI's rows read the same thing (ADR 0074).

use std::sync::Arc;

use er_engine::arena::{ArenaProgressState, RunStatus};
use er_protocol::{ArenaRunJob, ArenaRunProgress, ArenaRunStatus, TaskJob};

use crate::tasks::TaskHost;
use crate::workspace::{LiveReviewFlow, WorkspaceView};

/// The rows, each part sorted by its key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobRows {
    /// Oldest first.
    pub tasks: Vec<TaskJob>,
    /// By run id.
    pub arena_runs: Vec<ArenaRunJob>,
    /// By bucket.
    pub review_flows: Vec<LiveReviewFlow>,
}

/// The rows of `tasks` and of every view in `views`.
pub fn job_rows(views: &[Arc<WorkspaceView>], tasks: &TaskHost) -> JobRows {
    let mut arena_runs: Vec<ArenaRunJob> = views
        .iter()
        .flat_map(|view| view.arena_runs.iter())
        .map(|run| ArenaRunJob {
            run_id: run.run_id.clone(),
            er_dir: run.er_dir.clone(),
            status: run_status(&run.status),
            progress: progress(&run.progress),
        })
        .collect();
    arena_runs.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    let mut review_flows: Vec<LiveReviewFlow> = views
        .iter()
        .flat_map(|view| view.review_flows.iter().cloned())
        .collect();
    review_flows.sort_by(|a, b| a.er_dir.cmp(&b.er_dir));
    JobRows {
        tasks: tasks.rows(),
        arena_runs,
        review_flows,
    }
}

pub fn run_status(status: &RunStatus) -> ArenaRunStatus {
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
