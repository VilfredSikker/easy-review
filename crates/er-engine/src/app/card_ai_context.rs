//! System context for desktop card-level AI (Ask AI / Validate with AI).

use crate::git::{filter_raw_diff_by_paths, DiffFile, DiffHunk};
use crate::ai::prompts::CARD_AI_READ_BUDGET_LINE;

const MAX_CONTEXT_BYTES: usize = 16 * 1024;
const MAX_DIFF_EXCERPT_BYTES: usize = 12 * 1024;

/// Inputs for [`build_card_ai_system_context`].
pub struct CardAiContextParams<'a> {
    pub repo_root: &'a str,
    pub base_branch: &'a str,
    pub current_branch: &'a str,
    pub files: &'a [DiffFile],
    pub raw_diff: Option<&'a str>,
    pub file: &'a str,
    pub hunk_index: usize,
    pub line_start: Option<usize>,
    pub line_content: Option<&'a str>,
    pub thread_body: &'a str,
    pub finding_title: Option<&'a str>,
    pub finding_description: Option<&'a str>,
}

/// Build the system prompt for card-level AI: repo identity, diff excerpt, thread.
pub fn build_card_ai_system_context(p: &CardAiContextParams<'_>) -> String {
    let mut out = String::new();
    out.push_str("## Repository\n\n");
    out.push_str(&format!("- **repo_root:** `{}`\n", p.repo_root));
    out.push_str(&format!("- **branch:** `{}`\n", p.current_branch));
    out.push_str(&format!("- **base:** `{}`\n", p.base_branch));
    out.push_str(
        "\nYou are reviewing code in **this** repository. Do not assume the working directory is the easy-review app repo.\n\n",
    );
    out.push_str(CARD_AI_READ_BUDGET_LINE);
    out.push('\n');

    if let (Some(title), _) = (p.finding_title, p.finding_description) {
        out.push_str("\n## Linked finding\n\n");
        out.push_str(&format!("**Title:** {title}\n"));
        if let Some(desc) = p.finding_description {
            if !desc.trim().is_empty() {
                out.push_str(&format!("**Description:** {desc}\n"));
            }
        }
    }

    out.push_str("\n## Diff excerpt\n\n```diff\n");
    let excerpt = diff_excerpt_for_anchor(p);
    out.push_str(&excerpt);
    if !excerpt.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n");

    if let Some(lc) = p.line_content {
        if !lc.trim().is_empty() {
            out.push_str("\n## Anchor line content\n\n");
            out.push_str("```\n");
            out.push_str(lc);
            if !lc.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n");
        }
    }

    out.push_str("\n## Thread\n\n");
    out.push_str(p.thread_body);

    truncate_to_byte_cap(&mut out, MAX_CONTEXT_BYTES);
    out
}

fn diff_excerpt_for_anchor(p: &CardAiContextParams<'_>) -> String {
    if let Some(df) = p.files.iter().find(|f| f.path == p.file) {
        if !df.hunks.is_empty() {
            let text = if p.hunk_index < df.hunks.len() {
                df.hunks[p.hunk_index].to_text()
            } else {
                df.hunks
                    .iter()
                    .map(DiffHunk::to_text)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            return truncate_str_by_bytes(&text, MAX_DIFF_EXCERPT_BYTES);
        }
    }
    if let Some(raw) = p.raw_diff {
        let filtered = filter_raw_diff_by_paths(raw, &[p.file.to_string()]);
        if !filtered.is_empty() {
            return truncate_str_by_bytes(&filtered, MAX_DIFF_EXCERPT_BYTES);
        }
    }
    format!("(no diff excerpt available for `{}`)\n", p.file)
}

fn truncate_str_by_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut t = s[..end].to_string();
    t.push_str("\n… (truncated)\n");
    t
}

fn truncate_to_byte_cap(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut end = max.saturating_sub(40);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push_str("\n\n… (context truncated)\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{DiffFile, DiffHunk, DiffLine, FileStatus, LineType};

    fn sample_hunk() -> DiffHunk {
        DiffHunk {
            header: "@@ -1,1 +1,2 @@".to_string(),
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 2,
            lines: vec![
                DiffLine {
                    line_type: LineType::Context,
                    content: "fn main() {}".to_string(),
                    old_num: Some(1),
                    new_num: Some(1),
                },
                DiffLine {
                    line_type: LineType::Add,
                    content: "    println!(\"hi\");".to_string(),
                    old_num: None,
                    new_num: Some(2),
                },
            ],
        }
    }

    #[test]
    fn context_includes_repo_and_hunk() {
        let file = DiffFile {
            path: "src/lib.rs".to_string(),
            status: FileStatus::Modified,
            hunks: vec![sample_hunk()],
            adds: 1,
            dels: 0,
            compacted: false,
            raw_hunk_count: 1,
        };
        let p = CardAiContextParams {
            repo_root: "/tmp/my-repo",
            base_branch: "main",
            current_branch: "feature/x",
            files: std::slice::from_ref(&file),
            raw_diff: None,
            file: "src/lib.rs",
            hunk_index: 0,
            line_start: Some(2),
            line_content: Some("+    println!(\"hi\");"),
            thread_body: "Please validate.",
            finding_title: Some("Missing check"),
            finding_description: Some("Input not validated."),
        };
        let ctx = build_card_ai_system_context(&p);
        assert!(ctx.contains("/tmp/my-repo"));
        assert!(ctx.contains("feature/x"));
        assert!(ctx.contains("@@ -1,1 +1,2 @@"));
        assert!(ctx.contains("println"));
        assert!(ctx.contains("~10"));
        assert!(!ctx.contains("~30 total"));
        assert!(!ctx.contains("~50 total"));
        assert!(ctx.contains("Missing check"));
    }
}
