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
}
