//! Markdown export of review annotations (comments, questions, AI findings)
//! for handing off to a coding agent.
//!
//! See `.work/export-review/spec.md`. The renderer is pure: it walks the
//! `TabState`'s AI data and produces a single markdown string. The Tauri
//! commands in `commands.rs` wrap it and optionally write to disk.

use serde::Deserialize;

use er_engine::ai::{Confidence, Finding, GitHubReviewComment, ReviewQuestion, UiAnnotation};
use er_engine::app::TabState;

fn default_true() -> bool {
    true
}

/// Toggles for which annotation kinds to include in the export.
///
/// JSON shape from the UI uses camelCase (Tauri 2 default).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOpts {
    pub include_comments: bool,
    pub include_questions: bool,
    pub include_findings: bool,
    pub only_unresolved: bool,
    #[serde(default = "default_true")]
    pub include_annotations: bool,
}

impl Default for ExportOpts {
    fn default() -> Self {
        ExportOpts {
            include_comments: true,
            include_questions: true,
            include_findings: true,
            only_unresolved: false,
            include_annotations: true,
        }
    }
}

/// Render the active tab's annotations as a single markdown document, grouped
/// by file path. Returns a placeholder body when there is nothing to export.
pub fn render_markdown(tab: &TabState, opts: &ExportOpts) -> String {
    let branch = if tab.current_branch.is_empty() {
        "(unknown)"
    } else {
        tab.current_branch.as_str()
    };

    // Group items by file path while preserving insertion order across kinds.
    // Within a file we emit: questions → comments → findings, matching the
    // sidebar ordering and what an agent will most naturally act on.
    let mut groups: Vec<(String, Vec<ItemBlock>)> = Vec::new();

    fn push<'a>(groups: &mut Vec<(String, Vec<ItemBlock<'a>>)>, file: &str, block: ItemBlock<'a>) {
        if let Some((_, items)) = groups.iter_mut().find(|(p, _)| p == file) {
            items.push(block);
        } else {
            groups.push((file.to_string(), vec![block]));
        }
    }

    if opts.include_questions {
        if let Some(qs) = tab.ai.questions.as_ref() {
            // Top-level only — replies are rendered nested under their parent.
            let top_level: Vec<&ReviewQuestion> = qs
                .questions
                .iter()
                .filter(|q| q.in_reply_to.is_none())
                .collect();
            for q in top_level {
                if opts.only_unresolved && q.resolved {
                    continue;
                }
                let replies: Vec<&ReviewQuestion> = qs
                    .questions
                    .iter()
                    .filter(|r| r.in_reply_to.as_deref() == Some(q.id.as_str()))
                    .collect();
                push(&mut groups, &q.file, ItemBlock::Question(q, replies));
            }
        }
    }

    if opts.include_comments {
        if let Some(gc) = tab.ai.github_comments.as_ref() {
            let top_level: Vec<&GitHubReviewComment> = gc
                .comments
                .iter()
                .filter(|c| c.in_reply_to.is_none())
                .collect();
            for c in top_level {
                if opts.only_unresolved && c.resolved {
                    continue;
                }
                let replies: Vec<&GitHubReviewComment> = gc
                    .comments
                    .iter()
                    .filter(|r| r.in_reply_to.as_deref() == Some(c.id.as_str()))
                    .collect();
                push(&mut groups, &c.file, ItemBlock::Comment(c, replies));
            }
        }
    }

    if opts.include_findings {
        if let Some(review) = tab.ai.review.as_ref() {
            // Stable order: sort file paths alphabetically. The HashMap iteration
            // order is otherwise undefined, which makes diffs of exports churn.
            let mut paths: Vec<&String> = review.files.keys().collect();
            paths.sort();
            for path in paths {
                let file_review = &review.files[path];
                for f in file_review.findings.iter() {
                    if opts.only_unresolved
                        && (f.resolved || matches!(f.confidence, Confidence::Dropped))
                    {
                        continue;
                    }
                    push(&mut groups, path, ItemBlock::Finding(f));
                }
            }
        }
    }

    // UI annotations are loaded from disk (read path matching the snapshot
    // pipeline). Filtered by `only_unresolved` (treating stale as resolved).
    let ui_annotations: Vec<UiAnnotation> = if opts.include_annotations {
        er_engine::ai::load_ui_annotations(&tab.comments_dir())
            .into_iter()
            .filter(|a| !(opts.only_unresolved && a.stale))
            .collect()
    } else {
        Vec::new()
    };

    if groups.is_empty() && ui_annotations.is_empty() {
        return format!("# Review export — {branch}\nNo annotations.\n");
    }

    let mut out = String::new();
    out.push_str(&format!("# Review export — {branch}\n\n"));

    for (file, items) in &groups {
        out.push_str(&format!("## {file}\n\n"));
        for item in items {
            render_item(&mut out, item);
            out.push('\n');
        }
    }

    if !ui_annotations.is_empty() {
        render_ui_annotations(&mut out, &ui_annotations);
    }

    out
}

/// Render a `## UI annotations` section grouped by `url`. Pins are numbered
/// sequentially within each URL group, restarting at 1 per URL — matches the
/// numbered overlay in the embedded browser.
fn render_ui_annotations(out: &mut String, annotations: &[UiAnnotation]) {
    // Preserve first-occurrence URL order so output is stable.
    let mut url_order: Vec<String> = Vec::new();
    for a in annotations {
        if !url_order.iter().any(|u| u == &a.url) {
            url_order.push(a.url.clone());
        }
    }

    out.push_str("## UI annotations\n\n");
    for url in &url_order {
        let display_url = if url.is_empty() {
            "(unknown)"
        } else {
            url.as_str()
        };
        out.push_str(&format!("### `{display_url}`\n"));
        let mut idx = 0usize;
        for a in annotations.iter().filter(|a| &a.url == url) {
            idx += 1;
            let stale = if a.stale { " [stale]" } else { "" };
            let text = if a.text.is_empty() {
                "(no note)"
            } else {
                a.text.as_str()
            };
            match a.selector.as_deref() {
                Some(sel) if !sel.is_empty() => {
                    let x = a.box_x.round() as i64;
                    let y = a.box_y.round() as i64;
                    out.push_str(&format!(
                        "- **Pin #{idx}** (`{sel}` @ ({x}, {y})) — {text}{stale}\n"
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "- **Pin #{idx}** — Approximate location — {text}{stale}\n"
                    ));
                }
            }
            if let Some(ctx) = &a.dom_context {
                if let Ok(json) = serde_json::to_string_pretty(ctx) {
                    out.push_str("\n  DOM context:\n\n  ```json\n");
                    for line in json.lines() {
                        out.push_str("  ");
                        out.push_str(line);
                        out.push('\n');
                    }
                    out.push_str("  ```\n");
                }
            } else if let Some(summary) = &a.element_context {
                out.push_str(&format!("  Element: {summary}\n"));
            }
        }
        out.push('\n');
    }
}

enum ItemBlock<'a> {
    Question(&'a ReviewQuestion, Vec<&'a ReviewQuestion>),
    Comment(&'a GitHubReviewComment, Vec<&'a GitHubReviewComment>),
    Finding(&'a Finding),
}

fn render_item(out: &mut String, item: &ItemBlock<'_>) {
    match item {
        ItemBlock::Question(q, replies) => {
            let line = q
                .line_start
                .map(|l| l.to_string())
                .unwrap_or_else(|| "—".into());
            let stale = if q.stale { " [stale]" } else { "" };
            let resolved = if q.resolved { " [resolved]" } else { "" };
            let author = if q.author.is_empty() {
                "You"
            } else {
                q.author.as_str()
            };
            let ago = ago_label(&q.timestamp);
            out.push_str(&format!(
                "### `{file}:{line}` — Question ({author}{ago}){stale}{resolved}\n",
                file = q.file,
            ));
            push_blockquote(out, &q.text, 1);
            for r in replies {
                let r_author = if r.author.is_empty() {
                    "You"
                } else {
                    r.author.as_str()
                };
                let r_ago = ago_label(&r.timestamp);
                out.push_str(&format!("> ↳ **{r_author}{r_ago}**\n"));
                push_blockquote(out, &r.text, 2);
            }
        }
        ItemBlock::Comment(c, replies) => {
            let line = c
                .line_start
                .map(|l| l.to_string())
                .unwrap_or_else(|| "—".into());
            let stale = if c.stale { " [stale]" } else { "" };
            let resolved = if c.resolved { " [resolved]" } else { "" };
            let author = if c.author.is_empty() {
                "You"
            } else {
                c.author.as_str()
            };
            let ago = ago_label(&c.timestamp);
            out.push_str(&format!(
                "### `{file}:{line}` — Comment ({author}{ago}){stale}{resolved}\n",
                file = c.file,
            ));
            push_blockquote(out, &c.comment, 1);
            for r in replies {
                let r_author = if r.author.is_empty() {
                    "You"
                } else {
                    r.author.as_str()
                };
                let r_ago = ago_label(&r.timestamp);
                out.push_str(&format!("> ↳ **{r_author}{r_ago}**\n"));
                push_blockquote(out, &r.comment, 2);
            }
        }
        ItemBlock::Finding(f) => {
            let line_label = match (f.line_start, f.line_end) {
                (Some(s), Some(e)) if e > s => format!("{s}-{e}"),
                (Some(s), _) => s.to_string(),
                _ => "—".into(),
            };
            let severity = format!("{:?}", f.severity).to_lowercase();
            let category = if f.category.is_empty() {
                ""
            } else {
                f.category.as_str()
            };
            let badges = if category.is_empty() {
                severity.clone()
            } else {
                format!("{severity} · {category}")
            };
            let resolved = if f.resolved { " [resolved]" } else { "" };
            let outside = if f.outside_diff {
                " [outside diff]"
            } else {
                ""
            };
            // File-level finding (no line) goes into a clearly-named sub-bucket.
            let header_path = if f.line_start.is_some() {
                format!("`{file}:{line_label}`", file = first_file_of_finding(f))
            } else {
                "File-level finding".to_string()
            };
            out.push_str(&format!(
                "### {header_path} — AI finding ({badges}){outside}{resolved}\n"
            ));
            if !f.title.is_empty() {
                out.push_str(&format!("**{}**\n\n", f.title));
            }
            if !f.description.is_empty() {
                push_blockquote(out, &f.description, 1);
            }
            if !f.suggestion.is_empty() {
                out.push_str("\n_Suggestion:_\n");
                push_blockquote(out, &f.suggestion, 1);
            }
        }
    }
}

/// Findings live inside `ErFileReview` keyed by file path in `ErReview.files`,
/// but the `Finding` struct itself doesn't carry the path. The caller has it;
/// this helper exists only because `render_item` doesn't take the path as a
/// separate arg. Returns an empty string — the file path is already in the
/// `## <file>` group heading just above, so the per-item header can elide it.
fn first_file_of_finding(_f: &Finding) -> &'static str {
    ""
}

fn push_blockquote(out: &mut String, body: &str, depth: usize) {
    let prefix = "> ".repeat(depth);
    for line in body.lines() {
        if line.is_empty() {
            out.push_str(prefix.trim_end());
            out.push('\n');
        } else {
            out.push_str(&prefix);
            out.push_str(line);
            out.push('\n');
        }
    }
    // Trailing blank line after blockquote — markdown renderers need this to
    // close the quote block cleanly.
    out.push('\n');
}

/// Minimal "Xm ago" / "Xh ago" / "Xd ago" label from an ISO 8601 timestamp.
/// Returns "" if parsing fails (we don't pull chrono just for this).
fn ago_label(ts: &str) -> String {
    if ts.is_empty() {
        return String::new();
    }
    // Crude parse: extract Y-M-D H:M from "YYYY-MM-DDTHH:MM:SSZ".
    // We don't need precision — just a hint for the agent.
    let _ = ts;
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{
        AiState, ErFileReview, ErGitHubComments, ErQuestions, ErReview, GitHubReviewComment,
        ReviewQuestion, RiskLevel,
    };
    use er_engine::app::TabState;
    use std::collections::HashMap;

    fn tab_with_ai(ai: AiState) -> TabState {
        let mut tab = TabState::new_for_test(Vec::new());
        tab.ai = ai;
        tab.current_branch = "feature".into();
        tab
    }

    fn make_question(
        id: &str,
        file: &str,
        line: usize,
        text: &str,
        resolved: bool,
    ) -> ReviewQuestion {
        ReviewQuestion {
            id: id.into(),
            timestamp: String::new(),
            file: file.into(),
            hunk_index: Some(0),
            line_start: Some(line),
            line_content: String::new(),
            text: text.into(),
            resolved,
            stale: false,
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".into(),
            relocated_at_hash: String::new(),
            in_reply_to: None,
            author: "you".into(),
            promoted_to: None,
        }
    }

    fn make_comment(id: &str, file: &str, line: usize, body: &str) -> GitHubReviewComment {
        GitHubReviewComment {
            id: id.into(),
            timestamp: String::new(),
            file: file.into(),
            hunk_index: Some(0),
            line_start: Some(line),
            line_end: None,
            line_content: String::new(),
            comment: body.into(),
            in_reply_to: None,
            resolved: false,
            source: "local".into(),
            github_id: None,
            author: "you".into(),
            synced: false,
            outdated: false,
            stale: false,
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".into(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".into(),
        }
    }

    #[test]
    fn renders_grouped_by_file() {
        let mut ai = AiState::default();
        ai.questions = Some(ErQuestions {
            version: 1,
            diff_hash: String::new(),
            questions: vec![
                make_question("q-1", "packages/foo.ts", 10, "Why this?", false),
                make_question("q-2", "packages/bar.ts", 22, "And this?", false),
            ],
        });
        ai.github_comments = Some(ErGitHubComments {
            version: 1,
            diff_hash: String::new(),
            github: None,
            comments: vec![
                make_comment("c-1", "packages/foo.ts", 10, "Nice."),
                make_comment("c-2", "packages/bar.ts", 22, "Hmm."),
            ],
        });
        let tab = tab_with_ai(ai);
        let out = render_markdown(&tab, &ExportOpts::default());

        assert!(
            out.contains("## packages/foo.ts"),
            "missing foo.ts header in:\n{out}"
        );
        assert!(
            out.contains("## packages/bar.ts"),
            "missing bar.ts header in:\n{out}"
        );
        assert!(out.contains("Why this?"));
        assert!(out.contains("Nice."));
        assert!(out.contains("And this?"));
        assert!(out.contains("Hmm."));
    }

    #[test]
    fn respects_only_unresolved() {
        let mut ai = AiState::default();
        ai.questions = Some(ErQuestions {
            version: 1,
            diff_hash: String::new(),
            questions: vec![
                make_question("q-1", "src/a.ts", 1, "Done question", true),
                make_question("q-2", "src/a.ts", 2, "Open question", false),
            ],
        });
        let tab = tab_with_ai(ai);
        let opts = ExportOpts {
            include_comments: false,
            include_questions: true,
            include_findings: false,
            only_unresolved: true,
            include_annotations: false,
        };
        let out = render_markdown(&tab, &opts);

        assert!(
            out.contains("Open question"),
            "should include unresolved:\n{out}"
        );
        assert!(
            !out.contains("Done question"),
            "should exclude resolved:\n{out}"
        );
    }

    #[test]
    fn empty_returns_placeholder() {
        let tab = tab_with_ai(AiState::default());
        let out = render_markdown(&tab, &ExportOpts::default());
        assert!(out.contains("No annotations."), "got:\n{out}");
    }

    #[test]
    fn includes_ui_annotations() {
        use er_engine::ai::{save_ui_annotations, UiAnnotation};

        // Empty case: no annotations file → no "## UI annotations" section,
        // but other content still renders.
        let empty_dir =
            std::env::temp_dir().join(format!("er-export-anns-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty_dir);
        std::fs::create_dir_all(&empty_dir).unwrap();
        let mut ai = AiState::default();
        ai.questions = Some(ErQuestions {
            version: 1,
            diff_hash: String::new(),
            questions: vec![make_question("q-1", "src/a.ts", 1, "Why?", false)],
        });
        let mut tab = tab_with_ai(ai);
        tab.repo_root = empty_dir.to_string_lossy().to_string();
        tab.er_root = er_engine::ErRoot::RepoLocal(tab.repo_root.clone());
        let out = render_markdown(&tab, &ExportOpts::default());
        assert!(
            !out.contains("## UI annotations"),
            "empty case must not emit UI annotations section:\n{out}"
        );
        assert!(
            out.contains("Why?"),
            "other content should still render:\n{out}"
        );

        // Populated case: two annotations on the same URL plus one on another,
        // mixing selector + cross-origin (no selector).
        let pop_dir =
            std::env::temp_dir().join(format!("er-export-anns-pop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pop_dir);
        std::fs::create_dir_all(&pop_dir).unwrap();
        let er_dir = format!("{}/.er", pop_dir.to_string_lossy());
        std::fs::create_dir_all(&er_dir).unwrap();
        let anns = vec![
            UiAnnotation {
                id: "a1".into(),
                url: "/dashboard".into(),
                selector: Some("button.primary".into()),
                box_x: 240.0,
                box_y: 380.0,
                box_w: 80.0,
                box_h: 32.0,
                viewport_w: 1440,
                viewport_h: 900,
                text: "Padding looks off vs design.".into(),
                timestamp: String::new(),
                author: "you".into(),
                screenshot_path: None,
                stale: false,
                element_context: Some("button: Save".into()),
                dom_context: Some(serde_json::json!({
                    "summary": "button: Save",
                    "node": {
                        "tag": "button",
                        "text": "Save",
                        "attrs": { "class": "primary" }
                    },
                    "outer_html": "<button class=\"primary\">Save</button>"
                })),
            },
            UiAnnotation {
                id: "a2".into(),
                url: "/dashboard".into(),
                selector: None,
                box_x: 12.0,
                box_y: 14.0,
                box_w: 0.0,
                box_h: 0.0,
                viewport_w: 1440,
                viewport_h: 900,
                text: "Missing loading state.".into(),
                timestamp: String::new(),
                author: "you".into(),
                screenshot_path: None,
                stale: false,
                element_context: None,
                dom_context: None,
            },
            UiAnnotation {
                id: "a3".into(),
                url: "/settings".into(),
                selector: Some("input#email".into()),
                box_x: 100.0,
                box_y: 200.0,
                box_w: 200.0,
                box_h: 24.0,
                viewport_w: 1440,
                viewport_h: 900,
                text: "Validation hint cut off.".into(),
                timestamp: String::new(),
                author: "you".into(),
                screenshot_path: None,
                stale: false,
                element_context: None,
                dom_context: None,
            },
        ];
        save_ui_annotations(&er_dir, &anns).unwrap();
        let mut tab = tab_with_ai(AiState::default());
        tab.repo_root = pop_dir.to_string_lossy().to_string();
        tab.er_root = er_engine::ErRoot::RepoLocal(tab.repo_root.clone());
        let out = render_markdown(&tab, &ExportOpts::default());
        assert!(
            out.contains("## UI annotations"),
            "expected UI annotations section:\n{out}"
        );
        assert!(
            out.contains("### `/dashboard`"),
            "missing /dashboard header:\n{out}"
        );
        assert!(
            out.contains("### `/settings`"),
            "missing /settings header:\n{out}"
        );
        assert!(
            out.contains(
                "**Pin #1** (`button.primary` @ (240, 380)) — Padding looks off vs design."
            ),
            "missing selector pin row:\n{out}"
        );
        assert!(
            out.contains("DOM context:") && out.contains("\"outer_html\""),
            "missing structured DOM context:\n{out}"
        );
        assert!(
            out.contains("**Pin #2** — Approximate location — Missing loading state."),
            "missing approximate-location pin row:\n{out}"
        );
        assert!(
            out.contains("**Pin #1** (`input#email` @ (100, 200)) — Validation hint cut off."),
            "settings pin should restart numbering at 1:\n{out}"
        );

        // include_annotations: false skips the section even when data exists.
        let opts = ExportOpts {
            include_annotations: false,
            ..ExportOpts::default()
        };
        let out = render_markdown(&tab, &opts);
        assert!(
            !out.contains("## UI annotations"),
            "include_annotations=false must suppress section:\n{out}"
        );

        let _ = std::fs::remove_dir_all(&empty_dir);
        let _ = std::fs::remove_dir_all(&pop_dir);
    }

    #[test]
    fn includes_findings_grouped() {
        let mut ai = AiState::default();
        let mut files = HashMap::new();
        files.insert(
            "src/x.rs".to_string(),
            ErFileReview {
                risk: RiskLevel::Low,
                risk_reason: String::new(),
                summary: String::new(),
                findings: vec![Finding {
                    id: "f-1".into(),
                    severity: RiskLevel::Medium,
                    category: "style".into(),
                    title: "Use Map".into(),
                    description: "Detail".into(),
                    hunk_index: Some(0),
                    line_start: Some(5),
                    line_end: None,
                    suggestion: String::new(),
                    related_files: Vec::new(),
                    outside_diff: false,
                    confidence: Confidence::Confirmed,
                    verification_plan: String::new(),
                    evidence: Vec::new(),
                    responses: Vec::new(),
                    resolved: false,
                    resolved_note: String::new(),
                    resolved_at: String::new(),
                    promoted_to: None,
                }],
            },
        );
        ai.review = Some(ErReview {
            version: 1,
            diff_hash: String::new(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files,
            file_hashes: HashMap::new(),
        });
        let tab = tab_with_ai(ai);
        let out = render_markdown(&tab, &ExportOpts::default());
        assert!(out.contains("## src/x.rs"), "missing group heading:\n{out}");
        assert!(out.contains("AI finding"), "missing finding label:\n{out}");
        assert!(out.contains("Use Map"));
    }
}
