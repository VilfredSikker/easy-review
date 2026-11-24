//! The Workspace actor: commands in, events out, one actor per repo/worktree.
//!
//! The actor owns the workspace state for one repository. A command names what
//! to do; the actor reads that state on its own thread, hands any subprocess
//! work to an effect thread through [`WorkspaceDeps::ports`], and applies the
//! typed result back on the actor thread. Readers never touch the state: the
//! actor publishes an immutable [`WorkspaceView`] per revision behind an
//! `RwLock<Arc<_>>`, and [`WorkspaceHandle::view`] clones the current one.
//!
//! The engine dependency is deliberately headless (`default-features =
//! false`): the actor is the seam the desktop and the TUI will share, so it
//! must not pull a front end into the build.
//!
//! See ADR 0067 for the crate graph and the actor's position in it.

mod actor;
mod ai_extras;
mod ai_labels;
mod annotations;
mod annotations_resource;
mod app_registry;
pub mod arena;
mod ask_context;
mod content;
mod diff_resource;
mod effect;
mod finding_writes;
mod flow_view;
mod flows;
mod freshness;
mod github_comments;
mod github_sync;
mod guard;
mod handle;
mod history;
mod host;
mod hunks;
mod job_rows;
mod jobs;
mod pending_review;
mod pr_posts;
mod pr_resource;
mod protocol;
mod reviewed_resource;
mod run_worker;
pub mod runs;
mod settings;
mod settings_wire;
mod sidecar_reload;
pub mod tab_commands;
mod tab_pr;
pub mod tasks;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod workspace;
mod worktree;

pub use actor::{spawn, WorkspaceDeps};
pub use ai_extras::{AiExtras, OtherBucketFacts};
pub use ai_labels::{ai_labels, AiLabels};
pub use annotations_resource::{annotations_resource, annotations_version, tour_resource};
pub use app_registry::{
    live_tab_row, resolve_tab_key, tabs_resource, wire_tab_key, AppCommand, AppRegistry, CachedPr,
    TabEntry, TabsView,
};
pub use ask_context::{ask_raw_diff, ask_scope};
pub use content::{
    refused_as_moved, AiSection, DiffMeta, DiffSection, FreshnessSection, HistorySection,
    PrSection, ReviewedSection, ScopeStats, SidecarStamp, StackRead, TabContent,
};
pub use diff_resource::{
    diff_load_error, diff_resource, diff_version, file_rule_tables, listed_files, DiffInputs,
};
pub use effect::{descriptor_bucket, tab_bucket};
pub use flow_view::{
    current_review_diff, flow_context, review_flow_inputs, review_raw_diff, review_rules,
};
pub use flows::read_flow_run;
pub use freshness::{BaseBehind, Divergence, Freshness, RemoteTips};
pub use github_comments::{edit_message, EARLIER_VERSION_POSTED};
pub use handle::{WorkspaceError, WorkspaceHandle, REPLY_TIMEOUT};
pub use host::{ReplyReceiver, ServiceHost};
pub use hunks::{file_hunks, read_hunks, HunkRead};
pub use job_rows::{job_rows, run_status, JobRows};
pub use pending_review::{read_pending_review, OWN_PR_APPROVAL_REFUSED};
pub use pr_posts::{review_write_message, with_review_hints};
pub use pr_resource::{pr_resource, pr_version};
pub use protocol::{
    AcceptanceDiff, AnnotationKind, AnnotationTarget, ArenaCommand, ArenaContext, Command, Event,
    FlowCommand, FlowContext, JobOutcome, OpenTarget, PrRef, Reply, ReviewArtifact, ReviewEvent,
    ReviewLineTarget, SyncSource, TabContext,
};
pub use reviewed_resource::{
    content_view, painted_view_matches, reviewed_resource, reviewed_version,
};
pub use run_worker::{FlowRequest, RunReceiver, RunRequest};
pub use settings::{spawn_settings, SettingsCommand, SettingsHandle, SettingsReply, SettingsView};
pub use settings_wire::{risk_resource, settings_command, settings_resource};
pub use tasks::{Finished, Launch, TaskClock, TaskHooks, TaskHost};
pub use workspace::{LiveArenaRun, LiveReviewFlow, LoadError, WorkspaceTarget, WorkspaceView};

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod job_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod task_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod content_tests;
