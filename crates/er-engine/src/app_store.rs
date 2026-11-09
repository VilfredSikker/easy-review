//! App-scoped records in the artifact store.
//!
//! Records that belong to the app rather than to a repository — projects, inbox
//! state, the PR caches, tabs and the update check — live under `app/` in the
//! store. One kind is one file; callers address it through its [`ArtifactKey`].

use std::path::PathBuf;

use er_store::{Artifact, ArtifactKey, ArtifactStore, Scope, StoreError};

/// The store the app's own records live in.
pub fn store() -> ArtifactStore {
    ArtifactStore::new(crate::storage::storage_root())
}

/// The key for a single-file app record.
pub fn key<T: Artifact<Slot = ()>>() -> Result<ArtifactKey<T>, StoreError> {
    ArtifactKey::new(Scope::App, ())
}

/// Path of a single-file app record. `None` only when the kind's constant file
/// template is invalid, which the store's registry test rejects.
pub fn path<T: Artifact<Slot = ()>>() -> Option<PathBuf> {
    key::<T>().ok().map(|key| store().path(&key))
}
