mod diff;
mod status;

/// Default `--unified=N` context lines for every `git diff` invocation and
/// for the in-process context fold. Per-file overrides via `+`/`-` build on
/// top of this; `CONTEXT_STEPS` starts here.
pub const DEFAULT_CONTEXT_LINES: usize = 10;

/// Sentinel value passed as `--unified=N` to mean "show the entire file".
/// Git accepts arbitrarily large values; any value larger than the file's
/// line count yields the full file as a single hunk.
pub const FULL_CONTEXT: usize = 99999;

/// `+`/`-` ladder for per-file context expansion. First entry is the default;
/// last entry is `FULL_CONTEXT`. Pressing `+` walks forward, `-` walks back.
pub const CONTEXT_STEPS: &[usize] = &[DEFAULT_CONTEXT_LINES, 20, 40, 80, FULL_CONTEXT];

/// Auto-expand size ladder: `(max_diff_lines, context_value)`. A file whose
/// total diff line count is `<= max_diff_lines` is auto-expanded to the
/// matching `context_value`. Files larger than the last tier fall back to
/// `DEFAULT_CONTEXT_LINES`. Ordered small → large.
pub const SIZE_LADDER: &[(usize, usize)] = &[(60, FULL_CONTEXT), (180, 80), (500, 40), (1500, 20)];

#[allow(unused_imports)]
pub use diff::{
    compact_files, compact_files_match, expand_compacted_file, filter_raw_diff_by_paths,
    header_to_stub, parse_diff, parse_diff_headers, parse_file_at_offset,
    refetch_file_with_context, CompactionConfig, DiffFile, DiffFileHeader, DiffHunk, DiffLine,
    LineType,
};
pub use status::{
    detect_base_branch_in, diff_shortstat, diff_watched_file_snapshot, discover_watched_files,
    get_current_branch_in, get_repo_root, get_repo_root_in, git_commit, git_diff_against_branch,
    git_diff_checkout_against_base, git_diff_commit, git_diff_conflicts, git_diff_raw,
    git_diff_raw_file, git_diff_raw_range, git_log_branch, git_log_head, git_log_range, git_push,
    git_stage_all, git_stage_file, git_unstage_file, is_merge_in_progress, list_worktrees,
    read_watched_file_content, save_snapshot, unmerged_files, verify_gitignored, CommitInfo,
    FileStatus, WatchedFile, Worktree,
};
