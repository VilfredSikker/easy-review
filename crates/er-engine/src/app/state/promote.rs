//! App-side glue for promoting a question to a note.
//!
//! The conversion itself lives in [`crate::ai::promote`]; this wrapper supplies
//! the current-diff anchor, the file and the diff hash, then writes through
//! `annotation_store` and adopts the result in memory.

use anyhow::{anyhow, Context, Result};

use crate::ai::annotation_store;
use crate::ai::promote::{self, PromotionAnchor};
use crate::ai::ErNotes;
use crate::store_registry;

use super::App;

impl App {
    /// Promote a question to a note.
    ///
    /// Inserts the note, deletes the source question and its replies, adopts
    /// the written payload in memory, marks the sidecar and notifies.
    pub fn promote_question_to_note(
        &mut self,
        source_id: &str,
        body: Option<String>,
    ) -> Result<()> {
        // The notes file is keyed by the branch diff hash; compute it when the
        // tab has none yet (non-Branch views start empty).
        if self.tab().branch_diff_hash.is_empty() && !self.tab().is_remote() {
            let (base_branch, repo_root, pr_head_ref) = {
                let tab = self.tab();
                (
                    tab.base_branch.clone(),
                    tab.repo_root.clone(),
                    tab.pr_head_ref.clone(),
                )
            };
            if let Ok(raw) = self.tab().ports.git.diff_raw(
                "branch",
                &base_branch,
                &repo_root,
                pr_head_ref.as_deref(),
            ) {
                let hash = crate::ai::compute_diff_hash(&raw);
                self.tab_mut().branch_diff_hash = hash;
            }
        }

        let (source, thread, file, anchor, diff_hash, branch_diff_hash) = {
            let tab = self.tab();
            let questions = tab.ai.questions.as_ref().context("No questions loaded")?;
            let source = questions
                .questions
                .iter()
                .find(|question| question.id == source_id)
                .with_context(|| format!("Question not found: {source_id}"))?
                .clone();
            let thread = questions.questions.clone();
            let file = if source.file.is_empty() {
                tab.selected_diff_file()
                    .map(|diff_file| diff_file.path.clone())
                    .unwrap_or_default()
            } else {
                source.file.clone()
            };
            let anchor = if source.is_document_anchor() {
                PromotionAnchor {
                    line_content: source.line_content.clone(),
                    context_before: source.context_before.clone(),
                    context_after: source.context_after.clone(),
                    old_line_start: source.old_line_start,
                    hunk_header: source.hunk_header.clone(),
                }
            } else {
                let line_anchor = self.line_anchor_at(
                    &file,
                    source.hunk_index.unwrap_or(0),
                    source.line_start,
                    Some(source.side.as_str()),
                );
                PromotionAnchor {
                    line_content: line_anchor.line_content,
                    context_before: line_anchor.context_before,
                    context_after: line_anchor.context_after,
                    old_line_start: line_anchor.old_line_start,
                    hunk_header: line_anchor.hunk_header,
                }
            };
            (
                source,
                thread,
                file,
                anchor,
                tab.diff_hash.clone(),
                tab.branch_diff_hash.clone(),
            )
        };

        let note = promote::promote_to_note(&source, &thread, body, file, anchor, diff_hash);
        let text = note.text.clone();

        let store = store_registry::store();
        let scope = er_store::Scope::dir(self.tab().er_dir())?;
        let branch_diff_hash_for_write = branch_diff_hash.clone();
        let note_for_write = note.clone();
        let updated = annotation_store::update_notes(&store, &scope, move |current| {
            let mut notes = current.unwrap_or_else(|| ErNotes {
                version: 1,
                diff_hash: branch_diff_hash_for_write.clone(),
                notes: Vec::new(),
            });
            notes.diff_hash = branch_diff_hash_for_write;
            notes.notes.push(note_for_write);
            Ok::<_, anyhow::Error>(notes)
        })
        .map_err(|error| anyhow!("{error}"))?;

        let source_id_for_delete = source_id.to_string();
        let questions_updated =
            annotation_store::update_questions(&store, &scope, move |current| {
                let mut questions = current.context("questions.json is missing or unreadable")?;
                questions.questions.retain(|question| {
                    question.id != source_id_for_delete
                        && question.in_reply_to.as_deref() != Some(source_id_for_delete.as_str())
                });
                Ok::<_, anyhow::Error>(questions)
            })
            .map_err(|error| anyhow!("{error}"))?;

        let path = annotation_store::path_of::<ErNotes>(&store, &scope)?;
        if updated.was_unreadable {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("notes file");
            self.notify(&format!(
                "Warning: .er/{name} is invalid JSON — starting fresh"
            ));
        }
        let tab = self.tab_mut();
        tab.ai.notes = Some(updated.value);
        tab.ai.questions = Some(questions_updated.value);
        tab.ai.rebuild_comment_index();
        tab.mark_sidecar_written(&path.to_string_lossy());
        self.notify(&format!("Note added: {}", super::truncate(&text, 40)));
        Ok(())
    }
}
