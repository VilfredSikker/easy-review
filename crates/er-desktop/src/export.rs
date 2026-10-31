//! Desktop face of `er_engine::export`. The renderer lives in the engine; this
//! module only adds per-item exclusion.
//!
//! The Export view can leave out single items, but it only knows the items the
//! snapshot lists, and the snapshot omits some the engine still exports
//! (resolved questions and notes, arbiter-graded duplicate findings). An
//! allow-list built from what the UI sees would silently drop those, so the UI
//! sends exclusions instead and they are turned into the engine's allow-lists
//! here, against the full `TabState`.

use serde::Deserialize;

use er_engine::app::TabState;
use er_engine::export::ExportOpts as EngineExportOpts;

/// The engine's options plus per-category exclusion lists. Any `include*Ids`
/// allow-list a caller sends still applies; an excluded id is dropped from it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOpts {
    #[serde(flatten)]
    pub engine: EngineExportOpts,
    #[serde(default)]
    pub exclude_comment_ids: Vec<String>,
    #[serde(default)]
    pub exclude_question_ids: Vec<String>,
    #[serde(default)]
    pub exclude_note_ids: Vec<String>,
    #[serde(default)]
    pub exclude_finding_ids: Vec<String>,
}

pub fn render_markdown(tab: &TabState, opts: &ExportOpts) -> String {
    er_engine::export::render_markdown(tab, &resolve_exclusions(tab, opts))
}

fn keep_unexcluded<'a>(
    allow: Option<Vec<String>>,
    all: impl Iterator<Item = &'a str>,
    excluded: &[String],
) -> Option<Vec<String>> {
    if excluded.is_empty() {
        return allow;
    }
    let base: Vec<String> = allow.unwrap_or_else(|| all.map(str::to_string).collect());
    Some(
        base.into_iter()
            .filter(|id| !excluded.contains(id))
            .collect(),
    )
}

fn resolve_exclusions(tab: &TabState, opts: &ExportOpts) -> EngineExportOpts {
    let mut out = opts.engine.clone();
    let ai = &tab.ai;
    out.include_question_ids = keep_unexcluded(
        out.include_question_ids.take(),
        ai.questions
            .iter()
            .flat_map(|q| q.questions.iter().map(|q| q.id.as_str())),
        &opts.exclude_question_ids,
    );
    out.include_note_ids = keep_unexcluded(
        out.include_note_ids.take(),
        ai.notes
            .iter()
            .flat_map(|n| n.notes.iter().map(|n| n.id.as_str())),
        &opts.exclude_note_ids,
    );
    out.include_comment_ids = keep_unexcluded(
        out.include_comment_ids.take(),
        ai.github_comments
            .iter()
            .flat_map(|c| c.comments.iter().map(|c| c.id.as_str())),
        &opts.exclude_comment_ids,
    );
    out.include_finding_ids = keep_unexcluded(
        out.include_finding_ids.take(),
        ai.review.iter().flat_map(|r| {
            r.files
                .values()
                .flat_map(|f| f.findings.iter().map(|f| f.id.as_str()))
        }),
        &opts.exclude_finding_ids,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use er_engine::ai::{AiState, ErQuestions, ReviewQuestion};

    fn question(id: &str, text: &str, resolved: bool) -> ReviewQuestion {
        ReviewQuestion {
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
        let opts: ExportOpts =
            serde_json::from_value(serde_json::json!({ "excludeQuestionIds": ["q-drop"] }))
                .expect("opts parse");
        let out = render_markdown(&tab(), &opts);

        assert!(out.contains("Still open?"), "{out}");
        assert!(out.contains("Already answered"), "{out}");
        assert!(!out.contains("Leave me out"), "{out}");
    }

    #[test]
    fn exclusions_respect_only_unresolved_and_engine_defaults() {
        let opts: ExportOpts = serde_json::from_value(serde_json::json!({
            "onlyUnresolved": true,
            "excludeQuestionIds": ["q-drop"],
        }))
        .expect("opts parse");
        assert!(
            opts.engine.include_questions,
            "missing toggles keep engine defaults"
        );
        let out = render_markdown(&tab(), &opts);

        assert!(out.contains("Still open?"), "{out}");
        assert!(!out.contains("Already answered"), "{out}");
        assert!(!out.contains("Leave me out"), "{out}");
    }

    #[test]
    fn an_allow_list_still_applies_beside_exclusions() {
        let opts: ExportOpts = serde_json::from_value(serde_json::json!({
            "includeQuestionIds": ["q-open", "q-drop"],
            "excludeQuestionIds": ["q-drop"],
        }))
        .expect("opts parse");
        let out = render_markdown(&tab(), &opts);

        assert!(out.contains("Still open?"), "{out}");
        assert!(!out.contains("Already answered"), "{out}");
        assert!(!out.contains("Leave me out"), "{out}");
    }

    /// The Export view counts items from the rendered markdown
    /// (`countExportedItems` in `desktop-ui/src/lib/exportOptions.ts`): one
    /// `### … — <Kind> (` heading per exported item. Pin the shape it reads.
    #[test]
    fn every_exported_item_gets_one_counted_heading() {
        let out = render_markdown(&tab(), &ExportOpts::default());
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
