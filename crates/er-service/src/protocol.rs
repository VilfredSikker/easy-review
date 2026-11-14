//! The messages crossing the actor boundary.
//!
//! Every field an effect needs travels with the command: the actor never reads
//! app-scoped records, so the desktop resolves a project to a repo root and a
//! remote before sending. That keeps the store on the caller's side of the
//! seam and makes the service testable with a scripted port alone.

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

    // ── GitHub sync and the pending review ──
    /// Pull the tab's PR comments into its comments sidecar. The fetch runs
    /// off the actor; the actor writes the result.
    SyncGithubComments {
        context: TabContext,
        source: SyncSource,
    },
    /// Read the viewer's pending review on the tab's PR.
    GetPendingReview { context: TabContext },
    /// Add one line comment to the viewer's pending review.
    AddPendingReviewComment {
        context: TabContext,
        target: ReviewLineTarget,
        body: String,
    },
    /// Remove one comment from the viewer's pending review.
    RemovePendingReviewComment {
        context: TabContext,
        comment_node_id: String,
    },
    /// Submit the viewer's pending review with a decision.
    SubmitPendingReview {
        context: TabContext,
        event: ReviewEvent,
        summary: String,
    },
}

/// What started a comment sync, which decides how fresh the read must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncSource {
    /// The user asked. `force` skips the hover-prefetch bundle.
    Manual { force: bool },
    /// The periodic tick. Always reads GitHub, then drops the cached bundle so
    /// a later manual pull cannot go back to an older one.
    Background,
}

/// A line the composer selected for the pending review, in its side's numbering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewLineTarget {
    pub file: String,
    pub line_num: usize,
    pub line_num_end: Option<usize>,
    /// `LEFT` or `RIGHT`; `None` is the new side.
    pub side: Option<String>,
}

/// A review decision, as GitHub's review API names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewEvent {
    Approve,
    RequestChanges,
    Comment,
}

impl ReviewEvent {
    /// Parses the desktop's mode string; `None` for anything else.
    pub fn parse(mode: &str) -> Option<Self> {
        match mode {
            "APPROVE" => Some(Self::Approve),
            "REQUEST_CHANGES" => Some(Self::RequestChanges),
            "COMMENT" => Some(Self::Comment),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
            Self::Comment => "COMMENT",
        }
    }
}

/// The PR a GitHub command acted on, so the caller can refresh what it caches
/// for that PR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
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

/// The actor's answer to one command.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// What the sidecar holds after a sync.
    CommentsSynced {
        github_count: usize,
        local_count: usize,
    },
    /// The viewer's pending review, read after any write.
    PendingReview {
        pr: PrRef,
        review: er_github::PendingReview,
    },
    /// The pending review was submitted.
    ReviewSubmitted {
        pr: PrRef,
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
    /// A sync wrote the tab's comments sidecar.
    CommentsSynced {
        context: TabContext,
        github_count: usize,
        local_count: usize,
    },
    /// The viewer's pending review on the tab's PR changed or was submitted.
    PendingReviewChanged {
        context: TabContext,
    },
}
