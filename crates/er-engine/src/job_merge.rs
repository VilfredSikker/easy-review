//! By-id three-way merge for the user-edited artifact kinds.
//!
//! An update-kind job is handed a copy of its file at launch (`base`) and
//! rewrites the whole file (`theirs`). The user can edit the same file while
//! the agent runs (`current`, read from the store under the commit's lock). For
//! each id: if the agent changed it from base, take theirs, unless the user
//! also changed it — the user wins and the dropped agent change is reported.
//! Ids only the user holds survive untouched.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

/// A whole-file artifact the host merges by per-entry id.
pub trait MergeById: er_store::Artifact + Clone {
    type Entry: Serialize + Clone;

    fn schema_version(&self) -> u32;
    fn entries(&self) -> &[Self::Entry];
    fn set_entries(&mut self, entries: Vec<Self::Entry>);
    fn id(entry: &Self::Entry) -> &str;
}

impl MergeById for crate::ai::ErQuestions {
    type Entry = crate::ai::PrivateAnnotation;

    fn schema_version(&self) -> u32 {
        self.version
    }

    fn entries(&self) -> &[Self::Entry] {
        &self.questions
    }

    fn set_entries(&mut self, entries: Vec<Self::Entry>) {
        self.questions = entries;
    }

    fn id(entry: &Self::Entry) -> &str {
        &entry.id
    }
}

impl MergeById for crate::ai::ErNotes {
    type Entry = crate::ai::PrivateAnnotation;

    fn schema_version(&self) -> u32 {
        self.version
    }

    fn entries(&self) -> &[Self::Entry] {
        &self.notes
    }

    fn set_entries(&mut self, entries: Vec<Self::Entry>) {
        self.notes = entries;
    }

    fn id(entry: &Self::Entry) -> &str {
        &entry.id
    }
}

impl MergeById for crate::ai::ErGithubComments {
    type Entry = crate::ai::GithubComment;

    fn schema_version(&self) -> u32 {
        self.version
    }

    fn entries(&self) -> &[Self::Entry] {
        &self.comments
    }

    fn set_entries(&mut self, entries: Vec<Self::Entry>) {
        self.comments = entries;
    }

    fn id(entry: &Self::Entry) -> &str {
        &entry.id
    }
}

fn index<T: MergeById>(value: Option<&T>) -> BTreeMap<String, &T::Entry> {
    value
        .map(|value| {
            value
                .entries()
                .iter()
                .map(|entry| (T::id(entry).to_string(), entry))
                .collect()
        })
        .unwrap_or_default()
}

/// Merge `theirs` over `current`, using `base` to attribute each change.
///
/// Returns the merged artifact (`theirs`' version and diff hash, since those
/// were validated against the staged diff) and the ids whose agent change lost
/// to a user edit.
pub fn merge_by_id<T: MergeById>(
    base: Option<&T>,
    theirs: &T,
    current: Option<&T>,
) -> (T, Vec<String>) {
    let base = index(base);
    let theirs_index = index(Some(theirs));
    let current_index = index(current);

    // Start from the user's live copy, in its own order: every id they hold
    // keeps its place unless the agent changed it and the user did not.
    let mut merged: Vec<T::Entry> = current
        .map(|value| value.entries().to_vec())
        .unwrap_or_default();
    let mut dropped = Vec::new();

    let ids: BTreeSet<&str> = base
        .keys()
        .chain(theirs_index.keys())
        .map(String::as_str)
        .collect();
    for id in ids {
        let base_entry = base.get(id);
        let theirs_entry = theirs_index.get(id);
        if !changed(base_entry, theirs_entry) {
            continue;
        }
        let current_entry = current_index.get(id);
        if changed(base_entry, current_entry) {
            // Both sides touched this id. The user's copy stays; a deletion
            // on both sides is agreement, not a dropped change.
            if !(current_entry.is_none() && theirs_entry.is_none()) {
                dropped.push(id.to_string());
            }
            continue;
        }
        match theirs_entry {
            Some(entry) => {
                let entry = (*entry).clone();
                match merged.iter_mut().find(|existing| T::id(existing) == id) {
                    Some(slot) => *slot = entry,
                    None => merged.push(entry),
                }
            }
            None => merged.retain(|existing| T::id(existing) != id),
        }
    }

    let mut value = theirs.clone();
    value.set_entries(merged);
    (value, dropped)
}

fn changed<T: Serialize>(base: Option<&&T>, other: Option<&&T>) -> bool {
    match (base, other) {
        (None, None) => false,
        (Some(base), Some(other)) => !same(base, other),
        _ => true,
    }
}

/// Compare as the store would write them: the runtime-only fields are skipped
/// by serde, so they cannot make two otherwise equal entries differ.
fn same<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn questions(entries: &[(&str, &str)]) -> crate::ai::ErQuestions {
        let questions: Vec<serde_json::Value> = entries
            .iter()
            .map(|(id, text)| {
                json!({
                    "id": id,
                    "file": "src/a.rs",
                    "hunk_index": 0,
                    "line_start": 1,
                    "line_content": "x",
                    "text": text,
                })
            })
            .collect();
        serde_json::from_value(json!({
            "version": 1,
            "diff_hash": "hash",
            "questions": questions,
        }))
        .unwrap()
    }

    fn texts(value: &crate::ai::ErQuestions) -> Vec<(String, String)> {
        value
            .questions
            .iter()
            .map(|question| (question.id.clone(), question.text.clone()))
            .collect()
    }

    #[test]
    fn an_agent_change_lands_when_the_user_did_not_touch_the_id() {
        let base = questions(&[("q1", "base")]);
        let theirs = questions(&[("q1", "agent")]);
        let current = questions(&[("q1", "base")]);

        let (merged, dropped) = merge_by_id(Some(&base), &theirs, Some(&current));

        assert_eq!(texts(&merged), [("q1".into(), "agent".into())]);
        assert!(dropped.is_empty());
    }

    #[test]
    fn the_users_new_id_survives() {
        let base = questions(&[("q1", "base")]);
        let theirs = questions(&[("q1", "base")]);
        let current = questions(&[("q1", "base"), ("q2", "user")]);

        let (merged, dropped) = merge_by_id(Some(&base), &theirs, Some(&current));

        assert_eq!(
            texts(&merged),
            [("q1".into(), "base".into()), ("q2".into(), "user".into())]
        );
        assert!(dropped.is_empty());
    }

    #[test]
    fn the_user_wins_a_same_id_conflict() {
        let base = questions(&[("q1", "base")]);
        let theirs = questions(&[("q1", "agent")]);
        let current = questions(&[("q1", "user")]);

        let (merged, dropped) = merge_by_id(Some(&base), &theirs, Some(&current));

        assert_eq!(texts(&merged), [("q1".into(), "user".into())]);
        assert_eq!(dropped, ["q1"]);
    }

    #[test]
    fn an_agent_deletion_lands_when_the_user_did_not_touch_the_id() {
        let base = questions(&[("q1", "base"), ("q2", "base")]);
        let theirs = questions(&[("q2", "base")]);
        let current = questions(&[("q1", "base"), ("q2", "base")]);

        let (merged, dropped) = merge_by_id(Some(&base), &theirs, Some(&current));

        assert_eq!(texts(&merged), [("q2".into(), "base".into())]);
        assert!(dropped.is_empty());
    }

    #[test]
    fn the_users_edit_beats_the_agents_deletion() {
        let base = questions(&[("q1", "base")]);
        let theirs = questions(&[]);
        let current = questions(&[("q1", "user")]);

        let (merged, dropped) = merge_by_id(Some(&base), &theirs, Some(&current));

        assert_eq!(texts(&merged), [("q1".into(), "user".into())]);
        assert_eq!(dropped, ["q1"]);
    }

    #[test]
    fn an_agent_addition_lands_and_the_users_addition_of_the_same_id_wins() {
        let base = questions(&[]);
        let theirs = questions(&[("q1", "agent"), ("q2", "agent")]);
        let current = questions(&[("q2", "user")]);

        let (merged, dropped) = merge_by_id(Some(&base), &theirs, Some(&current));

        assert_eq!(
            texts(&merged),
            [("q2".into(), "user".into()), ("q1".into(), "agent".into())]
        );
        assert_eq!(dropped, ["q2"]);
    }

    #[test]
    fn github_comments_merge_by_comment_id() {
        let comment = |id: &str, text: &str| {
            json!({
                "id": id,
                "file": "src/a.rs",
                "hunk_index": 0,
                "line_start": 1,
                "line_end": 1,
                "comment": text,
            })
        };
        let value = |entries: &[(&str, &str)]| -> crate::ai::ErGithubComments {
            serde_json::from_value(json!({
                "version": 1,
                "diff_hash": "hash",
                "comments": entries.iter().map(|(id, text)| comment(id, text)).collect::<Vec<_>>(),
            }))
            .unwrap()
        };
        let base = value(&[("c1", "base")]);
        let theirs = value(&[("c1", "agent"), ("c2", "agent")]);
        let current = value(&[("c1", "user")]);

        let (merged, dropped) = merge_by_id(Some(&base), &theirs, Some(&current));

        let entries: Vec<(String, String)> = merged
            .comments
            .iter()
            .map(|comment| (comment.id.clone(), comment.comment.clone()))
            .collect();
        assert_eq!(
            entries,
            [("c1".into(), "user".into()), ("c2".into(), "agent".into())]
        );
        assert_eq!(dropped, ["c1"]);
    }
}
