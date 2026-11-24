//! Deleting a bucket's review artifacts: the triage alone, or the reviews
//! with the checklist, order and summary they produced. Both front ends and
//! the workspace call these, so they need no App.

/// Delete AI review sidecar files.
///
/// A busy bucket returns the lock error instead of reporting the review gone
/// while its files stay behind.
pub fn cleanup_reviews(er_dir: &str) -> Result<(), er_store::StoreError> {
    cleanup_sidecars(er_dir, false)
}

/// Delete triage sidecar only. A busy bucket returns the lock error.
pub fn cleanup_triage(er_dir: &str) -> Result<(), er_store::StoreError> {
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(er_dir)?;
    let guard = store.lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)?;
    if let Ok(key) = er_store::ArtifactKey::<crate::ai::TriageReview>::new(scope, ()) {
        let _ = store.delete_locked(&guard, &key);
    }
    Ok(())
}

/// Delete all AI review artifacts (general, expert, professor). Leaves triage intact.
pub fn cleanup_review_artifacts(er_dir: &str) -> Result<(), er_store::StoreError> {
    cleanup_sidecars(er_dir, true)
}

/// Remove the sidecars under one lock hold; `artifacts` adds the professor file
/// and every expert file, which only the full cleanup removes.
fn cleanup_sidecars(er_dir: &str, artifacts: bool) -> Result<(), er_store::StoreError> {
    let scope = er_store::Scope::dir(er_dir)?;
    let guard =
        crate::store_registry::store().lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)?;
    delete_common_sidecars(&scope, &guard);
    if artifacts {
        unlink_linked_dirs(er_dir);
        delete_review_artifacts(&scope, &guard);
    }
    if let Ok(file) = er_store::RelPath::new("summary.md") {
        let _ = crate::store_registry::store().delete_text_locked(&guard, &file);
    }
    if artifacts {
        if let Ok(file) = er_store::RelPath::new("review-files.txt") {
            let _ = crate::store_registry::store().delete_text_locked(&guard, &file);
        }
        delete_raw_leftovers(er_dir);
    }
    Ok(())
}

/// The files no store kind names, removed while the caller holds the lock.
fn delete_raw_leftovers(er_dir: &str) {
    // `experts/<id>.prev.json` is a scoped re-review snapshot, not a store kind,
    // so it and any other JSON left in the directory go through the raw path.
    let experts_dir = std::path::Path::new(er_dir).join("experts");
    if let Ok(entries) = std::fs::read_dir(&experts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    // A cleaned bucket holds no stale overlay or outcome; removing the whole
    // directory also clears any file an older overlay kind left behind.
    let _ = std::fs::remove_dir_all(std::path::Path::new(er_dir).join("scoped"));
}

/// A linked `experts` or `scoped` directory points outside the bucket, so
/// the cleanup removes the link itself and never lists or walks through it.
fn unlink_linked_dirs(er_dir: &str) {
    let bucket = std::path::Path::new(er_dir);
    // `scoped` comes before `scoped/experts`: once a linked `scoped` is gone,
    // the inner path no longer resolves through it.
    for dir in ["experts", "scoped", "scoped/experts"] {
        let path = bucket.join(dir);
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn delete_common_sidecars(scope: &er_store::Scope, guard: &er_store::ScopeGuard) {
    let store = crate::store_registry::store();
    if let Ok(key) = er_store::ArtifactKey::<crate::ai::ErReview>::new(scope.clone(), ()) {
        let _ = store.delete_locked(guard, &key);
    }
    if let Ok(key) = er_store::ArtifactKey::<crate::ai::ErOrder>::new(scope.clone(), ()) {
        let _ = store.delete_locked(guard, &key);
    }
    if let Ok(key) = er_store::ArtifactKey::<crate::ai::ErChecklist>::new(scope.clone(), ()) {
        let _ = store.delete_locked(guard, &key);
    }
}

fn delete_review_artifacts(scope: &er_store::Scope, guard: &er_store::ScopeGuard) {
    let store = crate::store_registry::store();
    if let Ok(key) = er_store::ArtifactKey::<crate::ai::ProfessorReview>::new(scope.clone(), ()) {
        let _ = store.delete_locked(guard, &key);
    }
    if let Ok(slots) = store.list::<crate::ai::ExpertReview>(scope) {
        for slot in slots {
            if let Ok(key) =
                er_store::ArtifactKey::<crate::ai::ExpertReview>::new(scope.clone(), slot)
            {
                let _ = store.delete_locked(guard, &key);
            }
        }
    }
    // Scoped overlays and validation are review state: the full cleanup drops
    // them, while the per-lens clearing at run completion drops only its own.
    if let Ok(key) = er_store::ArtifactKey::<crate::ai::ValidationReview>::new(scope.clone(), ()) {
        let _ = store.delete_locked(guard, &key);
    }
    if let Ok(key) = er_store::ArtifactKey::<crate::ai::ScopedReview>::new(scope.clone(), ()) {
        let _ = store.delete_locked(guard, &key);
    }
    if let Ok(key) =
        er_store::ArtifactKey::<crate::ai::ScopedProfessorReview>::new(scope.clone(), ())
    {
        let _ = store.delete_locked(guard, &key);
    }
    if let Ok(slots) = store.list::<crate::ai::ScopedExpertReview>(scope) {
        for slot in slots {
            if let Ok(key) =
                er_store::ArtifactKey::<crate::ai::ScopedExpertReview>::new(scope.clone(), slot)
            {
                let _ = store.delete_locked(guard, &key);
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A bucket whose `experts` and `scoped/experts` are links to directories
    /// outside it: the full cleanup removes the links and every file outside
    /// survives, the ones named like expert results included.
    #[test]
    fn a_linked_expert_directory_is_unlinked_and_never_walked() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let (bucket, outside) = (dir.path().join("bucket"), dir.path().join("outside"));
            let (experts, scoped) = (outside.join("experts"), outside.join("scoped"));
            for at in [&bucket.join("scoped"), &experts, &scoped] {
                std::fs::create_dir_all(at).unwrap();
            }
            for file in [
                experts.join("sec.json"),
                experts.join("keep.json"),
                scoped.join("sec.json"),
            ] {
                std::fs::write(file, "{}").unwrap();
            }
            std::os::unix::fs::symlink(&experts, bucket.join("experts")).unwrap();
            std::os::unix::fs::symlink(&scoped, bucket.join("scoped/experts")).unwrap();

            cleanup_review_artifacts(bucket.to_str().unwrap()).unwrap();

            for kept in ["experts/sec.json", "experts/keep.json", "scoped/sec.json"] {
                assert!(
                    outside.join(kept).exists(),
                    "{kept} outside the bucket was deleted"
                );
            }
            assert!(std::fs::symlink_metadata(bucket.join("experts")).is_err());
            assert!(!bucket.join("scoped").exists());
        });
    }

    /// A bucket whose `scoped` is itself a link to a directory outside it:
    /// the scoped expert listing never reads through it, and the file there
    /// survives.
    #[test]
    fn a_linked_scoped_directory_is_unlinked_and_never_walked() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let (bucket, outside) = (dir.path().join("bucket"), dir.path().join("outside"));
            std::fs::create_dir_all(&bucket).unwrap();
            std::fs::create_dir_all(outside.join("experts")).unwrap();
            std::fs::write(outside.join("experts/sec.json"), "{}").unwrap();
            std::os::unix::fs::symlink(&outside, bucket.join("scoped")).unwrap();

            cleanup_review_artifacts(bucket.to_str().unwrap()).unwrap();

            assert!(
                outside.join("experts/sec.json").exists(),
                "experts/sec.json outside the bucket was deleted"
            );
            assert!(std::fs::symlink_metadata(bucket.join("scoped")).is_err());
        });
    }
}
