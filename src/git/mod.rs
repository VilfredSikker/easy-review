mod diff;
mod status;

pub use diff::{
    DiffFile, DiffFileHeader, DiffHunk, LineType,
    parse_diff, parse_diff_headers, parse_file_at_offset, header_to_stub,
    compact_files, compact_files_match, expand_compacted_file, CompactionConfig,
    LAZY_PARSE_THRESHOLD,
};
#[cfg(test)]
pub(crate) use diff::DiffLine;
pub use status::{
    FileStatus, Worktree,
    detect_base_branch_in,
    get_repo_root,
    get_repo_root_in,
    get_current_branch_in,
    git_diff_raw,
    list_worktrees,
    git_stage_file, git_unstage_file, git_stage_all, git_stage_hunk,
};
