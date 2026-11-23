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
    cleanup_sidecars(er_dir, true)?;
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
    Ok(())
}

/// Remove the sidecars under one lock hold; `artifacts` adds the professor file
/// and every expert file, which only the full cleanup removes.
fn cleanup_sidecars(er_dir: &str, artifacts: bool) -> Result<(), er_store::StoreError> {
    let scope = er_store::Scope::dir(er_dir)?;
    let guard =
        crate::store_registry::store().lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)?;
    delete_common_sidecars(&scope, &guard);
    if artifacts {
        delete_review_artifacts(&scope, &guard);
    }
    if let Ok(file) = er_store::RelPath::new("summary.md") {
        let _ = crate::store_registry::store().delete_text_locked(&guard, &file);
    }
    if artifacts {
        if let Ok(file) = er_store::RelPath::new("review-files.txt") {
            let _ = crate::store_registry::store().delete_text_locked(&guard, &file);
        }
    }
    Ok(())
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
