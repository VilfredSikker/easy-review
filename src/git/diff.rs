use super::status::FileStatus;

/// A single line in a diff hunk
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_type: LineType,
    pub content: String,
    pub old_num: Option<usize>,
    pub new_num: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
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

            // Extract path from "diff --git a/path b/path"
            let path = line
                .split(" b/")
                .last()
                .unwrap_or("")
                .to_string();

            current_file = Some(DiffFile {
                path,
                status: FileStatus::Modified, // will be refined below
                hunks: Vec::new(),
                adds: 0,
                dels: 0,
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
            if line.starts_with('+') {
                hunk.lines.push(DiffLine {
                    line_type: LineType::Add,
                    content: line[1..].to_string(),
                    old_num: None,
                    new_num: Some(new_line),
                });
                new_line += 1;
                if let Some(ref mut file) = current_file {
                    file.adds += 1;
                }
            } else if line.starts_with('-') {
                hunk.lines.push(DiffLine {
                    line_type: LineType::Delete,
                    content: line[1..].to_string(),
                    old_num: Some(old_line),
                    new_num: None,
                });
                old_line += 1;
                if let Some(ref mut file) = current_file {
                    file.dels += 1;
                }
            } else if line.starts_with(' ') || line.is_empty() {
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
}
