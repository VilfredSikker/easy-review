//! The single reader for `order.json` and the reader/writer for
//! `checklist.json`, on top of [`er_store::ArtifactStore`].
//!
//! Both stay `Bare` in 1.0: the agent writes them in place, so the bytes on
//! disk do not change. The host only rewrites the `checked` flags.

use std::path::PathBuf;

use er_store::{
    Artifact, ArtifactKey, ArtifactStore, Loaded, Registry, Scope, StoreError, UpdateError,
    DEFAULT_LOCK_TIMEOUT,
};

use crate::ai::{ErChecklist, ErOrder};

impl Artifact for ErOrder {
    const KIND: &'static str = "order";
    const VERSION: u32 = 1;
    const FILE: &'static str = "order.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl Artifact for ErChecklist {
    const KIND: &'static str = "checklist";
    const VERSION: u32 = 1;
    const FILE: &'static str = "checklist.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

/// Register the order and checklist kinds.
pub fn register(registry: &mut Registry) -> Result<(), StoreError> {
    registry.register::<ErOrder>()?;
    registry.register::<ErChecklist>()?;
    Ok(())
}

fn key<T: Artifact<Slot = ()>>(scope: &Scope) -> Result<ArtifactKey<T>, StoreError> {
    ArtifactKey::<T>::new(scope.clone(), ())
}

pub fn load_order(store: &ArtifactStore, scope: &Scope) -> Result<Loaded<ErOrder>, StoreError> {
    store.read(&key(scope)?)
}

pub fn load_checklist(
    store: &ArtifactStore,
    scope: &Scope,
) -> Result<Loaded<ErChecklist>, StoreError> {
    store.read(&key(scope)?)
}

/// The checklist item an update wanted is not in the stored file.
#[derive(Debug)]
pub struct ChecklistItemMissing;

impl std::fmt::Display for ChecklistItemMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the checklist item is not in the stored file")
    }
}

impl std::error::Error for ChecklistItemMissing {}

/// The path the checklist resolves to, for `mark_sidecar_written`.
pub fn path_of(store: &ArtifactStore, scope: &Scope) -> Result<PathBuf, StoreError> {
    Ok(store.path(&key::<ErChecklist>(scope)?))
}

/// Set one item's `checked` flag under the scope lock, rebuilding from the
/// file so an agent's rewrite between load and toggle is kept.
pub fn set_checked(
    store: &ArtifactStore,
    scope: &Scope,
    item_id: &str,
    checked: bool,
) -> Result<(ErChecklist, Option<bool>), UpdateError<ChecklistItemMissing>> {
    let key = key::<ErChecklist>(scope)?;
    let (checklist, _) = store.update(&key, DEFAULT_LOCK_TIMEOUT, |current| {
        let Some(mut checklist) = current else {
            return Err(ChecklistItemMissing);
        };
        let Some(item) = checklist.items.iter_mut().find(|item| item.id == item_id) else {
            return Err(ChecklistItemMissing);
        };
        item.checked = checked;
        Ok(checklist)
    })?;
    let state = checklist
        .items
        .iter()
        .find(|item| item.id == item_id)
        .map(|item| item.checked);
    Ok((checklist, state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::ChecklistItem;
    use tempfile::TempDir;

    fn store_and_scope(tmp: &TempDir) -> (ArtifactStore, Scope) {
        (
            ArtifactStore::new(tmp.path()),
            Scope::dir(tmp.path()).unwrap(),
        )
    }

    fn checklist() -> ErChecklist {
        ErChecklist {
            version: 1,
            diff_hash: "hash".to_string(),
            items: vec![ChecklistItem {
                id: "c1".to_string(),
                text: "check it".to_string(),
                category: "tests".to_string(),
                checked: false,
                related_findings: Vec::new(),
                related_files: Vec::new(),
            }],
        }
    }

    fn loaded(store: &ArtifactStore, scope: &Scope) -> ErChecklist {
        match load_checklist(store, scope).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected the checklist, got {other:?}"),
        }
    }

    #[test]
    fn toggle_writes_the_same_bytes_as_before() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = store.path(&key::<ErChecklist>(&scope).unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&checklist()).unwrap()).unwrap();

        set_checked(&store, &scope, "c1", true).unwrap();

        let mut expected = checklist();
        expected.items[0].checked = true;
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw, serde_json::to_string_pretty(&expected).unwrap());
    }

    #[test]
    fn an_agent_rewritten_checklist_loads_and_a_toggle_keeps_its_items() {
        let tmp = TempDir::new().unwrap();
        crate::storage::with_test_storage_root(tmp.path(), || {
            let (store, scope) = store_and_scope(&tmp);
            crate::test_support::write_sidecar_json::<ErChecklist>(
                tmp.path(),
                serde_json::json!({"version":1,"diff_hash":"hash","items":[{"id":"c1","text":"first"}]}),
            );
            let mut app = crate::app::App::new_for_test(vec![]);
            app.tab_mut().er_root = crate::paths::ErRoot::managed(tmp.path().to_string_lossy());
            app.tab_mut().ai.checklist = Some(loaded(&store, &scope));
            // The agent rewrites the checklist after the host loaded it.
            crate::test_support::write_sidecar_json::<ErChecklist>(
                tmp.path(),
                serde_json::json!({"version":1,"diff_hash":"hash","items":[{"id":"c1","text":"first"},{"id":"c2","text":"added by the agent"}]}),
            );

            app.toggle_checklist_item_at(0).unwrap();

            let after = loaded(&store, &scope);
            assert!(after.items[0].checked);
            assert_eq!(
                after.items.len(),
                2,
                "the agent's rewrite was lost: {after:?}"
            );
            assert_eq!(after.items[1].text, "added by the agent");
            assert!(!after.items[1].checked);
        });
    }

    #[test]
    fn toggling_an_item_the_agent_removed_returns_none_and_keeps_the_bytes() {
        let tmp = TempDir::new().unwrap();
        crate::storage::with_test_storage_root(tmp.path(), || {
            let (store, scope) = store_and_scope(&tmp);
            let path = store.path(&key::<ErChecklist>(&scope).unwrap());
            crate::test_support::write_sidecar_json::<ErChecklist>(
                tmp.path(),
                serde_json::json!({"version":1,"diff_hash":"hash","items":[{"id":"c1","text":"first"},{"id":"c2","text":"second"}]}),
            );
            let mut app = crate::app::App::new_for_test(vec![]);
            app.tab_mut().er_root = crate::paths::ErRoot::managed(tmp.path().to_string_lossy());
            app.tab_mut().ai.checklist = Some(loaded(&store, &scope));
            // The agent rewrites the checklist without the item the host loaded.
            crate::test_support::write_sidecar_json::<ErChecklist>(
                tmp.path(),
                serde_json::json!({"version":1,"diff_hash":"hash","items":[{"id":"c1","text":"first"}]}),
            );
            let rewritten = std::fs::read_to_string(&path).unwrap();

            assert_eq!(app.toggle_checklist_item_at(1).unwrap(), None);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), rewritten);
        });
    }

    #[test]
    fn a_newer_checklist_is_not_overwritten() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = store.path(&key::<ErChecklist>(&scope).unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let newer = r#"{"version":99,"diff_hash":"hash","items":[{"id":"c1","text":"first"}]}"#;
        std::fs::write(&path, newer).unwrap();

        let error = set_checked(&store, &scope, "c1", true).unwrap_err();
        assert!(matches!(
            error,
            UpdateError::Store(StoreError::NewerVersion { found: 99, .. })
        ));
        assert_eq!(std::fs::read_to_string(path).unwrap(), newer);
    }

    #[test]
    fn setting_a_checklist_item_takes_the_scope_lock() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let guard = store.lock_scope(&scope, DEFAULT_LOCK_TIMEOUT).unwrap();
        let error = set_checked(&store, &scope, "c1", true).unwrap_err();
        assert!(matches!(
            error,
            UpdateError::Store(StoreError::AlreadyLocked { .. })
        ));
        drop(guard);
    }
}
