//! The process-wide artifact registry and store handle.
//!
//! The registry is built once per process; the store is a thin handle over the
//! managed storage root. Callers address artifacts through `ArtifactKey`s and
//! never build a store path by hand.

use er_store::{ArtifactStore, Registry};

/// The engine's registered artifact kinds.
///
/// Registration is static. A duplicate kind or pattern is a programming error
/// that the register test catches, so the `expect` is deliberate and scoped.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    #[expect(
        clippy::expect_used,
        reason = "the three annotation kinds are statically registered once; a duplicate fails the register test"
    )]
    crate::ai::annotation_store::register(&mut registry)
        .expect("the annotation kinds register once");
    registry
}

/// The artifact store rooted at managed storage.
pub fn store() -> ArtifactStore {
    ArtifactStore::new(crate::storage::storage_root())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_registers_the_three_annotation_kinds() {
        let registry = registry();
        for (file, kind) in [
            ("questions.json", "questions"),
            ("notes.json", "notes"),
            ("github-comments.json", "github-comments"),
        ] {
            assert_eq!(
                registry.kind_for(std::path::Path::new(file)),
                Some(kind),
                "{file}"
            );
        }
    }

    #[test]
    fn registering_the_annotation_kinds_twice_fails() {
        let mut registry = registry();
        assert!(crate::ai::annotation_store::register(&mut registry).is_err());
    }
}
