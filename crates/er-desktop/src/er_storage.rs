//! Re-export managed storage helpers from the shared engine crate.
pub use er_engine::storage::{
    branch_dir, legacy_cache_dir, migrate_into_managed, resolve_managed_root_from_slugs,
    slug_branch, slug_repo, slugify, storage_root, use_repo_local_storage,
};
