mod diff;
mod status;

#[allow(unused_imports)]
pub use diff::{
    compact_files, compact_files_match, expand_compacted_file, header_to_stub, parse_diff,
    parse_diff_headers, parse_file_at_offset, refetch_file_with_context, CompactionConfig,
    DiffFile, DiffFileHeader, DiffHunk, DiffLine, LineType,
};
pub use status::{
    detect_base_branch_in, diff_watched_file_snapshot, discover_watched_files,
    get_current_branch_in, get_repo_root, get_repo_root_in, git_commit, git_diff_commit,
    git_diff_conflicts, git_diff_raw, git_diff_raw_file, git_diff_raw_range, git_grep_symbol,
    git_log_branch, git_push, git_stage_all, git_stage_file, git_unstage_file,
    is_merge_in_progress, list_worktrees, read_watched_file_content, save_snapshot, unmerged_files,
    verify_gitignored, CommitInfo, FileStatus, WatchedFile, Worktree,
};
