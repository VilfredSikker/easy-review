//! Git subprocess access behind a port.
//!
//! Every git call is a subprocess (docs/adr/0001-shell-out-to-git.md). This
//! crate owns the subprocess code behind the [`Git`] trait: [`CliGit`] runs the
//! binary, [`FakeGit`] answers from a script, and [`proc`] bounds the calls.
//! The handle travels as `er_engine::ports::Ports`
//! (docs/adr/0054-git-is-a-port.md).
//!
//! A new git command is a new trait method, added by the PR that needs it.

pub mod proc;

mod blame;
mod cli;
mod error;
mod fake;
mod status;

pub use cli::{CliGit, Git};
pub use error::GitError;
pub use fake::FakeGit;

/// Default `--unified=N` context lines for every `git diff` invocation and
/// for the in-process context fold. Per-file overrides via `+`/`-` build on
/// top of this; the engine's `CONTEXT_STEPS` starts here.
pub const DEFAULT_CONTEXT_LINES: usize = 10;

pub use blame::{
    blame_range, git_merge_base, git_rev_parse_commit, BlameEnd, BlameRequest, BlameSpan,
    LineAttribution,
};
pub use status::{
    detect_base_branch_in, diff_shortstat, diff_watched_file_snapshot, discover_watched_files,
    get_current_branch_in, get_repo_root, get_repo_root_at, get_repo_root_in, git_blob_oid,
    git_commit, git_commit_shortstats, git_diff_against_branch, git_diff_checkout_against_base,
    git_diff_commit, git_diff_conflicts, git_diff_raw, git_diff_raw_file, git_diff_raw_range,
    git_log_branch, git_log_head, git_log_range, git_paths_present_at, git_push, git_read_blob,
    git_stage_all, git_stage_file, git_unpushed_shas, git_unstage_file, gitignored_paths,
    is_merge_in_progress, list_worktrees, read_watched_file_content, save_snapshot, unmerged_files,
    CommitInfo, FileStatus, WatchedFile, Worktree,
};
