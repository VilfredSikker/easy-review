use super::status::FileStatus;

/// Snap a byte offset to the nearest valid char boundary (searching forward).
fn snap_to_char_boundary(s: &str, offset: usize) -> usize {
    if offset >= s.len() {
        return s.len();
    }
    let mut pos = offset;
    while pos < s.len() && !s.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

/// A single line in a diff hunk
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_type: LineType,
    pub content: String,
    pub old_num: Option<usize>,
    pub new_num: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineType {
    Context,
    Add,
    Delete,
}

/// A diff hunk with header and lines
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: usize,
    #[allow(dead_code)]
    pub old_count: usize,
    pub new_start: usize,
    #[allow(dead_code)]
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
    // TODO(risk:minor): binary files appear in git diff output as "Binary files a/x and b/x differ"
    // with no hunk section. The parser produces a DiffFile with hunks=[], adds=0, dels=0, and no
    // way for the UI to distinguish "binary changed" from "mode-only change" or "compacted".
    // A `is_binary: bool` flag would allow the UI to show a meaningful label instead of nothing.
}

/// Lightweight file header extracted from a fast scan of the raw diff.
/// Contains metadata but no parsed hunks — used for lazy parsing.
#[derive(Debug, Clone)]
pub struct DiffFileHeader {
    pub path: String,
    pub status: FileStatus,
    pub adds: usize,
    pub dels: usize,
    pub hunk_count: usize,
    /// Byte offset of this file's section start in the raw diff string
    pub byte_offset: usize,
    /// Length of this file's section in bytes
    pub byte_length: usize,
}

/// Threshold for enabling lazy (two-phase) parsing
pub const LAZY_PARSE_THRESHOLD: usize = 5000;

/// Fast header-only scan of a raw diff.
/// Extracts file paths, status, +/- counts, and byte offsets without
/// allocating DiffLine structs. ~10x faster than full parse for large diffs.
pub fn parse_diff_headers(raw: &str) -> Vec<DiffFileHeader> {
    let mut headers: Vec<DiffFileHeader> = Vec::new();
    let mut current_header: Option<DiffFileHeader> = None;
    let mut byte_pos: usize = 0;

    for line in raw.lines() {
        // TODO(risk:medium): byte_pos advances by line.len() + 1 (assuming '\n'), but CRLF
        // line endings add an extra byte that is stripped by .lines(). On Windows or in diffs
        // containing CRLF, byte offsets will drift and parse_file_at_offset() will slice the
        // raw string at the wrong position, producing garbled or panicking parses.
        let line_byte_end = byte_pos + line.len() + 1; // +1 for \n

        if line.starts_with("diff --git") {
            // Flush previous header
            if let Some(ref mut h) = current_header {
                h.byte_length = byte_pos.saturating_sub(h.byte_offset);
                headers.push(h.clone());
            }

            // Extract path
            let path = if let Some(after_a) = line.strip_prefix("diff --git a/") {
                let path_len = (after_a.len().saturating_sub(3)) / 2;
                if path_len > 0
                    && after_a.len() >= path_len + 3
                    && after_a.get(..path_len) == after_a.get(path_len + 3..)
                {
                    after_a[..path_len].to_string()
                } else {
                    // TODO(risk:medium): for renames where old and new paths differ and both
                    // contain the substring " b/", split(" b/").last() returns the wrong
                    // segment. A path like "src/a b/foo.rs" renamed to "src/b b/bar.rs"
                    // produces an incorrect path. The full-parse variant below has the same
                    // issue. Correct fix: read the "rename to" line instead.
                    after_a.split(" b/").last().unwrap_or("").to_string()
                }
            } else {
                line.split(" b/").last().unwrap_or("").to_string()
            };

            current_header = Some(DiffFileHeader {
                path,
                status: FileStatus::Modified,
                adds: 0,
                dels: 0,
                hunk_count: 0,
                byte_offset: byte_pos,
                byte_length: 0,
            });
        } else if let Some(ref mut h) = current_header {
            // Detect file status
            if line.starts_with("new file") {
                h.status = FileStatus::Added;
            } else if line.starts_with("deleted file") {
                h.status = FileStatus::Deleted;
            } else if line.starts_with("rename from ") {
                let old_path = line.strip_prefix("rename from ").unwrap_or("").to_string();
                h.status = FileStatus::Renamed(old_path);
            } else if line.starts_with("@@") {
                h.hunk_count += 1;
            } else if line.starts_with('+') && !line.starts_with("+++") {
                h.adds += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                h.dels += 1;
            }
        }

        byte_pos = line_byte_end;
    }

    // Flush last header
    if let Some(ref mut h) = current_header {
        h.byte_length = raw.len().saturating_sub(h.byte_offset);
        headers.push(h.clone());
    }

    headers
}

/// Parse hunks for a single file from a raw diff section.
/// Used for on-demand (lazy) parsing when a file is selected.
pub fn parse_file_at_offset(raw: &str, header: &DiffFileHeader) -> DiffFile {
    let end = (header.byte_offset + header.byte_length).min(raw.len());
    // Snap byte offsets to the nearest char boundary to avoid slice panics on non-ASCII diffs
    let start = snap_to_char_boundary(raw, header.byte_offset);
    let end = snap_to_char_boundary(raw, end);
    let section = &raw[start..end];
    let mut files = parse_diff(section);
    if let Some(mut file) = files.pop() {
        // Ensure path matches (the section parse may produce a valid file)
        file.path = header.path.clone();
        file.status = header.status.clone();
        file
    } else {
        // Fallback: empty file with header metadata
        DiffFile {
            path: header.path.clone(),
            status: header.status.clone(),
            hunks: Vec::new(),
            adds: header.adds,
            dels: header.dels,
            compacted: false,
            raw_hunk_count: 0,
        }
    }
}

/// Convert a DiffFileHeader to a DiffFile with no hunks (for display in file tree)
pub fn header_to_stub(header: &DiffFileHeader) -> DiffFile {
    DiffFile {
        path: header.path.clone(),
        status: header.status.clone(),
        hunks: Vec::new(),
        adds: header.adds,
        dels: header.dels,
        compacted: false,
        raw_hunk_count: header.hunk_count,
    }
}

/// Parse unified diff output into structured data
pub fn parse_diff(raw: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut current_file: Option<DiffFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line: usize = 0;
    let mut new_line: usize = 0;

    for line in raw.lines() {
        // New file header: diff --git a/path b/path
        if line.starts_with("diff --git") {
            // Save previous hunk and file
            if let Some(hunk) = current_hunk.take() {
                if let Some(ref mut file) = current_file {
                    file.hunks.push(hunk);
                }
            }
            if let Some(file) = current_file.take() {
                files.push(file);
            }

            // Extract path from "diff --git a/PATH b/PATH"
            // For non-rename diffs both paths are identical, so the format is:
            //   "diff --git a/PATH b/PATH"
            // After stripping "diff --git a/" we have: "PATH b/PATH"
            // Total = 2*PATH_len + 3, so PATH_len = (total - 3) / 2
            // We validate both halves match to distinguish non-renames from renames.
            // For renames (different paths), fall back to split(" b/").last().
            let path = if let Some(after_a) = line.strip_prefix("diff --git a/") {
                let path_len = (after_a.len().saturating_sub(3)) / 2;
                if path_len > 0
                    && after_a.len() >= path_len + 3
                    && after_a.get(..path_len) == after_a.get(path_len + 3..)
                {
                    after_a[..path_len].to_string()
                } else {
                    // Rename or edge case: paths differ, use the new path after " b/"
                    after_a.split(" b/").last().unwrap_or("").to_string()
                }
            } else {
                line.split(" b/").last().unwrap_or("").to_string()
            };

            current_file = Some(DiffFile {
                path,
                status: FileStatus::Modified, // will be refined below
                hunks: Vec::new(),
                adds: 0,
                dels: 0,
                compacted: false,
                raw_hunk_count: 0,
            });
            continue;
        }

        // Detect file status from diff headers
        if let Some(ref mut file) = current_file {
            if line.starts_with("new file") {
                file.status = FileStatus::Added;
                continue;
            }
            if line.starts_with("deleted file") {
                file.status = FileStatus::Deleted;
                continue;
            }
            if line.starts_with("rename from ") {
                let old_path = line.strip_prefix("rename from ").unwrap_or("").to_string();
                file.status = FileStatus::Renamed(old_path);
                continue;
            }
            // Skip other header lines (index, ---, +++)
            if line.starts_with("index ")
                || line.starts_with("--- ")
                || line.starts_with("+++ ")
                || line.starts_with("similarity index")
                || line.starts_with("rename to")
                || line.starts_with("old mode")
                || line.starts_with("new mode")
            {
                continue;
            }
        }

        // Hunk header: @@ -old_start,old_count +new_start,new_count @@ context
        if line.starts_with("@@") {
            // Save previous hunk
            if let Some(hunk) = current_hunk.take() {
                if let Some(ref mut file) = current_file {
                    file.hunks.push(hunk);
                }
            }

            if let Some(parsed) = parse_hunk_header(line) {
                old_line = parsed.old_start;
                new_line = parsed.new_start;
                current_hunk = Some(parsed);
            }
            continue;
        }

        // Diff content lines
        if let Some(ref mut hunk) = current_hunk {
            if let Some(stripped) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine {
                    line_type: LineType::Add,
                    content: stripped.to_string(),
                    old_num: None,
                    new_num: Some(new_line),
                });
                new_line += 1;
                if let Some(ref mut file) = current_file {
                    file.adds += 1;
                }
            } else if let Some(stripped) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine {
                    line_type: LineType::Delete,
                    content: stripped.to_string(),
                    old_num: Some(old_line),
                    new_num: None,
                });
                old_line += 1;
                if let Some(ref mut file) = current_file {
                    file.dels += 1;
                }
            } else if line.starts_with(' ') || line.is_empty() {
                // TODO(risk:medium): treating a bare empty line as a context line (with both
                // old_num and new_num advancing) is only correct for genuine empty context
                // lines. Some git versions emit a truly empty line between hunk sections
                // rather than inside them; misclassifying those shifts all subsequent line
                // numbers by one, breaking comment-to-line-number mapping.
                let content = if line.is_empty() {
                    String::new()
                } else {
                    line[1..].to_string()
                };
                hunk.lines.push(DiffLine {
                    line_type: LineType::Context,
                    content,
                    old_num: Some(old_line),
                    new_num: Some(new_line),
                });
                old_line += 1;
                new_line += 1;
            }
            // Skip \ No newline at end of file
        }
    }

    // Don't forget the last hunk/file
    if let Some(hunk) = current_hunk {
        if let Some(ref mut file) = current_file {
            file.hunks.push(hunk);
        }
    }
    if let Some(file) = current_file {
        files.push(file);
    }

    files
}

/// Parse a hunk header like "@@ -10,4 +10,15 @@ fn foo()"
fn parse_hunk_header(line: &str) -> Option<DiffHunk> {
    // Find the range info between @@ markers
    let after_first = line.strip_prefix("@@ ")?;
    let end_idx = after_first.find(" @@")?;
    let range_str = &after_first[..end_idx];
    let context = after_first[end_idx + 3..].trim().to_string();

    // Parse "-old_start,old_count +new_start,new_count"
    let parts: Vec<&str> = range_str.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let (old_start, old_count) = parse_range(parts[0].trim_start_matches('-'))?;
    let (new_start, new_count) = parse_range(parts[1].trim_start_matches('+'))?;

    let header = if context.is_empty() {
        format!("@@ -{},{} +{},{} @@", old_start, old_count, new_start, new_count)
    } else {
        format!(
            "@@ -{},{} +{},{} @@ {}",
            old_start, old_count, new_start, new_count, context
        )
    };

    Some(DiffHunk {
        header,
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

/// Parse "start,count" or just "start" (count defaults to 1)
fn parse_range(s: &str) -> Option<(usize, usize)> {
    if let Some((start, count)) = s.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((s.parse().ok()?, 1))
    }
}

// ── Compaction ──

/// Default glob patterns for files that should be auto-compacted
const DEFAULT_COMPACTION_PATTERNS: &[&str] = &[
    "*.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "Gemfile.lock",
    "poetry.lock",
    "composer.lock",
    "go.sum",
    "*.min.js",
    "*.min.css",
    "*.generated.*",
    "*.snap",
    "*.pb.go",
    "*.g.dart",
];

/// Configuration for auto-compaction of low-value files
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    pub enabled: bool,
    pub patterns: Vec<String>,
    pub max_lines_before_compact: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        CompactionConfig {
            enabled: true,
            patterns: DEFAULT_COMPACTION_PATTERNS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            max_lines_before_compact: 1000,
        }
    }
}

/// Check if a file path matches a glob-like pattern.
/// Supports: `*.ext`, `exact_name`, `prefix*suffix`, `dir/**`
/// Public alias for use in lazy-mode compaction.
pub fn compact_files_match(pattern: &str, path: &str) -> bool {
    glob_match(pattern, path)
}

fn glob_match(pattern: &str, path: &str) -> bool {
    // Get just the filename for extension-based patterns
    let filename = path.rsplit('/').next().unwrap_or(path);

    if let Some(suffix) = pattern.strip_prefix("*.") {
        // *.ext — match against filename extension(s)
        // For patterns like "*.generated.*", check if filename contains the pattern
        if suffix.contains('*') {
            // Complex pattern like "*.generated.*" — check if middle part is present
            if let Some(middle) = suffix.strip_suffix(".*") {
                return filename.contains(&format!(".{}.", middle))
                    || filename.ends_with(&format!(".{}", middle));
            }
        }
        filename.ends_with(&format!(".{}", suffix))
    } else if pattern.ends_with("/**") {
        // dir/** — match if path starts with dir/
        let dir = pattern.trim_end_matches("/**");
        path.starts_with(&format!("{}/", dir))
    } else {
        // Exact filename match (e.g., "package-lock.json")
        filename == pattern || path == pattern
    }
}

/// Apply compaction to files based on pattern matching and size thresholds.
/// Compacted files have their hunks cleared to save memory.
pub fn compact_files(files: &mut [DiffFile], config: &CompactionConfig) {
    if !config.enabled {
        return;
    }
    for file in files.iter_mut() {
        let total_lines: usize = file.hunks.iter().map(|h| h.lines.len()).sum();
        let should_compact = config
            .patterns
            .iter()
            .any(|p| glob_match(p, &file.path))
            || total_lines > config.max_lines_before_compact;

        if should_compact {
            file.compacted = true;
            file.raw_hunk_count = file.hunks.len();
            file.hunks.clear();
            file.hunks.shrink_to_fit();
        }
    }
}

/// Expand a previously compacted file by re-parsing its diff from git.
/// Uses `git diff -- <path>` for just that one file.
pub fn expand_compacted_file(
    file: &mut DiffFile,
    repo_root: &str,
    mode: &str,
    base: &str,
) -> anyhow::Result<()> {
    let raw = super::status::git_diff_raw_file(mode, base, repo_root, &file.path)?;
    let parsed = parse_diff(&raw);
    if let Some(f) = parsed.into_iter().next() {
        file.hunks = f.hunks;
        file.adds = f.adds;
        file.dels = f.dels;
        file.compacted = false;
        file.raw_hunk_count = 0;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::status::FileStatus;

    // === Existing tests ===

    #[test]
    fn test_parse_simple_diff() {
        let raw = r#"diff --git a/src/main.rs b/src/main.rs
index abc123..def456 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@ fn main()
 fn main() {
+    println!("hello");
     let x = 1;
 }
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].adds, 1);
        assert_eq!(files[0].dels, 0);
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].lines.len(), 4);
    }

    #[test]
    fn test_parse_new_file() {
        let raw = r#"diff --git a/new.rs b/new.rs
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,2 @@
+fn hello() {}
+fn world() {}
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Added);
        assert_eq!(files[0].adds, 2);
    }

    #[test]
    fn test_parse_hunk_header() {
        let hunk = parse_hunk_header("@@ -10,4 +10,15 @@ impl Foo");
        assert!(hunk.is_some());
        let h = hunk.unwrap();
        assert_eq!(h.old_start, 10);
        assert_eq!(h.old_count, 4);
        assert_eq!(h.new_start, 10);
        assert_eq!(h.new_count, 15);
    }

    // === New tests ===

    // --- parse_diff ---

    #[test]
    fn test_parse_diff_empty_input() {
        let files = parse_diff("");
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_parse_diff_deleted_file() {
        let raw = r#"diff --git a/old.rs b/old.rs
deleted file mode 100644
index abc1234..0000000
--- a/old.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-fn gone() {
-    // this file is gone
-}
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "old.rs");
        assert_eq!(files[0].status, FileStatus::Deleted);
        assert_eq!(files[0].dels, 3);
        assert_eq!(files[0].adds, 0);
    }

    #[test]
    fn test_parse_diff_renamed_file() {
        let raw = r#"diff --git a/src/old_name.rs b/src/new_name.rs
similarity index 95%
rename from src/old_name.rs
rename to src/new_name.rs
index abc1234..def5678 100644
--- a/src/old_name.rs
+++ b/src/new_name.rs
@@ -1,3 +1,3 @@
 fn unchanged() {}
-fn old_fn() {}
+fn new_fn() {}
 fn also_unchanged() {}
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/new_name.rs");
        assert_eq!(files[0].status, FileStatus::Renamed("src/old_name.rs".to_string()));
    }

    #[test]
    fn test_parse_diff_multiple_files() {
        let raw = r#"diff --git a/foo.rs b/foo.rs
index aaa..bbb 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,2 +1,3 @@
 fn foo() {}
+fn bar() {}
 fn baz() {}
diff --git a/qux.rs b/qux.rs
index ccc..ddd 100644
--- a/qux.rs
+++ b/qux.rs
@@ -1,2 +1,1 @@
 fn qux() {}
-fn old() {}
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "foo.rs");
        assert_eq!(files[0].adds, 1);
        assert_eq!(files[0].dels, 0);
        assert_eq!(files[1].path, "qux.rs");
        assert_eq!(files[1].adds, 0);
        assert_eq!(files[1].dels, 1);
    }

    #[test]
    fn test_parse_diff_multiple_hunks_per_file() {
        let raw = r#"diff --git a/src/lib.rs b/src/lib.rs
index aaa..bbb 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,5 @@
 fn alpha() {}
+fn alpha_new() {}
 fn beta() {}
 fn gamma() {}
 fn delta() {}
@@ -20,4 +21,3 @@
 fn omega() {}
-fn removed() {}
 fn psi() {}
 fn chi() {}
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!(files[0].hunks[0].lines.len(), 5);
        assert_eq!(files[0].hunks[1].lines.len(), 4);
    }

    #[test]
    fn test_parse_diff_context_lines_have_both_line_numbers() {
        let raw = r#"diff --git a/src/lib.rs b/src/lib.rs
index aaa..bbb 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -5,4 +5,4 @@
 context_before
-deleted_line
+added_line
 context_after
"#;
        let files = parse_diff(raw);
        let hunk = &files[0].hunks[0];

        let context_before = &hunk.lines[0];
        assert_eq!(context_before.line_type, LineType::Context);
        assert_eq!(context_before.old_num, Some(5));
        assert_eq!(context_before.new_num, Some(5));

        let context_after = &hunk.lines[3];
        assert_eq!(context_after.line_type, LineType::Context);
        // old_num advanced past the deleted line, new_num past the added line
        assert_eq!(context_after.old_num, Some(7));
        assert_eq!(context_after.new_num, Some(7));
    }

    #[test]
    fn test_parse_diff_line_number_tracking() {
        let raw = r#"diff --git a/src/lib.rs b/src/lib.rs
index aaa..bbb 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,5 +10,5 @@
 context_line
-deleted_a
-deleted_b
+added_x
+added_y
 context_end
"#;
        let files = parse_diff(raw);
        let hunk = &files[0].hunks[0];

        // context_line at old=10, new=10
        assert_eq!(hunk.lines[0].old_num, Some(10));
        assert_eq!(hunk.lines[0].new_num, Some(10));

        // deleted_a at old=11, no new_num
        assert_eq!(hunk.lines[1].line_type, LineType::Delete);
        assert_eq!(hunk.lines[1].old_num, Some(11));
        assert_eq!(hunk.lines[1].new_num, None);

        // deleted_b at old=12, no new_num
        assert_eq!(hunk.lines[2].line_type, LineType::Delete);
        assert_eq!(hunk.lines[2].old_num, Some(12));
        assert_eq!(hunk.lines[2].new_num, None);

        // added_x: no old_num, new=11
        assert_eq!(hunk.lines[3].line_type, LineType::Add);
        assert_eq!(hunk.lines[3].old_num, None);
        assert_eq!(hunk.lines[3].new_num, Some(11));

        // added_y: no old_num, new=12
        assert_eq!(hunk.lines[4].line_type, LineType::Add);
        assert_eq!(hunk.lines[4].old_num, None);
        assert_eq!(hunk.lines[4].new_num, Some(12));

        // context_end at old=13, new=13
        assert_eq!(hunk.lines[5].old_num, Some(13));
        assert_eq!(hunk.lines[5].new_num, Some(13));
    }

    #[test]
    fn test_parse_diff_no_newline_at_eof_is_skipped() {
        let raw = r#"diff --git a/src/lib.rs b/src/lib.rs
index aaa..bbb 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,3 @@
 fn foo() {}
+fn bar() {}
 fn baz() {}
\ No newline at end of file
"#;
        let files = parse_diff(raw);
        let hunk = &files[0].hunks[0];
        // The "\ No newline" line must not appear as a diff line
        assert_eq!(hunk.lines.len(), 3);
        for line in &hunk.lines {
            assert!(!line.content.contains("No newline"));
        }
    }

    #[test]
    fn test_parse_diff_path_with_space_containing_b() {
        // Path "foo b/bar.rs" contains " b/" which would break naive split(" b/").last()
        let raw = "diff --git a/foo b/bar.rs b/foo b/bar.rs\nindex aaa..bbb 100644\n--- a/foo b/bar.rs\n+++ b/foo b/bar.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "foo b/bar.rs");
    }

    #[test]
    fn test_parse_diff_mode_only_change_no_hunk() {
        let raw = r#"diff --git a/script.sh b/script.sh
old mode 100644
new mode 100755
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "script.sh");
        assert_eq!(files[0].hunks.len(), 0);
        assert_eq!(files[0].adds, 0);
        assert_eq!(files[0].dels, 0);
    }

    #[test]
    fn test_parse_diff_only_additions() {
        let raw = r#"diff --git a/src/new.rs b/src/new.rs
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,4 @@
+fn one() {}
+fn two() {}
+fn three() {}
+fn four() {}
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].adds, 4);
        assert_eq!(files[0].dels, 0);
    }

    #[test]
    fn test_parse_diff_only_deletions() {
        let raw = r#"diff --git a/src/gone.rs b/src/gone.rs
deleted file mode 100644
index abc1234..0000000
--- a/src/gone.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-fn one() {}
-fn two() {}
-fn three() {}
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].adds, 0);
        assert_eq!(files[0].dels, 3);
    }

    #[test]
    fn test_parse_diff_adds_and_dels_accumulate_across_hunks() {
        let raw = r#"diff --git a/src/lib.rs b/src/lib.rs
index aaa..bbb 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,5 @@
+fn extra_top() {}
 fn alpha() {}
-fn beta_old() {}
+fn beta_new() {}
 fn gamma() {}
@@ -50,3 +51,4 @@
 fn omega() {}
+fn omega_extra() {}
-fn omega_removed() {}
+fn omega_replaced() {}
"#;
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        // hunk 1: +extra_top, -beta_old, +beta_new  => 2 adds, 1 del
        // hunk 2: +omega_extra, -omega_removed, +omega_replaced => 2 adds, 1 del
        assert_eq!(files[0].adds, 4);
        assert_eq!(files[0].dels, 2);
    }

    // --- parse_hunk_header ---

    #[test]
    fn test_parse_hunk_header_without_context() {
        let h = parse_hunk_header("@@ -1,3 +1,4 @@").unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 3);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 4);
        // Header reconstructed without trailing context
        assert_eq!(h.header, "@@ -1,3 +1,4 @@");
    }

    #[test]
    fn test_parse_hunk_header_start_only_no_count() {
        // "@@ -1 +1 @@" — count defaults to 1 for both sides
        let h = parse_hunk_header("@@ -1 +1 @@").unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 1);
    }

    #[test]
    fn test_parse_hunk_header_malformed_missing_closing_markers() {
        // Missing second "@@" — should return None
        let result = parse_hunk_header("@@ -1,3 +1,4");
        assert_eq!(result.is_none(), true);
    }

    #[test]
    fn test_parse_hunk_header_zero_ranges_new_file() {
        // New file hunk: old side is 0,0
        let h = parse_hunk_header("@@ -0,0 +1,2 @@").unwrap();
        assert_eq!(h.old_start, 0);
        assert_eq!(h.old_count, 0);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 2);
    }

    // --- DiffHunk::to_text ---

    #[test]
    fn test_to_text_mixed_lines() {
        let hunk = DiffHunk {
            header: "@@ -1,3 +1,3 @@".to_string(),
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                DiffLine {
                    line_type: LineType::Context,
                    content: "unchanged".to_string(),
                    old_num: Some(1),
                    new_num: Some(1),
                },
                DiffLine {
                    line_type: LineType::Delete,
                    content: "old line".to_string(),
                    old_num: Some(2),
                    new_num: None,
                },
                DiffLine {
                    line_type: LineType::Add,
                    content: "new line".to_string(),
                    old_num: None,
                    new_num: Some(2),
                },
            ],
        };

        let text = hunk.to_text();
        let expected = "@@ -1,3 +1,3 @@\n unchanged\n-old line\n+new line\n";
        assert_eq!(text, expected);
    }

    #[test]
    fn test_to_text_only_additions() {
        let hunk = DiffHunk {
            header: "@@ -0,0 +1,2 @@".to_string(),
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 2,
            lines: vec![
                DiffLine {
                    line_type: LineType::Add,
                    content: "fn first() {}".to_string(),
                    old_num: None,
                    new_num: Some(1),
                },
                DiffLine {
                    line_type: LineType::Add,
                    content: "fn second() {}".to_string(),
                    old_num: None,
                    new_num: Some(2),
                },
            ],
        };

        let text = hunk.to_text();
        assert_eq!(text, "@@ -0,0 +1,2 @@\n+fn first() {}\n+fn second() {}\n");
    }

    #[test]
    fn test_to_text_empty_content_context_line() {
        let hunk = DiffHunk {
            header: "@@ -5,3 +5,3 @@".to_string(),
            old_start: 5,
            old_count: 3,
            new_start: 5,
            new_count: 3,
            lines: vec![
                DiffLine {
                    line_type: LineType::Context,
                    content: String::new(),
                    old_num: Some(5),
                    new_num: Some(5),
                },
                DiffLine {
                    line_type: LineType::Add,
                    content: "fn foo() {}".to_string(),
                    old_num: None,
                    new_num: Some(6),
                },
                DiffLine {
                    line_type: LineType::Delete,
                    content: "fn bar() {}".to_string(),
                    old_num: Some(6),
                    new_num: None,
                },
            ],
        };

        let text = hunk.to_text();
        // Empty context line renders as " \n" (space prefix, empty content, newline)
        assert_eq!(text, "@@ -5,3 +5,3 @@\n \n+fn foo() {}\n-fn bar() {}\n");
    }

    // ── glob_match ──

    #[test]
    fn glob_match_extension_pattern() {
        assert!(super::glob_match("*.lock", "Cargo.lock"));
        assert!(super::glob_match("*.lock", "some/path/Gemfile.lock"));
        assert!(!super::glob_match("*.lock", "lockfile.txt"));
    }

    #[test]
    fn glob_match_exact_filename() {
        assert!(super::glob_match("package-lock.json", "package-lock.json"));
        assert!(super::glob_match("package-lock.json", "some/dir/package-lock.json"));
        assert!(!super::glob_match("package-lock.json", "other.json"));
    }

    #[test]
    fn glob_match_dir_glob() {
        assert!(super::glob_match("__generated__/**", "__generated__/types.ts"));
        assert!(super::glob_match("__generated__/**", "__generated__/sub/file.rs"));
        assert!(!super::glob_match("__generated__/**", "src/generated.rs"));
    }

    #[test]
    fn glob_match_generated_wildcard() {
        assert!(super::glob_match("*.generated.*", "types.generated.ts"));
        assert!(super::glob_match("*.generated.*", "path/api.generated.go"));
        assert!(!super::glob_match("*.generated.*", "generated.ts"));
    }

    #[test]
    fn glob_match_min_js() {
        assert!(super::glob_match("*.min.js", "bundle.min.js"));
        assert!(super::glob_match("*.min.css", "styles.min.css"));
        assert!(!super::glob_match("*.min.js", "bundle.js"));
    }

    // ── compact_files ──

    #[test]
    fn compact_files_by_pattern() {
        let mut files = vec![
            DiffFile {
                path: "Cargo.lock".to_string(),
                status: FileStatus::Modified,
                hunks: vec![DiffHunk {
                    header: "@@ -1,1 +1,1 @@".to_string(),
                    old_start: 1, old_count: 1, new_start: 1, new_count: 1,
                    lines: vec![DiffLine {
                        line_type: LineType::Add, content: "x".to_string(),
                        old_num: None, new_num: Some(1),
                    }],
                }],
                adds: 1, dels: 0, compacted: false, raw_hunk_count: 0,
            },
            DiffFile {
                path: "src/main.rs".to_string(),
                status: FileStatus::Modified,
                hunks: vec![],
                adds: 0, dels: 0, compacted: false, raw_hunk_count: 0,
            },
        ];
        let config = CompactionConfig::default();
        compact_files(&mut files, &config);
        assert!(files[0].compacted);
        assert_eq!(files[0].raw_hunk_count, 1);
        assert!(files[0].hunks.is_empty());
        assert!(!files[1].compacted);
    }

    #[test]
    fn compact_files_by_size_threshold() {
        let many_lines: Vec<DiffLine> = (0..1100).map(|i| DiffLine {
            line_type: LineType::Add, content: format!("line {}", i),
            old_num: None, new_num: Some(i),
        }).collect();
        let mut files = vec![DiffFile {
            path: "src/big_file.rs".to_string(),
            status: FileStatus::Modified,
            hunks: vec![DiffHunk {
                header: "@@ -1,1 +1,1100 @@".to_string(),
                old_start: 1, old_count: 1, new_start: 1, new_count: 1100,
                lines: many_lines,
            }],
            adds: 1100, dels: 0, compacted: false, raw_hunk_count: 0,
        }];
        let config = CompactionConfig::default();
        compact_files(&mut files, &config);
        assert!(files[0].compacted);
        assert_eq!(files[0].raw_hunk_count, 1);
    }

    #[test]
    fn compact_files_disabled_does_nothing() {
        let mut files = vec![DiffFile {
            path: "Cargo.lock".to_string(),
            status: FileStatus::Modified,
            hunks: vec![DiffHunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                old_start: 1, old_count: 1, new_start: 1, new_count: 1,
                lines: vec![DiffLine {
                    line_type: LineType::Add, content: "x".to_string(),
                    old_num: None, new_num: Some(1),
                }],
            }],
            adds: 1, dels: 0, compacted: false, raw_hunk_count: 0,
        }];
        let config = CompactionConfig { enabled: false, ..Default::default() };
        compact_files(&mut files, &config);
        assert!(!files[0].compacted);
    }

    // ── parse_diff_headers ──

    #[test]
    fn parse_diff_headers_single_file() {
        let raw = "diff --git a/src/main.rs b/src/main.rs\nindex abc..def 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n+use std::io;\n fn main() {\n }\n";
        let headers = parse_diff_headers(raw);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].path, "src/main.rs");
        assert_eq!(headers[0].adds, 1);
        assert_eq!(headers[0].dels, 0);
        assert_eq!(headers[0].hunk_count, 1);
        assert!(headers[0].byte_length > 0);
    }

    #[test]
    fn parse_diff_headers_multiple_files() {
        let raw = "diff --git a/foo.rs b/foo.rs\n@@ -1,1 +1,2 @@\n+line1\n fn orig() {}\ndiff --git a/bar.rs b/bar.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let headers = parse_diff_headers(raw);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].path, "foo.rs");
        assert_eq!(headers[0].adds, 1);
        assert_eq!(headers[1].path, "bar.rs");
        assert_eq!(headers[1].adds, 1);
        assert_eq!(headers[1].dels, 1);
    }

    #[test]
    fn parse_diff_headers_empty_diff() {
        let headers = parse_diff_headers("");
        assert!(headers.is_empty());
    }

    #[test]
    fn parse_diff_headers_new_file_status() {
        let raw = "diff --git a/new.rs b/new.rs\nnew file mode 100644\n--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1,1 @@\n+hello\n";
        let headers = parse_diff_headers(raw);
        assert_eq!(headers.len(), 1);
        assert!(matches!(headers[0].status, FileStatus::Added));
    }

    #[test]
    fn parse_diff_headers_deleted_file_status() {
        let raw = "diff --git a/old.rs b/old.rs\ndeleted file mode 100644\n--- a/old.rs\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-goodbye\n";
        let headers = parse_diff_headers(raw);
        assert_eq!(headers.len(), 1);
        assert!(matches!(headers[0].status, FileStatus::Deleted));
    }

    // ── parse_file_at_offset ──

    #[test]
    fn parse_file_at_offset_extracts_hunks() {
        let raw = "diff --git a/foo.rs b/foo.rs\nindex abc..def 100644\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1,1 +1,2 @@\n context\n+added line\n";
        let headers = parse_diff_headers(raw);
        assert_eq!(headers.len(), 1);
        let file = parse_file_at_offset(raw, &headers[0]);
        assert_eq!(file.hunks.len(), 1);
        assert_eq!(file.hunks[0].lines.len(), 2);
    }

    #[test]
    fn parse_file_at_offset_from_multi_file_diff() {
        let raw = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,2 @@\n ctx\n+added\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1,2 +1,1 @@\n ctx\n-removed\n";
        let headers = parse_diff_headers(raw);
        assert_eq!(headers.len(), 2);

        let file_a = parse_file_at_offset(raw, &headers[0]);
        assert_eq!(file_a.path, "a.rs");
        assert!(!file_a.hunks.is_empty());

        let file_b = parse_file_at_offset(raw, &headers[1]);
        assert_eq!(file_b.path, "b.rs");
        assert!(!file_b.hunks.is_empty());
    }

    // ── header_to_stub ──

    #[test]
    fn header_to_stub_creates_empty_hunks_file() {
        let header = DiffFileHeader {
            path: "test.rs".to_string(),
            status: FileStatus::Modified,
            adds: 5,
            dels: 3,
            hunk_count: 2,
            byte_offset: 0,
            byte_length: 100,
        };
        let file = header_to_stub(&header);
        assert_eq!(file.path, "test.rs");
        assert_eq!(file.adds, 5);
        assert_eq!(file.dels, 3);
        assert_eq!(file.raw_hunk_count, 2);
        assert!(file.hunks.is_empty());
    }
}
