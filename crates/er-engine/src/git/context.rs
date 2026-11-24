//! The context-line steps and the automatic pick, shared by the App and the
//! flag-on TUI so both choose the same count.

use super::{
    DiffFile, FileStatus, CONTEXT_STEPS, DEFAULT_CONTEXT_LINES, FULL_CONTEXT, SIZE_LADDER,
};

/// The step `+` takes from `current`, none at full context.
pub fn next_context_step(current: usize) -> Option<usize> {
    let next = CONTEXT_STEPS
        .iter()
        .copied()
        .find(|&step| step > current)
        .unwrap_or(FULL_CONTEXT);
    (next != current).then_some(next)
}

/// The step `-` takes from `current`, none at the default.
pub fn prev_context_step(current: usize) -> Option<usize> {
    (current > DEFAULT_CONTEXT_LINES).then(|| {
        CONTEXT_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&step| step < current)
            .unwrap_or(DEFAULT_CONTEXT_LINES)
    })
}

/// Whether a file's diff already shows the whole file: an added file in one
/// hunk.
pub fn shows_whole_file(file: &DiffFile) -> bool {
    file.status == FileStatus::Added && file.hunks.len() <= 1
}

/// The count the automatic expansion picks for a parsed file with no count
/// of its own, none when it keeps the default.
pub fn auto_context(file: &DiffFile) -> Option<usize> {
    if file.compacted || shows_whole_file(file) {
        return None;
    }
    let total_lines: usize = file.hunks.iter().map(|hunk| hunk.lines.len()).sum();
    if total_lines == 0 {
        return None;
    }
    let picked = pick_context_for_size(total_lines, file.hunks.len());
    (picked > DEFAULT_CONTEXT_LINES).then_some(picked)
}

/// Tiered ladder via `SIZE_LADDER`: bigger context for smaller files, lean
/// context for big ones. `total_lines` is the file's diff line count at the
/// default `--unified` value; `hunks` is the hunk count. Files larger than
/// the last tier keep `DEFAULT_CONTEXT_LINES`.
pub fn pick_context_for_size(total_lines: usize, hunks: usize) -> usize {
    let Some(tier) = SIZE_LADDER
        .iter()
        .position(|(limit, _)| total_lines <= *limit)
    else {
        return DEFAULT_CONTEXT_LINES;
    };
    // Many scattered small hunks read better with more context.
    let nudged = if hunks >= 4 && tier > 0 {
        tier - 1
    } else {
        tier
    };
    SIZE_LADDER[nudged].1
}
