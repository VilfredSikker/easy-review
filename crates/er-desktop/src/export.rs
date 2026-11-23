//! Desktop face of the review export. The renderer lives in the engine; this
//! module only adds per-item exclusion.
//!
//! The Export view can leave out single items, but it only knows the items the
//! page lists, and the page omits some the engine still exports (resolved
//! questions and notes, arbiter-graded duplicate findings). An allow-list built
//! from what the UI sees would silently drop those, so the UI sends exclusions
//! instead and they are turned into the engine's allow-lists here, against the
//! tab's whole AI state.

use er_engine::ai::AiState;
use er_engine::app::TabState;
use er_engine::review_export::ExportOpts;
use er_protocol::ExportOptions;

/// The App's tab rendered with `options`: the path without the service.
pub fn render_markdown(tab: &TabState, options: &ExportOptions) -> String {
    er_engine::export::render_markdown(tab, &resolve_exclusions(&tab.ai, options))
}

/// A tab's published content rendered with `options`: its branch, its AI
/// section, and the UI annotations in its view bucket, `context.er_dir`.
pub fn render_content(content: &er_service::TabContent, options: &ExportOptions) -> String {
    let source = er_engine::review_export::ExportSource {
        branch: &content.tab.current_branch,
        ai: &content.ai.state,
        er_dir: &content.context.er_dir,
    };
    er_engine::review_export::render(&source, &resolve_exclusions(&content.ai.state, options))
}

fn keep_unexcluded<'a>(
    allow: Option<Vec<String>>,
    all: impl Iterator<Item = &'a str>,
    excluded: Option<&Vec<String>>,
) -> Option<Vec<String>> {
    let Some(excluded) = excluded.filter(|excluded| !excluded.is_empty()) else {
        return allow;
    };
    let base: Vec<String> = allow.unwrap_or_else(|| all.map(str::to_string).collect());
    Some(
        base.into_iter()
            .filter(|id| !excluded.contains(id))
            .collect(),
    )
}

/// The engine's options for `options`: a toggle left out keeps the engine's
/// default, and each exclusion list becomes an allow-list over `ai`.
fn resolve_exclusions(ai: &AiState, options: &ExportOptions) -> ExportOpts {
    let defaults = ExportOpts::default();
    let mut out = ExportOpts {
        include_comments: options
            .include_comments
            .unwrap_or(defaults.include_comments),
        include_questions: options
            .include_questions
            .unwrap_or(defaults.include_questions),
        include_notes: options.include_notes.unwrap_or(defaults.include_notes),
        include_findings: options
            .include_findings
            .unwrap_or(defaults.include_findings),
        only_unresolved: options.only_unresolved.unwrap_or(defaults.only_unresolved),
        include_annotations: options
            .include_annotations
            .unwrap_or(defaults.include_annotations),
        include_checklist: options
            .include_checklist
            .unwrap_or(defaults.include_checklist),
        include_comment_ids: options.include_comment_ids.clone(),
        include_question_ids: options.include_question_ids.clone(),
        include_note_ids: options.include_note_ids.clone(),
        include_finding_ids: options.include_finding_ids.clone(),
    };
    out.include_question_ids = keep_unexcluded(
        out.include_question_ids.take(),
        ai.questions
            .iter()
            .flat_map(|q| q.questions.iter().map(|q| q.id.as_str())),
        options.exclude_question_ids.as_ref(),
    );
    out.include_note_ids = keep_unexcluded(
        out.include_note_ids.take(),
        ai.notes
            .iter()
            .flat_map(|n| n.notes.iter().map(|n| n.id.as_str())),
        options.exclude_note_ids.as_ref(),
    );
    out.include_comment_ids = keep_unexcluded(
        out.include_comment_ids.take(),
        ai.github_comments
            .iter()
            .flat_map(|c| c.comments.iter().map(|c| c.id.as_str())),
        options.exclude_comment_ids.as_ref(),
    );
    out.include_finding_ids = keep_unexcluded(
        out.include_finding_ids.take(),
        ai.review.iter().flat_map(|r| {
            r.files
                .values()
                .flat_map(|f| f.findings.iter().map(|f| f.id.as_str()))
        }),
        options.exclude_finding_ids.as_ref(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{AiState, ErQuestions, PrivateAnnotation};

    fn question(id: &str, text: &str, resolved: bool) -> PrivateAnnotation {
        PrivateAnnotation {
            id: id.into(),
            timestamp: String::new(),
            file: "src/lib.rs".into(),
            hunk_index: Some(0),
            line_start: Some(3),
            line_end: None,
            line_content: String::new(),
            text: text.into(),
            resolved,
            stale: false,
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            side: "RIGHT".into(),
            hunk_header: String::new(),
            anchor_status: "original".into(),
            relocated_at_hash: String::new(),
            in_reply_to: None,
            author: "you".into(),
            promoted_to: None,
            finding_ref: None,
            ai_action: None,
        }
    }

    fn tab() -> TabState {
        let mut ai = AiState::default();
        ai.questions = Some(ErQuestions {
            version: 1,
            diff_hash: String::new(),
            questions: vec![
                question("q-open", "Still open?", false),
                question("q-drop", "Leave me out", false),
                question("q-done", "Already answered", true),
            ],
        });
        let mut tab = TabState::new_for_test(Vec::new());
        tab.ai = ai;
        tab
    }

    /// The snapshot never lists resolved questions, so the UI cannot name
    /// them; excluding a different question must not drop them.
    #[test]
    fn excluding_one_question_keeps_resolved_ones() {
        let opts: ExportOptions =
            serde_json::from_value(serde_json::json!({ "excludeQuestionIds": ["q-drop"] }))
                .expect("opts parse");
        let out = render_markdown(&tab(), &opts);

        assert!(out.contains("Still open?"), "{out}");
        assert!(out.contains("Already answered"), "{out}");
        assert!(!out.contains("Leave me out"), "{out}");
    }

    #[test]
    fn exclusions_respect_only_unresolved_and_engine_defaults() {
        let opts: ExportOptions = serde_json::from_value(serde_json::json!({
            "onlyUnresolved": true,
            "excludeQuestionIds": ["q-drop"],
        }))
        .expect("opts parse");
        assert!(
            resolve_exclusions(&AiState::default(), &opts).include_questions,
            "missing toggles keep engine defaults"
        );
        let out = render_markdown(&tab(), &opts);

        assert!(out.contains("Still open?"), "{out}");
        assert!(!out.contains("Already answered"), "{out}");
        assert!(!out.contains("Leave me out"), "{out}");
    }

    #[test]
    fn an_allow_list_still_applies_beside_exclusions() {
        let opts: ExportOptions = serde_json::from_value(serde_json::json!({
            "includeQuestionIds": ["q-open", "q-drop"],
            "excludeQuestionIds": ["q-drop"],
        }))
        .expect("opts parse");
        let out = render_markdown(&tab(), &opts);

        assert!(out.contains("Still open?"), "{out}");
        assert!(!out.contains("Already answered"), "{out}");
        assert!(!out.contains("Leave me out"), "{out}");
    }

    /// The payloads the page sends: the quick copy's one toggle, which keeps
    /// every other default, and the Export page's every toggle with its
    /// exclusions.
    #[test]
    fn the_pages_payloads_map_onto_the_engine_options() {
        let quick: ExportOptions =
            serde_json::from_value(serde_json::json!({ "includeChecklist": true }))
                .expect("quick copy");
        let opts = resolve_exclusions(&AiState::default(), &quick);
        let on = [
            opts.include_comments,
            opts.include_questions,
            opts.include_notes,
            opts.include_findings,
            opts.include_annotations,
            opts.include_checklist,
        ];
        assert_eq!(on, [true; 6]);
        assert!(!opts.only_unresolved);

        let page: ExportOptions = serde_json::from_value(serde_json::json!({
            "includeComments": false, "includeQuestions": true, "includeNotes": true,
            "includeFindings": true, "includeAnnotations": false, "includeChecklist": true,
            "onlyUnresolved": true, "excludeCommentIds": [], "excludeQuestionIds": ["q-drop"],
            "excludeNoteIds": [], "excludeFindingIds": []
        }))
        .expect("Export page");
        let opts = resolve_exclusions(&tab().ai, &page);
        assert!(!opts.include_comments && !opts.include_annotations && opts.only_unresolved);
        let kept = Some(vec!["q-open".to_string(), "q-done".to_string()]);
        assert_eq!(opts.include_question_ids, kept);
        assert_eq!(opts.include_note_ids, None);
    }

    /// The Export view counts items from the rendered markdown
    /// (`countExportedItems` in `desktop-ui/src/lib/exportOptions.ts`): one
    /// `### … — <Kind> (` heading per exported item. Pin the shape it reads.
    #[test]
    fn every_exported_item_gets_one_counted_heading() {
        let out = render_markdown(&tab(), &ExportOptions::default());
        let counted = out
            .lines()
            .filter(|l| {
                l.starts_with("### ")
                    && ["Question", "Note", "Comment", "AI finding"]
                        .iter()
                        .any(|k| l.contains(&format!(" — {k} (")))
            })
            .count();
        assert_eq!(counted, 3, "{out}");
    }
}
