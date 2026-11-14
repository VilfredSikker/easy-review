use crate::status::FileStatus;

/// A single line in a diff hunk
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_type: LineType,
    pub content: String,
    pub old_num: Option<usize>,
    pub new_num: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineType {
    Context,
    Add,
    Delete,
    /// A folded run of context lines. Carries the count of hidden lines.
    Fold(usize),
}

/// A diff hunk with header and lines
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

impl DiffHunk {
    /// Format this hunk as displayable text (for clipboard)
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&self.header);
        text.push('\n');
        for line in &self.lines {
            let prefix = match line.line_type {
                LineType::Add => "+",
                LineType::Delete => "-",
                LineType::Context => " ",
                LineType::Fold(_) => continue,
            };
            text.push_str(prefix);
            text.push_str(&line.content);
            text.push('\n');
        }
        text
    }
}

/// A file with its diff hunks and metadata
#[derive(Debug, Clone)]
pub struct DiffFile {
    pub path: String,
    pub status: FileStatus,
    pub hunks: Vec<DiffHunk>,
    pub adds: usize,
    pub dels: usize,
    /// Whether this file is compacted (hunks not parsed/rendered)
    pub compacted: bool,
    /// Raw hunk count before compaction (for display)
    pub raw_hunk_count: usize,
    /// Git's `similarity index N%` for a rename or copy; `None` for every other
    /// status. Kept beside `status` rather than inside `FileStatus::Renamed` so
    /// the many matches on that variant do not change shape.
    pub similarity: Option<u8>,
    // Binary files ("Binary files a/x and b/x differ") parse to hunks=[], adds=0,
    // dels=0 — indistinguishable here from a mode-only change.
}
