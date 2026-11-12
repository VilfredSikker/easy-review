//! The single writer for `questions.json`, `notes.json` and
//! `github-comments.json`.
//!
//! Every read and write of the three files goes through this module on top of
//! [`er_store::ArtifactStore`]. It has no `App` access, so it never notifies:
//! a caller that needs the existing "invalid JSON — starting fresh" warning
//! reads [`Updated::was_unreadable`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use er_store::{
    Artifact, ArtifactKey, ArtifactStore, Loaded, Registry, RelPath, Scope, StoreError,
    UpdateError, DEFAULT_LOCK_TIMEOUT,
};

use crate::ai::{ErGithubComments, ErNotes, ErQuestions};

/// The payload a successful update wrote, plus whether the previous file was
/// present but unreadable (not `NewerVersion`).
#[derive(Debug, Clone, PartialEq)]
pub struct Updated<T> {
    pub value: T,
    pub was_unreadable: bool,
}

/// Register the three annotation kinds.
pub fn register(registry: &mut Registry) -> Result<(), StoreError> {
    registry.register::<ErQuestions>()?;
    registry.register::<ErNotes>()?;
    registry.register::<ErGithubComments>()?;
    Ok(())
}

fn key<T: Artifact<Slot = ()>>(scope: &Scope) -> Result<ArtifactKey<T>, StoreError> {
    ArtifactKey::<T>::new(scope.clone(), ())
}

pub fn load_questions(
    store: &ArtifactStore,
    scope: &Scope,
) -> Result<Loaded<ErQuestions>, StoreError> {
    store.read(&key(scope)?)
}

/// The raw text `questions.json` holds, for a caller that copies the file
/// verbatim. The store still owns the read, the size cap and the path.
pub fn questions_text(store: &ArtifactStore, scope: &Scope) -> Result<Loaded<String>, StoreError> {
    store.read_text(scope, &RelPath::new(ErQuestions::FILE)?)
}

pub fn load_notes(store: &ArtifactStore, scope: &Scope) -> Result<Loaded<ErNotes>, StoreError> {
    store.read(&key(scope)?)
}

pub fn load_github_comments(
    store: &ArtifactStore,
    scope: &Scope,
) -> Result<Loaded<ErGithubComments>, StoreError> {
    store.read(&key(scope)?)
}

/// Read, apply and write `questions.json` under one scope-lock hold.
pub fn update_questions<E, F>(
    store: &ArtifactStore,
    scope: &Scope,
    f: F,
) -> Result<Updated<ErQuestions>, UpdateError<E>>
where
    F: FnOnce(Option<ErQuestions>) -> Result<ErQuestions, E>,
{
    let (value, unreadable) = store.update_durable(&key(scope)?, DEFAULT_LOCK_TIMEOUT, f)?;
    Ok(Updated {
        value,
        was_unreadable: unreadable.is_some(),
    })
}

/// Read, apply and write `notes.json` under one scope-lock hold.
pub fn update_notes<E, F>(
    store: &ArtifactStore,
    scope: &Scope,
    f: F,
) -> Result<Updated<ErNotes>, UpdateError<E>>
where
    F: FnOnce(Option<ErNotes>) -> Result<ErNotes, E>,
{
    let (value, unreadable) = store.update_durable(&key(scope)?, DEFAULT_LOCK_TIMEOUT, f)?;
    Ok(Updated {
        value,
        was_unreadable: unreadable.is_some(),
    })
}

/// Read, apply and write `github-comments.json` under one scope-lock hold.
///
/// The non-durable write is deliberate: a pull can rebuild these entries from
/// GitHub, while questions and notes are user data.
pub fn update_github_comments<E, F>(
    store: &ArtifactStore,
    scope: &Scope,
    f: F,
) -> Result<Updated<ErGithubComments>, UpdateError<E>>
where
    F: FnOnce(Option<ErGithubComments>) -> Result<ErGithubComments, E>,
{
    let (value, unreadable) = store.update(&key(scope)?, DEFAULT_LOCK_TIMEOUT, f)?;
    Ok(Updated {
        value,
        was_unreadable: unreadable.is_some(),
    })
}

/// Delete the whole file for a singleton kind.
pub fn delete<T: Artifact<Slot = ()>>(
    store: &ArtifactStore,
    scope: &Scope,
) -> Result<(), StoreError> {
    store.delete(&key::<T>(scope)?)
}

/// The path a singleton kind resolves to, for mtime bookkeeping.
pub fn path_of<T: Artifact<Slot = ()>>(
    store: &ArtifactStore,
    scope: &Scope,
) -> Result<PathBuf, StoreError> {
    Ok(store.path(&key::<T>(scope)?))
}

static ANNOTATION_SEQ: AtomicU64 = AtomicU64::new(0);

/// A new question or note id. The prefix includes the dash (`"q-"` or `"n-"`).
///
/// The sequence suffix keeps two ids created in the same millisecond apart; the
/// wire format stays `q-<millis>-<seq>` / `n-<millis>-<seq>`.
pub fn new_annotation_id(prefix: &str) -> String {
    let seq = ANNOTATION_SEQ.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{prefix}{millis}-{seq}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{GithubComment, PrivateAnnotation};
    use er_store::{ArtifactStore, Loaded, StoreError};
    use tempfile::TempDir;

    fn annotation(id: &str) -> PrivateAnnotation {
        PrivateAnnotation {
            id: id.to_string(),
            timestamp: String::new(),
            file: "a.rs".to_string(),
            hunk_index: Some(0),
            line_start: Some(4),
            line_end: None,
            line_content: String::new(),
            text: "text".to_string(),
            resolved: false,
            stale: false,
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            side: "RIGHT".to_string(),
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            in_reply_to: None,
            author: "You".to_string(),
            promoted_to: None,
            finding_ref: None,
            ai_action: None,
        }
    }

    fn github_comment(id: &str) -> GithubComment {
        GithubComment {
            id: id.to_string(),
            timestamp: String::new(),
            file: "a.rs".to_string(),
            hunk_index: Some(0),
            line_start: Some(4),
            line_end: None,
            line_content: String::new(),
            comment: "text".to_string(),
            in_reply_to: None,
            resolved: false,
            source: "github".to_string(),
            github_id: Some(1),
            author: "You".to_string(),
            outdated: false,
            stale: false,
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".to_string(),
        }
    }

    fn questions() -> ErQuestions {
        ErQuestions {
            version: 1,
            diff_hash: "hash".to_string(),
            questions: vec![annotation("q-1")],
        }
    }

    fn notes() -> ErNotes {
        ErNotes {
            version: 1,
            diff_hash: "hash".to_string(),
            notes: vec![annotation("n-1")],
        }
    }

    fn github_comments() -> ErGithubComments {
        ErGithubComments {
            version: 1,
            diff_hash: "hash".to_string(),
            github: None,
            comments: vec![github_comment("gh-1")],
        }
    }

    fn store_and_scope(tmp: &TempDir) -> (ArtifactStore, Scope) {
        (
            ArtifactStore::new(tmp.path()),
            Scope::dir(tmp.path()).unwrap(),
        )
    }

    #[test]
    fn questions_are_stored_as_todays_bytes() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let payload = questions();
        update_questions(&store, &scope, |_| {
            Ok::<_, std::convert::Infallible>(payload.clone())
        })
        .unwrap();
        let path = path_of::<ErQuestions>(&store, &scope).unwrap();
        let raw = std::fs::read_to_string(path).unwrap();
        assert_eq!(raw, serde_json::to_string_pretty(&payload).unwrap());
    }

    #[test]
    fn notes_are_stored_as_todays_bytes() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let payload = notes();
        update_notes(&store, &scope, |_| {
            Ok::<_, std::convert::Infallible>(payload.clone())
        })
        .unwrap();
        let path = path_of::<ErNotes>(&store, &scope).unwrap();
        let raw = std::fs::read_to_string(path).unwrap();
        assert_eq!(raw, serde_json::to_string_pretty(&payload).unwrap());
    }

    #[test]
    fn github_comments_are_stored_as_todays_bytes() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let payload = github_comments();
        update_github_comments(&store, &scope, |_| {
            Ok::<_, std::convert::Infallible>(payload.clone())
        })
        .unwrap();
        let path = path_of::<ErGithubComments>(&store, &scope).unwrap();
        let raw = std::fs::read_to_string(path).unwrap();
        assert_eq!(raw, serde_json::to_string_pretty(&payload).unwrap());
    }

    #[test]
    fn a_file_an_agent_rewrote_still_loads() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = path_of::<ErQuestions>(&store, &scope).unwrap();
        std::fs::write(
            path,
            r#"{"diff_hash":"hash","version":1,"questions":[{"id":"q-1","file":"a.rs","text":"text","extra":true}]}"#,
        )
        .unwrap();
        match load_questions(&store, &scope).unwrap() {
            Loaded::Found(stored) => {
                assert_eq!(stored.value.questions.len(), 1);
                assert_eq!(stored.value.questions[0].id, "q-1");
            }
            other => panic!("expected a payload, got {other:?}"),
        }
    }

    #[test]
    fn a_newer_questions_file_is_not_overwritten() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = path_of::<ErQuestions>(&store, &scope).unwrap();
        std::fs::write(&path, r#"{"version":99,"diff_hash":"hash","questions":[]}"#).unwrap();
        let error = update_questions(
            &store,
            &scope,
            |_| -> Result<ErQuestions, std::convert::Infallible> {
                panic!("the closure must not run for a newer file")
            },
        )
        .unwrap_err();
        assert!(matches!(
            &error,
            UpdateError::Store(StoreError::NewerVersion { found: 99, .. })
        ));
        let message = error.to_string();
        assert!(message.contains("questions.json"), "{message}");
        assert!(message.contains("99"), "{message}");
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            r#"{"version":99,"diff_hash":"hash","questions":[]}"#
        );
    }

    #[test]
    fn update_reports_an_unreadable_file() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = path_of::<ErQuestions>(&store, &scope).unwrap();
        std::fs::write(path, "{ not json").unwrap();
        let payload = questions();
        let updated = update_questions(&store, &scope, |current| {
            assert!(current.is_none());
            Ok::<_, std::convert::Infallible>(payload)
        })
        .unwrap();
        assert!(updated.was_unreadable);
    }

    #[test]
    fn github_comments_live_in_the_pr_bucket() {
        let tmp = TempDir::new().unwrap();
        let pr_bucket = tmp.path().join("repos/r/prs/pr-7");
        let scope = Scope::dir(&pr_bucket).unwrap();
        let store = ArtifactStore::new(tmp.path());
        update_github_comments(&store, &scope, |_| {
            Ok::<_, std::convert::Infallible>(github_comments())
        })
        .unwrap();
        let path = path_of::<ErGithubComments>(&store, &scope).unwrap();
        assert!(path.ends_with("repos/r/prs/pr-7/github-comments.json"));
        assert!(matches!(
            load_github_comments(&store, &scope).unwrap(),
            Loaded::Found(_)
        ));
        assert!(matches!(
            load_notes(&store, &scope).unwrap(),
            Loaded::Absent
        ));
        assert!(matches!(
            load_questions(&store, &scope).unwrap(),
            Loaded::Absent
        ));
    }
}
