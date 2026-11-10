//! The process-wide artifact registry and store handle.
//!
//! The registry is built once per process; the store is a thin handle over the
//! managed storage root. Callers address artifacts through `ArtifactKey`s and
//! never build a store path by hand.

use std::path::PathBuf;

use er_store::{Artifact, ArtifactKey, ArtifactStore, Loaded, Registry, Scope, StoreError};

/// The engine's registered artifact kinds.
///
/// Registration is static. A duplicate kind or pattern is a programming error
/// that the register test catches, so the `expect` is deliberate and scoped.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    #[expect(
        clippy::expect_used,
        reason = "every kind is statically registered once; a duplicate fails the register test"
    )]
    {
        crate::ai::annotation_store::register(&mut registry)
            .expect("the annotation kinds register once");
        crate::ai::checklist_store::register(&mut registry)
            .expect("the checklist kinds register once");
        register_ai_kinds(&mut registry).expect("the AI kinds register once");
        #[cfg(feature = "ui")]
        crate::app::session_store::register(&mut registry)
            .expect("the session kinds register once");
        registry
            .register::<crate::projects_pins::ProjectsFile>()
            .expect("the projects kind registers once");
        crate::release_update::register(&mut registry)
            .expect("the update-check kind registers once");
    }
    registry
}

fn register_ai_kinds(registry: &mut Registry) -> Result<(), er_store::StoreError> {
    registry.register::<crate::ai::ErReview>()?;
    registry.register::<crate::ai::ExpertReview>()?;
    registry.register::<crate::ai::ProfessorReview>()?;
    registry.register::<crate::ai::TriageReview>()?;
    registry.register::<crate::ai::ArbiterReview>()?;
    registry.register::<crate::ai::ErTour>()?;
    registry.register::<crate::ai::ErDiagram>()?;
    registry.register_text("summary", "summary.md")?;
    Ok(())
}

/// Read one artifact from a bucket directory by slot.
///
/// Absent and unreadable both read as `None`: a sidecar an agent wrote badly is
/// skipped, never a load failure.
pub(crate) fn read_found<T: Artifact>(er_dir: &str, slot: T::Slot) -> Option<T> {
    if er_dir.is_empty() {
        return None;
    }
    let scope = Scope::dir(er_dir).ok()?;
    let key = ArtifactKey::<T>::new(scope, slot).ok()?;
    match store().read(&key).ok()? {
        Loaded::Found(stored) => Some(stored.value),
        _ => None,
    }
}

/// The artifact store rooted at managed storage.
pub fn store() -> ArtifactStore {
    ArtifactStore::new(crate::storage::storage_root())
}

/// The key for a single-file, app-scoped record.
pub fn app_key<T: Artifact<Slot = ()>>() -> Result<ArtifactKey<T>, StoreError> {
    ArtifactKey::new(Scope::App, ())
}

/// Path of a single-file app record. `None` only when the kind's constant file
/// template is invalid, which the register test above rejects.
pub fn app_path<T: Artifact<Slot = ()>>() -> Option<PathBuf> {
    app_key::<T>().ok().map(|key| store().path(&key))
}

/// Last-modified time of an app record, for callers that watch it. This is the
/// one filesystem read the store's own module allows: the record itself is
/// still read through [`ArtifactStore::read`].
pub fn app_modified<T: Artifact<Slot = ()>>() -> Option<std::time::SystemTime> {
    let path = app_path::<T>()?;
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_registers_every_kind() {
        let registry = registry();
        for (file, kind) in [
            ("questions.json", "questions"),
            ("notes.json", "notes"),
            ("github-comments.json", "github-comments"),
            ("session.json", "session"),
            ("reviewed.json", "reviewed"),
            ("order.json", "order"),
            ("checklist.json", "checklist"),
            ("review.json", "review"),
            ("experts/security.json", "expert"),
            ("professor.json", "professor"),
            ("triage.json", "triage"),
            ("arbiter.json", "arbiter"),
            ("tour.json", "tour"),
            ("diagrams/flows.json", "diagram"),
            ("summary.md", "summary"),
            ("projects.json", "projects"),
            ("update-check.json", "update-check"),
        ] {
            assert_eq!(
                registry.kind_for(std::path::Path::new(file)),
                Some(kind),
                "{file}"
            );
        }
    }

    #[test]
    fn registering_a_kind_twice_fails() {
        let mut registry = registry();
        assert!(crate::ai::annotation_store::register(&mut registry).is_err());
        assert!(crate::ai::checklist_store::register(&mut registry).is_err());
        assert!(register_ai_kinds(&mut registry).is_err());
        assert!(crate::app::session_store::register(&mut registry).is_err());
    }
}
