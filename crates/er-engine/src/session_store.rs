//! The single writer for `session.json` and `reviewed.json`.
//!
//! Both are `Enveloped` artifacts; every read and write goes through
//! [`er_store::ArtifactStore`]. An unreadable or absent file reads as empty,
//! matching the pre-store behaviour.

use std::collections::HashMap;

use er_store::{
    Artifact, ArtifactKey, ArtifactStore, Loaded, Registry, Scope, StoreError, Unreadable,
    DEFAULT_LOCK_TIMEOUT,
};
use serde::{Deserialize, Serialize};

/// Serializable session state for restoring review progress across restarts.
/// Keyed by diff hash — only restored when the diff hasn't changed.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionState {
    /// SHA-256 hash of the diff this session was saved against
    pub diff_hash: String,

    /// Current branch name (for sanity check on restore)
    #[serde(default)]
    pub branch: String,

    /// Index of the selected file
    #[serde(default)]
    pub selected_file: usize,

    /// Current hunk index
    #[serde(default)]
    pub current_hunk: usize,

    /// Current line index within hunk (None = hunk-level)
    #[serde(default)]
    pub current_line: Option<usize>,

    /// Vertical scroll offset in diff view
    #[serde(default)]
    pub diff_scroll: u16,

    /// Horizontal scroll offset
    #[serde(default)]
    pub h_scroll: u16,

    /// Active diff viewing mode
    #[serde(default)]
    pub diff_mode: String,

    /// Active filter expression
    #[serde(default)]
    pub filter_expr: String,

    /// Filter history (most recent first)
    #[serde(default)]
    pub filter_history: Vec<String>,

    /// Whether showing only unreviewed files
    #[serde(default)]
    pub show_unreviewed_only: bool,

    /// Whether sorting by mtime
    #[serde(default)]
    pub sort_by_mtime: bool,

    /// In-progress comment draft text (empty if none)
    #[serde(default)]
    pub comment_draft: String,

    /// Comment draft target file
    #[serde(default)]
    pub comment_draft_file: String,

    /// Comment draft target hunk
    #[serde(default)]
    pub comment_draft_hunk: usize,

    /// Comment draft target line
    #[serde(default)]
    pub comment_draft_line: Option<usize>,

    /// Comment draft type ("question" or "github")
    #[serde(default)]
    pub comment_draft_type: String,

    /// Comment draft reply target, so a restored reply is still a reply.
    #[serde(default)]
    pub comment_draft_reply_to: Option<String>,

    /// Finding the draft replies to. Without it a restored finding reply would
    /// post as a public line comment.
    #[serde(default)]
    pub comment_draft_finding_ref: Option<String>,

    /// Comment being edited, so a restored edit stays an edit.
    #[serde(default)]
    pub comment_draft_edit_id: Option<String>,

    /// Review side of the draft ("LEFT" / "RIGHT").
    #[serde(default)]
    pub comment_draft_side: Option<String>,

    /// Inclusive end line of a multi-line draft.
    #[serde(default)]
    pub comment_draft_line_end: Option<usize>,

    /// Client id of the draft's GitHub post, reused by a retry.
    #[serde(default)]
    pub comment_draft_client_id: Option<String>,
}

impl Artifact for SessionState {
    const KIND: &'static str = "session";
    const VERSION: u32 = 1;
    const FILE: &'static str = "session.json";
    type Slot = ();
}

/// The reviewed marks as one envelope: path to the per-file hash it was marked at.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReviewedFiles {
    #[serde(default)]
    pub files: HashMap<String, String>,
}

impl Artifact for ReviewedFiles {
    const KIND: &'static str = "reviewed";
    const VERSION: u32 = 1;
    const FILE: &'static str = "reviewed.json";
    type Slot = ();
}

/// Register the session and reviewed kinds.
pub fn register(registry: &mut Registry) -> Result<(), StoreError> {
    registry.register::<SessionState>()?;
    registry.register::<ReviewedFiles>()?;
    Ok(())
}

pub(crate) fn key<T: Artifact<Slot = ()>>(scope: &Scope) -> Result<ArtifactKey<T>, StoreError> {
    ArtifactKey::<T>::new(scope.clone(), ())
}

/// The stored session, or `None` when it is absent or unreadable.
pub fn load_session(store: &ArtifactStore, scope: &Scope) -> Option<SessionState> {
    match store.read(&key::<SessionState>(scope).ok()?).ok()? {
        Loaded::Found(stored) => Some(stored.value),
        _ => None,
    }
}

pub fn save_session(
    store: &ArtifactStore,
    scope: &Scope,
    session: &SessionState,
) -> Result<(), StoreError> {
    store.write(&key(scope)?, session)
}

/// The reviewed marks. An absent or unreadable file is an empty map.
pub fn load_reviewed(
    store: &ArtifactStore,
    scope: &Scope,
) -> Result<HashMap<String, String>, StoreError> {
    match store.read(&key::<ReviewedFiles>(scope)?)? {
        Loaded::Found(stored) => Ok(stored.value.files),
        _ => Ok(HashMap::new()),
    }
}

/// Save the reviewed marks; an empty map deletes the file.
///
/// Deleting takes the lock and runs the same newer-version check as a write,
/// so an unmark cannot erase a file a newer build wrote.
pub fn save_reviewed(
    store: &ArtifactStore,
    scope: &Scope,
    files: &HashMap<String, String>,
) -> Result<(), StoreError> {
    let key = key::<ReviewedFiles>(scope)?;
    if files.is_empty() {
        let guard = store.lock_scope(scope, DEFAULT_LOCK_TIMEOUT)?;
        if let Loaded::Unreadable(Unreadable::NewerVersion { found }) = store.read(&key)? {
            return Err(StoreError::NewerVersion {
                path: store.path(&key),
                found,
                expected: ReviewedFiles::VERSION,
            });
        }
        return store.delete_locked(&guard, &key);
    }
    store.write(
        &key,
        &ReviewedFiles {
            files: files.clone(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store_and_scope(tmp: &TempDir) -> (ArtifactStore, Scope) {
        (
            ArtifactStore::new(tmp.path()),
            Scope::dir(tmp.path()).unwrap(),
        )
    }

    fn reviewed_path(store: &ArtifactStore, scope: &Scope) -> std::path::PathBuf {
        store.path(&key::<ReviewedFiles>(scope).unwrap())
    }

    #[test]
    fn reviewed_round_trips_through_an_envelope() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let files = HashMap::from([
            ("a.rs".to_string(), "hash-a".to_string()),
            ("b.rs".to_string(), "hash-b".to_string()),
        ]);
        save_reviewed(&store, &scope, &files).unwrap();

        let raw = std::fs::read_to_string(reviewed_path(&store, &scope)).unwrap();
        let document: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(document["kind"], "reviewed");
        assert_eq!(document["payload"]["files"]["a.rs"], "hash-a");
        assert_eq!(document["payload"]["files"]["b.rs"], "hash-b");

        assert_eq!(load_reviewed(&store, &scope).unwrap(), files);
    }

    #[test]
    fn saving_an_empty_reviewed_map_removes_the_file() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let files = HashMap::from([("a.rs".to_string(), "hash-a".to_string())]);
        save_reviewed(&store, &scope, &files).unwrap();
        assert!(reviewed_path(&store, &scope).is_file());

        save_reviewed(&store, &scope, &HashMap::new()).unwrap();
        assert!(!reviewed_path(&store, &scope).exists());
        assert!(load_reviewed(&store, &scope).unwrap().is_empty());
    }

    #[test]
    fn an_empty_hash_entry_survives_a_round_trip() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let files = HashMap::from([("a.rs".to_string(), String::new())]);
        save_reviewed(&store, &scope, &files).unwrap();
        assert_eq!(
            load_reviewed(&store, &scope).unwrap(),
            HashMap::from([("a.rs".to_string(), String::new())])
        );
    }

    #[test]
    fn unmarking_refuses_a_newer_reviewed_file() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = reviewed_path(&store, &scope);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let newer = r#"{"kind":"reviewed","version":99,"payload":{"files":{"a.rs":"hash-a"}}}"#;
        std::fs::write(&path, newer).unwrap();

        let error = save_reviewed(&store, &scope, &HashMap::new()).unwrap_err();
        assert!(matches!(error, StoreError::NewerVersion { found: 99, .. }));
        assert_eq!(std::fs::read_to_string(path).unwrap(), newer);
    }

    #[test]
    fn a_session_with_an_unknown_version_is_ignored() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = store.path(&key::<SessionState>(&scope).unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            r#"{"kind":"session","version":99,"payload":{"diff_hash":"hash"}}"#,
        )
        .unwrap();
        assert!(load_session(&store, &scope).is_none());
    }
}

#[cfg(all(test, feature = "ui"))]
#[expect(
    clippy::unwrap_used,
    reason = "this test module needs the ui feature, which clippy's test exemption does not recognise"
)]
mod ui_tests {
    use super::*;
    use crate::app::TabState;
    use crate::paths::ErRoot;
    use tempfile::TempDir;

    fn store_and_scope(tmp: &TempDir) -> (ArtifactStore, Scope) {
        (
            ArtifactStore::new(tmp.path()),
            Scope::dir(tmp.path()).unwrap(),
        )
    }

    fn captured_session() -> SessionState {
        let mut tab = TabState::new_for_test(vec![]);
        tab.branch_diff_hash = "hash".to_string();
        tab.capture_session()
    }

    #[test]
    fn a_newer_session_is_not_overwritten() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = store.path(&key::<SessionState>(&scope).unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let newer = r#"{"kind":"session","version":99,"payload":{"diff_hash":"hash"}}"#;
        std::fs::write(&path, newer).unwrap();

        let error = save_session(&store, &scope, &captured_session()).unwrap_err();
        assert!(matches!(error, StoreError::NewerVersion { found: 99, .. }));
        assert!(load_session(&store, &scope).is_none());
        assert_eq!(std::fs::read_to_string(path).unwrap(), newer);
    }

    #[test]
    fn saving_a_session_takes_the_scope_lock() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let guard = store.lock_scope(&scope, DEFAULT_LOCK_TIMEOUT).unwrap();
        let error = save_session(&store, &scope, &captured_session()).unwrap_err();
        assert!(matches!(error, StoreError::AlreadyLocked { .. }));
        drop(guard);
    }

    #[test]
    fn session_round_trips_and_restore_rejects_a_changed_diff() {
        let tmp = TempDir::new().unwrap();
        crate::storage::with_test_storage_root(tmp.path(), || {
            let mut tab = TabState::new_for_test(vec![]);
            tab.er_root = ErRoot::managed(tmp.path().to_string_lossy());
            tab.branch_diff_hash = "hash-a".to_string();
            tab.selected_file = 3;
            tab.save_session();

            let (store, scope) = store_and_scope(&tmp);
            let session = load_session(&store, &scope).expect("the saved session");
            assert_eq!(session.diff_hash, "hash-a");
            assert_eq!(session.selected_file, 3);

            tab.branch_diff_hash = "hash-b".to_string();
            assert!(!tab.restore_session());
        });
    }
}
