use crate::git::{DiffFile, LineType};

/// Anchor data extracted from a comment for relocation matching
pub struct CommentAnchor {
    pub file: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    pub line_content: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
    pub old_line_start: Option<usize>,
    pub hunk_header: String,
}

pub enum RelocationResult {
    /// Line found at same position with same content
    Unchanged,
    /// Line found at a new position
    Relocated { new_hunk_index: usize, new_line_start: usize },
    /// Line was deleted or cannot be found
    Lost,
}

/// Try to relocate a comment to its new position in the updated diff.
pub fn relocate_comment(anchor: &CommentAnchor, diff_file: &DiffFile) -> RelocationResult {
    // Hunk-level comments: match by hunk header
    if anchor.line_start.is_none() {
        return relocate_hunk_level(anchor, diff_file);
    }

    // Pass 1: exact content + same line number
    if let Some(result) = pass1_exact(anchor, diff_file) {
        return result;
    }

    // Pass 2: content + context scoring
    if let Some(result) = pass2_scored(anchor, diff_file) {
        return result;
    }

    // Pass 3: fuzzy context match (line was edited)
    if let Some(result) = pass3_fuzzy(anchor, diff_file) {
        return result;
    }

    RelocationResult::Lost
}

fn pass1_exact(anchor: &CommentAnchor, diff_file: &DiffFile) -> Option<RelocationResult> {
    let target_line = anchor.line_start?;

    // Collect ALL matching lines — only act if there's exactly one (or we find the exact position)
    let mut matches: Vec<(usize, usize)> = Vec::new(); // (hunk_idx, new_num)

    for (hunk_idx, hunk) in diff_file.hunks.iter().enumerate() {
        for dl in &hunk.lines {
            if dl.line_type == LineType::Delete {
                continue;
            }
            if dl.content != anchor.line_content {
                continue;
            }
            if let Some(new_num) = dl.new_num {
                if new_num == target_line {
                    // Exact position match — always Unchanged regardless of duplicates
                    return Some(RelocationResult::Unchanged);
                }
                matches.push((hunk_idx, new_num));
            }
        }
    }

    // Only return Relocated from pass1 if the content is unique
    if matches.len() == 1 {
        Some(RelocationResult::Relocated {
            new_hunk_index: matches[0].0,
            new_line_start: matches[0].1,
        })
    } else {
        // Multiple or zero matches — let pass2 score them with context
        None
    }
}

fn pass2_scored(anchor: &CommentAnchor, diff_file: &DiffFile) -> Option<RelocationResult> {
    let target_line = anchor.line_start?;
    let mut best_score = 2i32; // minimum score to consider
    let mut best: Option<(usize, usize)> = None;

    for (hunk_idx, hunk) in diff_file.hunks.iter().enumerate() {
        for (line_idx, dl) in hunk.lines.iter().enumerate() {
            if dl.line_type == LineType::Delete {
                continue;
            }
            if dl.content != anchor.line_content {
                continue;
            }
            let new_num = match dl.new_num {
                Some(n) => n,
                None => continue,
            };

            let mut score: i32 = 0;

            // Context before: up to 3 lines
            for (offset, ctx) in anchor.context_before.iter().rev().enumerate() {
                if line_idx >= offset + 1 {
                    if hunk.lines[line_idx - offset - 1].content == *ctx {
                        score += 1;
                    }
                }
            }

            // Context after: up to 3 lines
            for (offset, ctx) in anchor.context_after.iter().enumerate() {
                let after_idx = line_idx + offset + 1;
                if after_idx < hunk.lines.len() {
                    if hunk.lines[after_idx].content == *ctx {
                        score += 1;
                    }
                }
            }

            // old_line_start match
            if let (Some(anchor_old), Some(dl_old)) = (anchor.old_line_start, dl.old_num) {
                if anchor_old == dl_old {
                    score += 2;
                }
            }

            // Hunk header match
            if !anchor.hunk_header.is_empty() && hunk.header == anchor.hunk_header {
                score += 1;
            }

            // Proximity to original line_start
            let dist = (new_num as i64 - target_line as i64).unsigned_abs() as usize;
            if dist <= 10 {
                score += 1;
            }

            if score > best_score {
                best_score = score;
                best = Some((hunk_idx, new_num));
            }
        }
    }

    best.map(|(hunk_idx, new_line_start)| RelocationResult::Relocated {
        new_hunk_index: hunk_idx,
        new_line_start,
    })
}

fn pass3_fuzzy(anchor: &CommentAnchor, diff_file: &DiffFile) -> Option<RelocationResult> {
    // Only useful when we have context
    let total_context = anchor.context_before.len() + anchor.context_after.len();
    if total_context == 0 {
        return None;
    }

    // Require at least 2/3 of available context lines to match
    // For 2 context lines: need > 1 (i.e. both)
    // For 3 context lines: need > 1 (i.e. >= 2)
    // For 6 context lines: need > 3 (i.e. >= 4)
    let min_required = ((total_context * 2 + 2) / 3).max(1);
    let mut best_score = min_required.saturating_sub(1); // > this to qualify
    let mut best: Option<(usize, usize)> = None;

    for (hunk_idx, hunk) in diff_file.hunks.iter().enumerate() {
        for (line_idx, dl) in hunk.lines.iter().enumerate() {
            if dl.line_type == LineType::Delete {
                continue;
            }
            let new_num = match dl.new_num {
                Some(n) => n,
                None => continue,
            };

            let mut ctx_matches = 0usize;

            for (offset, ctx) in anchor.context_before.iter().rev().enumerate() {
                if line_idx >= offset + 1 {
                    if hunk.lines[line_idx - offset - 1].content == *ctx {
                        ctx_matches += 1;
                    }
                }
            }

            for (offset, ctx) in anchor.context_after.iter().enumerate() {
                let after_idx = line_idx + offset + 1;
                if after_idx < hunk.lines.len() {
                    if hunk.lines[after_idx].content == *ctx {
                        ctx_matches += 1;
                    }
                }
            }

            if ctx_matches > best_score {
                best_score = ctx_matches;
                best = Some((hunk_idx, new_num));
            }
        }
    }

    best.map(|(hunk_idx, new_line_start)| RelocationResult::Relocated {
        new_hunk_index: hunk_idx,
        new_line_start,
    })
}

fn relocate_hunk_level(anchor: &CommentAnchor, diff_file: &DiffFile) -> RelocationResult {
    if anchor.hunk_header.is_empty() {
        // No header to verify identity — can't confirm the hunk at the same index is the same one
        let _ = diff_file;
        return RelocationResult::Lost;
    }

    // Try to find a hunk with matching header
    for (hunk_idx, hunk) in diff_file.hunks.iter().enumerate() {
        if hunk.header == anchor.hunk_header {
            let original_idx = anchor.hunk_index.unwrap_or(usize::MAX);
            if hunk_idx == original_idx {
                return RelocationResult::Unchanged;
            } else {
                return RelocationResult::Relocated {
                    new_hunk_index: hunk_idx,
                    new_line_start: hunk.new_start,
                };
            }
        }
    }

    // Try prefix match (hunk headers can differ in line counts after edits)
    for (hunk_idx, hunk) in diff_file.hunks.iter().enumerate() {
        // Extract the @@ prefix and context part for fuzzy comparison
        let anchor_ctx = extract_hunk_context(&anchor.hunk_header);
        let hunk_ctx = extract_hunk_context(&hunk.header);
        if !anchor_ctx.is_empty() && anchor_ctx == hunk_ctx {
            return RelocationResult::Relocated {
                new_hunk_index: hunk_idx,
                new_line_start: hunk.new_start,
            };
        }
    }

    RelocationResult::Lost
}

/// Extract the context string from a hunk header (the part after " @@ ")
fn extract_hunk_context(header: &str) -> &str {
    if let Some(idx) = header.find(" @@ ") {
        &header[idx + 4..]
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{DiffFile, DiffHunk, DiffLine, LineType, FileStatus};

    fn make_file(hunks: Vec<DiffHunk>) -> DiffFile {
        DiffFile {
            path: "test.rs".to_string(),
            status: FileStatus::Modified,
            hunks,
            adds: 0,
            dels: 0,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    fn make_hunk(header: &str, lines: Vec<DiffLine>) -> DiffHunk {
        DiffHunk {
            header: header.to_string(),
            old_start: 1,
            old_count: lines.len(),
            new_start: 1,
            new_count: lines.len(),
            lines,
        }
    }

    fn ctx_line(content: &str, old: usize, new: usize) -> DiffLine {
        DiffLine {
            line_type: LineType::Context,
            content: content.to_string(),
            old_num: Some(old),
            new_num: Some(new),
        }
    }

    fn add_line(content: &str, new: usize) -> DiffLine {
        DiffLine {
            line_type: LineType::Add,
            content: content.to_string(),
            old_num: None,
            new_num: Some(new),
        }
    }

    fn del_line(content: &str, old: usize) -> DiffLine {
        DiffLine {
            line_type: LineType::Delete,
            content: content.to_string(),
            old_num: Some(old),
            new_num: None,
        }
    }

    fn anchor(line_start: Option<usize>, content: &str, before: Vec<&str>, after: Vec<&str>) -> CommentAnchor {
        CommentAnchor {
            file: "test.rs".to_string(),
            hunk_index: Some(0),
            line_start,
            line_content: content.to_string(),
            context_before: before.iter().map(|s| s.to_string()).collect(),
            context_after: after.iter().map(|s| s.to_string()).collect(),
            old_line_start: None,
            hunk_header: String::new(),
        }
    }

    #[test]
    fn exact_match_same_position() {
        let file = make_file(vec![make_hunk("@@ -1,3 +1,3 @@", vec![
            ctx_line("fn foo() {", 1, 1),
            ctx_line("    let x = 1;", 2, 2),
            ctx_line("}", 3, 3),
        ])]);
        let a = anchor(Some(2), "    let x = 1;", vec!["fn foo() {"], vec!["}"]);
        let result = relocate_comment(&a, &file);
        assert!(matches!(result, RelocationResult::Unchanged));
    }

    #[test]
    fn exact_match_shifted() {
        // Target line shifted down by 2
        let file = make_file(vec![make_hunk("@@ -1,5 +1,5 @@", vec![
            add_line("// new comment", 1),
            add_line("// another", 2),
            ctx_line("fn foo() {", 3, 3),
            ctx_line("    let x = 1;", 4, 4),
            ctx_line("}", 5, 5),
        ])]);
        let a = anchor(Some(2), "    let x = 1;", vec!["fn foo() {"], vec!["}"]);
        let result = relocate_comment(&a, &file);
        match result {
            RelocationResult::Relocated { new_line_start, .. } => {
                assert_eq!(new_line_start, 4);
            }
            _ => panic!("Expected Relocated"),
        }
    }

    #[test]
    fn content_match_with_context_disambiguation() {
        // Two lines with same content "}" — context should disambiguate
        let file = make_file(vec![make_hunk("@@ -1,8 +1,8 @@", vec![
            ctx_line("fn bar() {", 1, 1),
            ctx_line("    x()", 2, 2),
            ctx_line("}", 3, 3),  // wrong one
            ctx_line("fn foo() {", 4, 4),
            ctx_line("    let x = 1;", 5, 5),
            ctx_line("}", 6, 6),  // right one (context_before = "    let x = 1;")
            ctx_line("fn baz() {", 7, 7),
            ctx_line("}", 8, 8),
        ])]);
        // Comment was on '}' at line 6, with context "    let x = 1;" before and "fn baz() {" after
        let a = anchor(Some(6), "}", vec!["    let x = 1;"], vec!["fn baz() {"]);
        let result = relocate_comment(&a, &file);
        match result {
            RelocationResult::Unchanged => {}
            RelocationResult::Relocated { new_line_start, .. } => {
                assert_eq!(new_line_start, 6);
            }
            RelocationResult::Lost => panic!("Should not be Lost"),
        }
    }

    #[test]
    fn line_deleted() {
        // Target line removed entirely
        let file = make_file(vec![make_hunk("@@ -1,2 +1,2 @@", vec![
            ctx_line("fn foo() {", 1, 1),
            ctx_line("}", 2, 2),
        ])]);
        let a = anchor(Some(3), "    let x = 1;", vec!["fn foo() {"], vec!["}"]);
        let result = relocate_comment(&a, &file);
        assert!(matches!(result, RelocationResult::Lost));
    }

    #[test]
    fn fuzzy_context_match() {
        // Target line was edited ("let x = 1;" → "let x = 2;") but context still matches
        let file = make_file(vec![make_hunk("@@ -1,3 +1,3 @@", vec![
            ctx_line("fn foo() {", 1, 1),
            ctx_line("    let x = 2;", 2, 2),  // edited line
            ctx_line("}", 3, 3),
        ])]);
        // Anchor had the old content
        let a = anchor(Some(2), "    let x = 1;", vec!["fn foo() {"], vec!["}"]);
        let result = relocate_comment(&a, &file);
        // Should relocate via fuzzy context (both context lines match)
        match result {
            RelocationResult::Relocated { new_line_start, .. } => {
                assert_eq!(new_line_start, 2);
            }
            RelocationResult::Lost => panic!("Should have found via fuzzy context"),
            RelocationResult::Unchanged => panic!("Content differs, cannot be Unchanged"),
        }
    }

    #[test]
    fn hunk_level_relocated() {
        let file = make_file(vec![
            make_hunk("@@ -1,3 +1,3 @@ fn first()", vec![
                ctx_line("fn first() {", 1, 1),
                ctx_line("}", 2, 2),
            ]),
            make_hunk("@@ -10,3 +10,3 @@ fn target()", vec![
                ctx_line("fn target() {", 10, 10),
                ctx_line("}", 11, 11),
            ]),
        ]);

        let a = CommentAnchor {
            file: "test.rs".to_string(),
            hunk_index: Some(0),
            line_start: None,
            line_content: String::new(),
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: "@@ -10,3 +10,3 @@ fn target()".to_string(),
        };
        let result = relocate_comment(&a, &file);
        match result {
            RelocationResult::Relocated { new_hunk_index, .. } => {
                assert_eq!(new_hunk_index, 1);
            }
            _ => panic!("Expected Relocated to hunk index 1"),
        }
    }

    #[test]
    fn no_context_fallback() {
        // Old comments without context should still work via content match
        let file = make_file(vec![make_hunk("@@ -1,3 +3,3 @@", vec![
            ctx_line("fn foo() {", 3, 3),
            ctx_line("    let x = 1;", 4, 4),
            ctx_line("}", 5, 5),
        ])]);
        // No context provided (empty vecs)
        let a = anchor(Some(4), "    let x = 1;", vec![], vec![]);
        let result = relocate_comment(&a, &file);
        // Content found — should find it (may be Unchanged or Relocated)
        assert!(!matches!(result, RelocationResult::Lost));
    }

    #[test]
    fn already_relocated_skipped() {
        // Test the anchor itself correctly identifies a match
        // (The hash check skip logic lives in state.rs, not here)
        let file = make_file(vec![make_hunk("@@ -1,3 +1,3 @@", vec![
            ctx_line("fn foo() {", 1, 1),
            ctx_line("    let x = 1;", 2, 2),
            ctx_line("}", 3, 3),
        ])]);
        let a = anchor(Some(2), "    let x = 1;", vec!["fn foo() {"], vec!["}"]);
        // Run relocate twice — both should give consistent results
        let r1 = relocate_comment(&a, &file);
        let r2 = relocate_comment(&a, &file);
        // Both should be Unchanged
        assert!(matches!(r1, RelocationResult::Unchanged));
        assert!(matches!(r2, RelocationResult::Unchanged));
    }
}
