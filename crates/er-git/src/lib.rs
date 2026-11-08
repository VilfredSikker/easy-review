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

pub use blame::{BlameEnd, BlameRequest, BlameSpan, LineAttribution};
pub use status::{
    discover_watched_files, git_log_branch, read_watched_file_content, save_snapshot, CommitInfo,
    FileStatus, WatchedFile, Worktree,
};
