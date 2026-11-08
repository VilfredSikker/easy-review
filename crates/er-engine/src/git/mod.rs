mod diff;
mod diff_stats;
mod file_kind;

pub use er_git::DEFAULT_CONTEXT_LINES;

/// Sentinel value passed as `--unified=N` to mean "show the entire file".
/// Git accepts arbitrarily large values; any value larger than the file's
/// line count yields the full file as a single hunk.
pub const FULL_CONTEXT: usize = 99999;

/// `+`/`-` ladder for per-file context expansion. First entry is the default;
/// last entry is `FULL_CONTEXT`. Pressing `+` walks forward, `-` walks back.
pub const CONTEXT_STEPS: &[usize] = &[DEFAULT_CONTEXT_LINES, 20, 40, 80, FULL_CONTEXT];

/// Auto-expand size ladder: `(max_diff_lines, context_value)`. A file whose
///
/// total diff line count is `<= max_diff_lines` is auto-expanded to the
/// matching `context_value`. Files larger than the last tier fall back to
/// `DEFAULT_CONTEXT_LINES`. Ordered small → large.
pub const SIZE_LADDER: &[(usize, usize)] = &[(60, FULL_CONTEXT), (180, 80), (500, 40), (1500, 20)];

pub use diff::{
    compact_files, compact_files_match, expand_compacted_file, filter_raw_diff_by_paths,
    filter_raw_diff_exclude_globs, header_to_stub, lazy_files_with_compaction, parse_diff,
    parse_diff_headers, parse_file_at_offset, refetch_file_with_context, CompactionConfig,
    DiffFile, DiffFileHeader, DiffHunk, DiffLine, LineType,
};
pub use diff_stats::{DiffKindStats, FileDiffStat, ProdDiffStats};
pub use er_git::{
    BlameEnd, BlameRequest, BlameSpan, CommitInfo, FileStatus, Git, GitError, LineAttribution,
    WatchedFile, Worktree,
};
pub use file_kind::{classify_path, FileKind};
