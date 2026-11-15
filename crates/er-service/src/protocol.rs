//! The messages crossing the actor boundary.
//!
//! Every field an effect needs travels with the command: the actor never reads
//! app-scoped records, so the desktop resolves a project to a repo root and a
//! remote before sending. That keeps the store on the caller's side of the
//! seam and makes the service testable with a scripted port alone.

use std::sync::mpsc::Sender;
use std::sync::Arc;

use er_agent::job::{JobError, JobProgress};
use er_engine::arena::{
    ArenaBatchStartParams, ArenaDiffPreview, ArenaScope, ArenaStartParams, ReviewerRef, Verdict,
};
use er_engine::config::ErConfig;
use er_engine::jobs::{JobId, JobResult, JobSubmission};

use crate::workspace::WorkspaceTarget;

/// A command sent to the workspace actor.
#[derive(Clone, Debug)]
pub enum Command {
    /// Open a PR target. The actor resolves it off-actor and applies it.
    OpenTarget {
        target: OpenTarget,
        /// Whether the target replaces the active one rather than joining the
        /// list. The actor applies either by target identity; the flag is
        /// carried so the desktop route keeps its `replace` parameter.
        replace: bool,
    },
    /// Test support: reply without touching any port.
    #[cfg(test)]
    Ping,
    /// Test support: call a port on the actor thread, which the guard refuses.
    #[cfg(test)]
    PortProbe,
    /// Test support: hold the reply open forever, so a caller can prove its
    /// own timeout fires.
    #[cfg(test)]
    Stall,

    // ── Annotation commands (P41.1) ──
    /// Flip one path's reviewed mark against the stored map.
    ToggleReviewed { context: TabContext, path: String },
    /// Set one path's reviewed mark to an explicit state.
    MarkReviewed {
        context: TabContext,
        path: String,
        reviewed: bool,
    },
    /// Set every in-diff path of a tour pillar to an explicit state.
    BulkReviewPillar {
        context: TabContext,
        pillar_id: String,
        reviewed: bool,
    },
    /// Add a question. `client_id` is the id the front end painted.
    AddQuestion {
        context: TabContext,
        target: AnnotationTarget,
        body: String,
        side: Option<String>,
        client_id: Option<String>,
        reply_to: Option<String>,
        finding_ref: Option<String>,
    },
    /// Add a note.
    AddNote {
        context: TabContext,
        target: AnnotationTarget,
        body: String,
        side: Option<String>,
        client_id: Option<String>,
        reply_to: Option<String>,
        finding_ref: Option<String>,
    },
    /// Anchor a question or note on a line of the document itself.
    AddDocumentThread {
        context: TabContext,
        file: String,
        line: usize,
        line_end: Option<usize>,
        kind: AnnotationKind,
        body: String,
        /// The previewed file text the line was picked in.
        source: String,
    },
    /// Reply to a question, note or local GitHub comment.
    ReplyToThread {
        context: TabContext,
        parent_id: String,
        body: String,
        client_id: Option<String>,
    },
    /// Replace one thread message's body.
    UpdateThreadMessage {
        context: TabContext,
        id: String,
        body: String,
    },
    /// Delete one thread and its replies from its sidecar.
    DeleteThread { context: TabContext, id: String },
    /// Mark one thread resolved.
    ResolveThread { context: TabContext, id: String },
    /// Promote a question to a private note.
    PromoteToNote {
        context: TabContext,
        id: String,
        body: Option<String>,
    },
    /// Reply to a finding; the AI-assist variant stays on the flag-off path.
    ReplyToFinding {
        context: TabContext,
        finding_id: String,
        body: String,
        ai_assist: bool,
        client_id: Option<String>,
    },

    // ── GitHub comment commands (P41.2) ──
    /// Create a comment and post it to the tab's pull request.
    AddComment {
        context: TabContext,
        target: AnnotationTarget,
        body: String,
        side: Option<String>,
        client_id: Option<String>,
    },
    /// Reply to a thread that is already on GitHub.
    ReplyToGithubThread {
        context: TabContext,
        parent_id: String,
        body: String,
        client_id: Option<String>,
    },
    /// Promote a question or note to a GitHub comment, then remove the source.
    PromoteToComment {
        context: TabContext,
        id: String,
        body: Option<String>,
    },
    /// Promote a finding to a GitHub comment, then remove the finding.
    PromoteFindingToComment {
        context: TabContext,
        finding_id: String,
        body: Option<String>,
    },

    // ── Job and arena commands ──
    /// Run one agent job on an effect thread. The actor keeps its stop handle
    /// until it finishes, so `CancelJob` can reach the runner; the outcome goes
    /// to `result` and out as `Event::JobFinished`.
    StartJob {
        job: JobId,
        submission: JobSubmission,
        result: Sender<JobResult>,
    },
    /// Stop a running job by killing the stop handle its runner watches.
    CancelJob { job: JobId },
    /// Load-time recovery of the named buckets: stale arena runs and a dead
    /// owner's running workflow stages are marked. Nothing is started.
    Reconcile { buckets: Vec<String> },
    /// An arena action on the actor's live runs.
    Arena(ArenaCommand),
}

/// An arena action. Each carries the [`ArenaContext`] its effect needs, except
/// a cancel, which acts on the live run alone.
#[derive(Clone, Debug)]
pub enum ArenaCommand {
    /// Price a run before it starts. It reads the diff through the ports, so
    /// it runs as an effect and answers with the preview; there is no state
    /// to publish for it.
    Estimate {
        arena: ArenaContext,
        scope: ArenaScope,
        files: Option<Vec<String>>,
        reviewers: Vec<ReviewerRef>,
        rounds: Option<u8>,
        arbiter: Option<ReviewerRef>,
    },
    EstimateBatch {
        arena: ArenaContext,
        params: ArenaBatchStartParams,
    },
    StartRun {
        arena: ArenaContext,
        params: ArenaStartParams,
    },
    StartBatch {
        arena: ArenaContext,
        params: ArenaBatchStartParams,
    },
    /// Dedupe the tab's expert findings and rule on them with one arbiter job.
    ValidateFindings {
        arena: ArenaContext,
    },
    AcceptFindings {
        arena: ArenaContext,
        run_id: String,
        finding_ids: Option<Vec<String>>,
    },
    /// Delete a finished run's directory.
    Delete {
        arena: ArenaContext,
        run_id: String,
    },
    Cancel {
        run_id: String,
    },
    Override {
        arena: ArenaContext,
        run_id: String,
        finding_id: String,
        verdict: Verdict,
        note: String,
    },
}

/// What an arena effect reads from the tab and the App, resolved by the
/// desktop before sending.
#[derive(Clone, Debug)]
pub struct ArenaContext {
    /// The active tab: its bucket and the inputs its diff fetch needs.
    pub tab: TabContext,
    /// The branch a run is filed under.
    pub branch_ref: String,
    pub config: Arc<ErConfig>,
    /// Every open tab's bucket, the active one first. A run id is looked up
    /// here when no live control names its bucket.
    pub buckets: Vec<String>,
    /// The loaded review's diff hash, which seeded validation is checked
    /// against. Empty when no review is loaded.
    pub review_hash: String,
}

/// How a job ended, as subscribers see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobOutcome {
    /// The provider exited; `success` is its exit status.
    Completed {
        success: bool,
    },
    Failed {
        message: String,
    },
    Cancelled,
    TimedOut,
}

impl JobOutcome {
    pub fn of(result: &JobResult) -> Self {
        match result {
            Ok(output) => Self::Completed {
                success: output.status.success(),
            },
            Err(JobError::Cancelled) => Self::Cancelled,
            Err(JobError::TimedOut(_)) => Self::TimedOut,
            Err(error) => Self::Failed {
                message: error.to_string(),
            },
        }
    }
}

/// The tab a command acts on, with every input its effect needs. The desktop
/// resolves these from the tab before sending, so the actor never reads the
/// front end's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabContext {
    /// Managed bucket the private sidecars live in.
    pub er_dir: String,
    /// Bucket the GitHub comment sidecar lives in (the PR bucket for PR tabs).
    pub comments_dir: String,
    /// Whether the tab is a remote PR tab, as `resolve_post_target` expects.
    pub is_remote: bool,
    pub repo_root: String,
    pub base_branch: String,
    /// `branch`, `staged` or `unstaged`.
    pub scope: String,
    pub remote_repo: Option<String>,
    pub pr_number: Option<u64>,
    pub pr_head_ref: Option<String>,
    pub local_branch_view: Option<String>,
    pub checkout_root: Option<String>,
    /// The tab's branch diff hash. Question and note writes compute one when
    /// it is empty; `PromoteToNote` and GitHub comments store it as given, so
    /// an empty value stays empty — the same file contents the flag-off path
    /// wrote.
    pub branch_diff_hash: String,
    /// The tab's parsed diff hash.
    pub diff_hash: String,
}

/// Where an annotation goes, as the desktop passes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationTarget {
    pub file: String,
    pub hunk_idx: usize,
    pub line_num: Option<usize>,
    pub line_num_end: Option<usize>,
}

/// Which private annotation a document anchor creates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationKind {
    Question,
    Note,
}

/// What to open. See the module docs for why the repo root is a parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenTarget {
    /// A GitHub PR URL, e.g. from a link in the UI.
    PrUrl { url: String, repo_root: String },
    /// An owner/repo/number given directly.
    RemotePr {
        owner: String,
        repo: String,
        number: u64,
        repo_root: String,
    },
    /// A project's PR, by project id and the project's remote (empty when the
    /// project has no remote; the target still opens, just without state).
    PrReview {
        project_id: String,
        remote: String,
        pr_number: u64,
        repo_root: String,
    },
}

impl OpenTarget {
    /// The repo root the caller resolved for this target; empty when the
    /// caller has no local project. Used as the actor's workspace key.
    pub fn repo_root(&self) -> &str {
        match self {
            Self::PrUrl { repo_root, .. }
            | Self::RemotePr { repo_root, .. }
            | Self::PrReview { repo_root, .. } => repo_root,
        }
    }
}

/// The actor's answer to one command. No `Eq`: an estimate carries prices.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    TargetOpened(WorkspaceTarget),
    /// The stored row's id, which is the painted id when one was supplied.
    AnnotationWritten {
        id: String,
    },
    /// How many reviewed rows changed.
    ReviewedChanged {
        changed: usize,
    },
    /// The sidecar a thread row was changed in.
    ThreadChanged {
        path: String,
    },
    /// The note a question was promoted into.
    NotePromoted {
        id: String,
    },
    /// The GitHub comment a question, note or finding was promoted into.
    CommentPromoted {
        id: String,
    },
    /// The finding thread a reply landed in.
    FindingReplied {
        root_id: String,
        reply_id: String,
    },
    JobStarted {
        job: JobId,
    },
    JobCancelled {
        job: JobId,
    },
    /// The buckets whose workflow record can be re-run.
    Reconciled {
        resumable: Vec<String>,
    },
    ArenaEstimate(ArenaDiffPreview),
    ArenaStarted {
        run_ids: Vec<String>,
    },
    /// The seeded run, or `None` when there were no expert findings.
    ArenaValidating {
        run_id: Option<String>,
    },
    ArenaAccepted {
        accepted: usize,
    },
    ArenaChanged {
        run_id: String,
    },
    #[cfg(test)]
    Pong,
}

/// Pushed to subscribers after the actor applies a change. An event names the
/// revision, not the state: the reader fetches the view, so a slow subscriber
/// never holds the actor or pins an old copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A new view revision is published; readers fetch `handle.view()`.
    ViewPublished {
        revision: u64,
    },
    TargetOpened {
        target: WorkspaceTarget,
    },
    CommandFailed {
        message: String,
    },
    /// One output line from a running job.
    JobProgress {
        job: JobId,
        line: JobProgress,
    },
    JobFinished {
        job: JobId,
        outcome: JobOutcome,
    },
}
