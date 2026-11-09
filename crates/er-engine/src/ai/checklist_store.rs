//! The single reader for `order.json` and the reader/writer for
//! `checklist.json`, on top of [`er_store::ArtifactStore`].
//!
//! Both stay `Bare` in 1.0: the agent writes them in place, so the bytes on
//! disk do not change. The host only rewrites the `checked` flags.

use std::path::PathBuf;

use er_store::{Artifact, ArtifactKey, ArtifactStore, Loaded, Mode, Registry, Scope, StoreError};

use crate::ai::{ErChecklist, ErOrder};

impl Artifact for ErOrder {
    const KIND: &'static str = "order";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Bare;
    const FILE: &'static str = "order.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl Artifact for ErChecklist {
    const KIND: &'static str = "checklist";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Bare;
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

/// Write the checklist and return its path for `mark_sidecar_written`.
pub fn save_checklist(
    store: &ArtifactStore,
    scope: &Scope,
    checklist: &ErChecklist,
) -> Result<PathBuf, StoreError> {
    let key = key::<ErChecklist>(scope)?;
    store.write(&key, checklist)?;
    Ok(store.path(&key))
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

    #[test]
    fn toggle_writes_the_same_bytes_as_before() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let payload = checklist();
        save_checklist(&store, &scope, &payload).unwrap();
        let raw =
            std::fs::read_to_string(store.path(&key::<ErChecklist>(&scope).unwrap())).unwrap();
        assert_eq!(raw, serde_json::to_string_pretty(&payload).unwrap());
    }

    #[test]
    fn an_agent_rewritten_checklist_loads_and_a_toggle_keeps_its_items() {
        let tmp = TempDir::new().unwrap();
        let (store, scope) = store_and_scope(&tmp);
        let path = store.path(&key::<ErChecklist>(&scope).unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"version":1,"diff_hash":"hash","items":[{"id":"c1","text":"first"},{"id":"c2","text":"second"}]}"#,
        )
        .unwrap();
        let agent = match load_checklist(&store, &scope).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected the agent checklist, got {other:?}"),
        };

        let mut app = crate::app::App::new_for_test(vec![]);
        app.tab_mut().er_root = crate::paths::ErRoot::managed(tmp.path().to_string_lossy());
        app.tab_mut().ai.checklist = Some(agent);
        app.toggle_checklist_item_at(0).unwrap();

        let toggled = match load_checklist(&store, &scope).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected the rewritten checklist, got {other:?}"),
        };
        assert!(toggled.items[0].checked);
        assert!(!toggled.items[1].checked);
        assert_eq!(toggled.items[1].text, "second");
    }
}
