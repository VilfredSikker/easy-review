//! Clearing a bucket's private drafts: the questions and notes with their
//! backups, or only the answers Claude wrote into them. Both front ends call
//! these, so they need no App, and a busy bucket answers its lock error.

use er_store::{Scope, StoreError, UpdateError};

use crate::ai::{annotation_store, ErNotes, ErQuestions, PrivateAnnotation};

/// Delete the questions and notes, and their `.prev.json` backups, under one
/// lock hold.
pub fn cleanup_questions_and_notes(er_dir: &str) -> Result<(), StoreError> {
    let store = crate::store_registry::store();
    let scope = Scope::dir(er_dir)?;
    let guard = store.lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)?;
    store.delete_locked(
        &guard,
        &er_store::ArtifactKey::<ErQuestions>::new(scope.clone(), ())?,
    )?;
    store.delete_locked(&guard, &er_store::ArtifactKey::<ErNotes>::new(scope, ())?)?;
    // The `.prev.json` backups stay plain files.
    let base = std::path::Path::new(er_dir);
    let _ = std::fs::remove_file(base.join("questions.prev.json"));
    let _ = std::fs::remove_file(base.join("notes.prev.json"));
    Ok(())
}

/// Remove Claude's replies from the notes, keeping human notes, and reopen
/// the notes they answered. A missing notes file has nothing to clear.
pub fn cleanup_note_replies(er_dir: &str) -> Result<(), StoreError> {
    let store = crate::store_registry::store();
    let scope = Scope::dir(er_dir)?;
    absent_is_clear(annotation_store::update_notes(&store, &scope, |current| {
        let mut ns = current.ok_or(Absent)?;
        cleanup_claude_replies(&mut ns.notes);
        Ok(ns)
    }))
}

/// Remove Claude's answers from the questions, keeping human questions, and
/// reopen the questions they answered.
pub fn cleanup_question_answers(er_dir: &str) -> Result<(), StoreError> {
    let store = crate::store_registry::store();
    let scope = Scope::dir(er_dir)?;
    absent_is_clear(annotation_store::update_questions(
        &store,
        &scope,
        |current| {
            let mut qs = current.ok_or(Absent)?;
            cleanup_claude_replies(&mut qs.questions);
            Ok(qs)
        },
    ))
}

/// A file with nothing to read: the update writes nothing.
struct Absent;

fn absent_is_clear<T>(updated: Result<T, UpdateError<Absent>>) -> Result<(), StoreError> {
    match updated {
        Ok(_) | Err(UpdateError::Apply(Absent)) => Ok(()),
        Err(UpdateError::Store(error)) => Err(error),
    }
}

fn cleanup_claude_replies(items: &mut Vec<PrivateAnnotation>) {
    let reply_ids: std::collections::HashSet<String> = items
        .iter()
        .filter(|q| q.author == "Claude")
        .map(|q| q.id.clone())
        .collect();
    let parent_ids: std::collections::HashSet<String> = items
        .iter()
        .filter(|q| q.author == "Claude")
        .filter_map(|q| q.in_reply_to.clone())
        .collect();

    items.retain(|q| !reply_ids.contains(&q.id));

    for q in items.iter_mut() {
        if parent_ids.contains(&q.id) {
            q.resolved = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: [&str; 7] = [
        "questions.json",
        "notes.json",
        "questions.prev.json",
        "notes.prev.json",
        "review.json",
        "professor.json",
        "experts/sec.json",
    ];

    /// Every clear keeps to its bucket: a sibling bucket of the same parent,
    /// holding the same files, keeps every one of them.
    #[test]
    fn the_clears_never_touch_a_sibling_bucket() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let (bucket, sibling) = (dir.path().join("branch"), dir.path().join("staged"));
            for at in [&bucket, &sibling] {
                std::fs::create_dir_all(at.join("experts")).unwrap();
                let row = serde_json::json!({
                    "id": "q-1", "file": "a.rs", "hunk_index": 0, "line_start": 1,
                    "text": "q", "author": "Claude"
                });
                crate::test_support::write_sidecar_json::<ErQuestions>(
                    at,
                    serde_json::json!({"version": 1, "diff_hash": "h", "questions": [row]}),
                );
                crate::test_support::write_sidecar_json::<ErNotes>(
                    at,
                    serde_json::json!({"version": 1, "diff_hash": "h", "notes": [row]}),
                );
                for file in &FILES[2..] {
                    std::fs::write(at.join(file), "{}").unwrap();
                }
            }
            let er = bucket.to_str().unwrap();
            cleanup_question_answers(er).unwrap();
            cleanup_note_replies(er).unwrap();
            cleanup_questions_and_notes(er).unwrap();
            crate::ai::review_cleanup::cleanup_review_artifacts(er).unwrap();
            let left =
                |at: &std::path::Path| FILES.iter().filter(|file| at.join(file).exists()).count();
            assert_eq!(left(&bucket), 0);
            assert_eq!(left(&sibling), FILES.len());
            let kept = crate::test_support::read_sidecar::<ErQuestions>(&sibling);
            assert_eq!(kept.questions.len(), 1, "the sibling's answer stays");
        });
    }
}
