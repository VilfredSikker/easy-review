mod diff;
mod status;

pub use diff::{DiffFile, LineType, parse_diff};
#[cfg(test)]
pub(crate) use diff::{DiffHunk, DiffLine};
pub use status::{
    FileStatus, Worktree, CommitInfo,
    detect_base_branch_in,
    get_repo_root,
    get_repo_root_in,
    get_current_branch_in,
    git_diff_raw,
    list_worktrees,
    git_stage_file, git_unstage_file, git_stage_all, git_stage_hunk,
    git_log_branch, git_diff_commit,
};
