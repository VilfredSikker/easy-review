//! Archive and validate the producer sidecars owned by one review flow.

use anyhow::{bail, Context, Result};
use er_store::{
    Artifact, ArtifactKey, ArtifactStore, BackupSnapshot, ExpertId, Loaded, RelPath, Scope,
    Unreadable,
};
use std::path::{Path, PathBuf};

use crate::ai::{expert_by_id, ErReview, ExpertReview, TriageReview};

const BACKUP_DIR: &str = "flow-backups";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FlowProducer {
    Triage,
    General,
    Expert(String),
}

impl FlowProducer {
    fn relative_output(&self) -> Result<PathBuf> {
        match self {
            Self::Triage => Ok(PathBuf::from("triage.json")),
            Self::General => Ok(PathBuf::from("review.json")),
            Self::Expert(id) => {
                if expert_by_id(id).is_none() {
                    bail!("unknown review-flow expert id: {id}");
                }
                Ok(PathBuf::from("experts").join(format!("{id}.json")))
            }
        }
    }

    fn output_key(&self, scope: &Scope) -> Result<OutputKey> {
        Ok(match self {
            Self::Triage => OutputKey::Triage(ArtifactKey::new(scope.clone(), ())?),
            Self::General => OutputKey::General(ArtifactKey::new(scope.clone(), ())?),
            Self::Expert(id) => {
                if expert_by_id(id).is_none() {
                    bail!("unknown review-flow expert id: {id}");
                }
                let slot = ExpertId::new(id)?;
                OutputKey::Expert(ArtifactKey::new(scope.clone(), slot)?)
            }
        })
    }
}

/// The typed key a producer writes, so each archive and validate call reaches
/// the store through the kind's own path.
enum OutputKey {
    Triage(ArtifactKey<TriageReview>),
    General(ArtifactKey<ErReview>),
    Expert(ArtifactKey<ExpertReview>),
}

/// Whether the destination holds output, and whether that output is usable.
struct OutputState {
    present: bool,
    well_formed: bool,
}

/// A recoverable copy of the output that existed before a flow stage started.
///
/// The archived file is intentionally retained after restoration. Flow backups
/// are recovery material, not temporary files.
#[derive(Debug, Clone)]
pub(super) struct FlowOutputBackup {
    scope: Scope,
    producer: FlowProducer,
    archived: Option<BackupSnapshot>,
    backup_dir: RelPath,
}

impl FlowOutputBackup {
    #[cfg(test)]
    pub(super) fn archive_path(&self) -> Option<&Path> {
        self.archived.as_ref().map(BackupSnapshot::backup_path)
    }

    /// The archived file, for a caller that must compare it or read it back.
    pub(super) fn archived(&self) -> Option<&BackupSnapshot> {
        self.archived.as_ref()
    }

    /// Restore the previous file only when the stage left no usable output.
    ///
    /// A parseable producer output is left in place, even if it belongs to a
    /// different diff. The caller can then report that output as stale without
    /// risking replacement of a newer generation. Malformed or wrong-expert
    /// output is moved into this run's backup before restoration.
    ///
    /// The caller must ensure the owned writer has stopped before restoring.
    pub(super) fn restore(&self) -> Result<()> {
        let store = crate::store_registry::store();
        let key = self.producer.output_key(&self.scope)?;
        let state = output_state(&store, &key)?;
        let Some(archived) = &self.archived else {
            if state.present && !state.well_formed {
                self.preserve_replacement(&store, &key)?;
            }
            return Ok(());
        };
        if state.present {
            if state.well_formed {
                return Ok(());
            }
            self.preserve_replacement(&store, &key)?;
        }
        store
            .restore_backup(archived)
            .context("restore flow output")
    }

    fn preserve_replacement(&self, store: &ArtifactStore, key: &OutputKey) -> Result<()> {
        let replacements = RelPath::new(self.backup_dir.as_path().join("replacements"))?;
        archive_output(store, key, &replacements).context("preserve invalid flow output")?;
        Ok(())
    }
}

/// Move an existing producer output into a fresh bucket-local run archive.
/// A run key is one path component and may not be reused, even when the old
/// output is absent.
pub(super) fn archive_flow_output(
    er_dir: &Path,
    run_key: &str,
    producer: &FlowProducer,
) -> Result<FlowOutputBackup> {
    validate_run_key(run_key)?;
    let store = crate::store_registry::store();
    let scope = Scope::dir(er_dir)?;
    let key = producer.output_key(&scope)?;
    let destination = er_dir.join(producer.relative_output()?);
    // Reject an unsafe destination before a backup directory exists; the
    // archive checks again under the lock.
    check_output_path(&store, &key)
        .with_context(|| format!("archive flow output {}", destination.display()))?;
    let backup_dir = RelPath::new(Path::new(BACKUP_DIR).join(run_key))?;
    store
        .create_backup_dir(&scope, &backup_dir)
        .with_context(|| {
            format!(
                "create unique flow backup {}",
                backup_dir.as_path().display()
            )
        })?;

    let archived = match archive_output(&store, &key, &backup_dir) {
        Ok(archived) => archived,
        Err(error) => {
            let _ = store.remove_backup_dir(&scope, &backup_dir);
            return Err(error)
                .with_context(|| format!("archive flow output {}", destination.display()));
        }
    };

    Ok(FlowOutputBackup {
        scope,
        producer: producer.clone(),
        archived,
        backup_dir,
    })
}

fn check_output_path(store: &ArtifactStore, key: &OutputKey) -> Result<()> {
    match key {
        OutputKey::Triage(key) => Ok(store.check_artifact_path(key)?),
        OutputKey::General(key) => Ok(store.check_artifact_path(key)?),
        OutputKey::Expert(key) => Ok(store.check_artifact_path(key)?),
    }
}

fn archive_output(
    store: &ArtifactStore,
    key: &OutputKey,
    backup_dir: &RelPath,
) -> Result<Option<BackupSnapshot>> {
    Ok(match key {
        OutputKey::Triage(key) => store.archive_to_dir(key, backup_dir)?,
        OutputKey::General(key) => store.archive_to_dir(key, backup_dir)?,
        OutputKey::Expert(key) => store.archive_to_dir(key, backup_dir)?,
    })
}

/// Parse the raw sidecar for `producer` and require the exact prepared diff
/// hash. Expert files must also identify the requested registered expert.
pub(super) fn validate_flow_output(
    er_dir: &Path,
    producer: &FlowProducer,
    diff_hash: &str,
) -> Result<()> {
    let store = crate::store_registry::store();
    let scope = Scope::dir(er_dir)?;
    let key = producer.output_key(&scope)?;
    let destination = er_dir.join(producer.relative_output()?);
    let actual_hash = match &key {
        OutputKey::Triage(key) => {
            read_output::<TriageReview>(&store, key)
                .with_context(|| format!("read flow output {}", destination.display()))?
                .diff_hash
        }
        OutputKey::General(key) => {
            read_output::<ErReview>(&store, key)
                .with_context(|| format!("read flow output {}", destination.display()))?
                .diff_hash
        }
        OutputKey::Expert(key) => {
            let output = read_output::<ExpertReview>(&store, key)
                .with_context(|| format!("read flow output {}", destination.display()))?;
            if output.expert_id != key.slot().as_str() {
                bail!(
                    "expert output identity mismatch: expected {}, got {}",
                    key.slot().as_str(),
                    output.expert_id
                );
            }
            output.diff_hash
        }
    };
    if actual_hash != diff_hash {
        bail!("flow output diff hash mismatch: expected {diff_hash}, got {actual_hash}");
    }
    Ok(())
}

fn read_output<T: Artifact>(store: &ArtifactStore, key: &ArtifactKey<T>) -> Result<T> {
    match store.snapshot(key)? {
        Loaded::Found(stored) => match stored.value.parse() {
            Loaded::Found(parsed) => Ok(parsed.value),
            Loaded::Unreadable(reason) => bail!("parse flow output: {}", describe(&reason)),
            Loaded::Absent => bail!("flow output is missing"),
        },
        Loaded::Absent => bail!("flow output is missing"),
        Loaded::Unreadable(reason) => bail!("read flow output: {}", describe(&reason)),
    }
}

fn output_state(store: &ArtifactStore, key: &OutputKey) -> Result<OutputState> {
    match key {
        OutputKey::Triage(key) => inspect(store, key, |_| true),
        OutputKey::General(key) => inspect(store, key, |_| true),
        OutputKey::Expert(key) => inspect(store, key, |value: &ExpertReview| {
            value.expert_id == key.slot().as_str()
        }),
    }
}

fn inspect<T: Artifact>(
    store: &ArtifactStore,
    key: &ArtifactKey<T>,
    acceptable: impl Fn(&T) -> bool,
) -> Result<OutputState> {
    Ok(match store.snapshot(key)? {
        Loaded::Absent => OutputState {
            present: false,
            well_formed: false,
        },
        Loaded::Unreadable(_) => OutputState {
            present: true,
            well_formed: false,
        },
        Loaded::Found(stored) => OutputState {
            present: true,
            well_formed: matches!(stored.value.parse(), Loaded::Found(parsed) if acceptable(&parsed.value)),
        },
    })
}

fn describe(reason: &Unreadable) -> String {
    match reason {
        Unreadable::Malformed(reason) => reason.clone(),
        Unreadable::UnknownVersion { found } => format!("unknown version {found}"),
        Unreadable::NewerVersion { found } => format!("newer version {found}"),
        Unreadable::WrongKind { found } => format!("wrong kind {found}"),
        Unreadable::TooLarge { bytes } => format!("{bytes} bytes is over the store limit"),
    }
}

fn validate_run_key(run_key: &str) -> Result<()> {
    if run_key.is_empty()
        || run_key == "."
        || run_key == ".."
        || !run_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        bail!("unsafe flow output run key");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    fn output<T: Serialize>(path: &Path, value: &T) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn general(hash: &str) -> ErReview {
        ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: Default::default(),
            file_hashes: Default::default(),
        }
    }

    fn triage(hash: &str) -> TriageReview {
        serde_json::from_value(serde_json::json!({"version":1,"diff_hash":hash})).unwrap()
    }

    fn expert(id: &str, hash: &str) -> ExpertReview {
        serde_json::from_value(serde_json::json!({"version":1,"expert_id":id,"diff_hash":hash}))
            .unwrap()
    }

    #[test]
    fn archives_even_same_hash_output_and_leaves_other_experts_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        crate::storage::with_test_storage_root(root, || {
            output(&root.join("triage.json"), &triage("same"));
            output(
                &root.join("experts/security.json"),
                &expert("security", "same"),
            );
            output(
                &root.join("experts/testing.json"),
                &expert("testing", "other"),
            );

            let backup = archive_flow_output(root, "run-1", &FlowProducer::Triage).unwrap();
            assert!(!root.join("triage.json").exists());
            assert_eq!(
                backup.archive_path().unwrap(),
                std::fs::canonicalize(root)
                    .unwrap()
                    .join("flow-backups/run-1/triage.json")
            );
            assert!(root.join("experts/security.json").exists());
            assert!(root.join("experts/testing.json").exists());
        });
    }

    #[test]
    fn validates_hash_and_expert_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        crate::storage::with_test_storage_root(root, || {
            output(&root.join("review.json"), &general("old"));
            assert!(validate_flow_output(root, &FlowProducer::General, "new").is_err());
            output(&root.join("review.json"), &general("new"));
            assert!(validate_flow_output(root, &FlowProducer::General, "new").is_ok());

            output(
                &root.join("experts/security.json"),
                &expert("testing", "new"),
            );
            assert!(
                validate_flow_output(root, &FlowProducer::Expert("security".into()), "new")
                    .is_err()
            );
            assert!(
                archive_flow_output(root, "run-2", &FlowProducer::Expert("unknown".into()))
                    .is_err()
            );
        });
    }

    #[test]
    fn rejects_missing_malformed_and_oversized_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        crate::storage::with_test_storage_root(root, || {
            assert!(validate_flow_output(root, &FlowProducer::General, "h").is_err());
            std::fs::write(root.join("review.json"), b"{").unwrap();
            assert!(validate_flow_output(root, &FlowProducer::General, "h").is_err());
            std::fs::write(root.join("review.json"), vec![b'x'; 10_000_001]).unwrap();
            assert!(validate_flow_output(root, &FlowProducer::General, "h").is_err());
        });
    }

    #[test]
    fn restores_missing_and_preserves_invalid_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        crate::storage::with_test_storage_root(root, || {
            output(&root.join("review.json"), &general("old"));
            let backup = archive_flow_output(root, "run-restore", &FlowProducer::General).unwrap();
            backup.restore().unwrap();
            assert_eq!(
                serde_json::from_slice::<ErReview>(
                    &std::fs::read(root.join("review.json")).unwrap()
                )
                .unwrap()
                .diff_hash,
                "old"
            );

            let backup = archive_flow_output(root, "run-invalid", &FlowProducer::General).unwrap();
            std::fs::write(root.join("review.json"), b"not json").unwrap();
            backup.restore().unwrap();
            assert_eq!(
                serde_json::from_slice::<ErReview>(
                    &std::fs::read(root.join("review.json")).unwrap()
                )
                .unwrap()
                .diff_hash,
                "old"
            );
            assert_eq!(
                std::fs::read(
                    root.join(backup.backup_dir.as_path())
                        .join("replacements/review.json")
                )
                .unwrap(),
                b"not json"
            );
        });
    }

    #[test]
    fn leaves_valid_replacement_and_keeps_backup_as_a_separate_copy() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        crate::storage::with_test_storage_root(root, || {
            output(&root.join("review.json"), &general("old"));
            let backup = archive_flow_output(root, "run-valid", &FlowProducer::General).unwrap();
            output(&root.join("review.json"), &general("new"));

            backup.restore().unwrap();

            let current: ErReview =
                serde_json::from_slice(&std::fs::read(root.join("review.json")).unwrap()).unwrap();
            let archived: ErReview =
                serde_json::from_slice(&std::fs::read(backup.archive_path().unwrap()).unwrap())
                    .unwrap();
            assert_eq!(current.diff_hash, "new");
            assert_eq!(archived.diff_hash, "old");
        });
    }

    #[test]
    fn refuses_archive_collision_and_unsafe_run_keys() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        crate::storage::with_test_storage_root(root, || {
            archive_flow_output(root, "same", &FlowProducer::General).unwrap();
            assert!(archive_flow_output(root, "same", &FlowProducer::General).is_err());
            assert!(archive_flow_output(root, "../escape", &FlowProducer::General).is_err());
        });
    }

    #[cfg(unix)]
    mod symlink_tests {
        use super::*;
        use std::os::unix::fs::symlink;

        #[test]
        fn archive_rejects_symlinked_expert_directory_and_backup_root() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("bucket");
            let outside = temp.path().join("outside");
            crate::storage::with_test_storage_root(&root, || {
                std::fs::create_dir_all(&root).unwrap();
                std::fs::create_dir_all(&outside).unwrap();
                output(
                    &outside.join("security.json"),
                    &expert("security", "outside"),
                );
                symlink(&outside, root.join("experts")).unwrap();
                assert!(archive_flow_output(
                    &root,
                    "expert-link",
                    &FlowProducer::Expert("security".into())
                )
                .is_err());
                assert!(outside.join("security.json").exists());
                assert!(!root.join("flow-backups/expert-link").exists());

                std::fs::remove_file(root.join("experts")).unwrap();
                std::fs::write(root.join("triage.json"), b"original").unwrap();
                symlink(&outside, root.join("flow-backups")).unwrap();
                assert!(archive_flow_output(&root, "backup-link", &FlowProducer::Triage).is_err());
                assert_eq!(
                    std::fs::read(root.join("triage.json")).unwrap(),
                    b"original"
                );
                assert!(outside.join("security.json").exists());
            });
        }

        #[test]
        fn validation_rejects_symlinked_output_without_reading_outside_file() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("bucket");
            let outside = temp.path().join("outside.json");
            crate::storage::with_test_storage_root(&root, || {
                std::fs::create_dir_all(&root).unwrap();
                output(&outside, &general("expected"));
                symlink(&outside, root.join("review.json")).unwrap();

                assert!(validate_flow_output(&root, &FlowProducer::General, "expected").is_err());
                assert_eq!(
                    serde_json::from_slice::<ErReview>(&std::fs::read(outside).unwrap())
                        .unwrap()
                        .diff_hash,
                    "expected"
                );
            });
        }

        #[test]
        fn restore_rejects_symlinked_destination_and_archive() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("bucket");
            let outside = temp.path().join("outside");
            crate::storage::with_test_storage_root(&root, || {
                std::fs::create_dir_all(&root).unwrap();
                std::fs::create_dir_all(&outside).unwrap();
                output(
                    &root.join("experts/security.json"),
                    &expert("security", "old"),
                );
                let backup = archive_flow_output(
                    &root,
                    "destination-link",
                    &FlowProducer::Expert("security".into()),
                )
                .unwrap();

                std::fs::remove_dir(root.join("experts")).unwrap();
                output(
                    &outside.join("security.json"),
                    &expert("security", "outside"),
                );
                symlink(&outside, root.join("experts")).unwrap();
                assert!(backup.restore().is_err());
                assert_eq!(
                    serde_json::from_slice::<ExpertReview>(
                        &std::fs::read(outside.join("security.json")).unwrap()
                    )
                    .unwrap()
                    .diff_hash,
                    "outside"
                );

                std::fs::remove_file(root.join("experts")).unwrap();
                std::fs::create_dir(root.join("experts")).unwrap();
                let archived = backup.archive_path().unwrap();
                std::fs::remove_file(archived).unwrap();
                symlink(outside.join("security.json"), archived).unwrap();
                assert!(backup.restore().is_err());
                assert_eq!(
                    serde_json::from_slice::<ExpertReview>(
                        &std::fs::read(outside.join("security.json")).unwrap()
                    )
                    .unwrap()
                    .diff_hash,
                    "outside"
                );
            });
        }

        #[test]
        fn replacement_preservation_rejects_symlinked_backup_component() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("bucket");
            let outside = temp.path().join("outside");
            crate::storage::with_test_storage_root(&root, || {
                std::fs::create_dir_all(&root).unwrap();
                std::fs::create_dir_all(&outside).unwrap();
                output(&root.join("review.json"), &general("old"));
                output(&outside.join("review.json"), &general("outside"));
                let backup =
                    archive_flow_output(&root, "replacement-link", &FlowProducer::General).unwrap();
                std::fs::write(root.join("review.json"), b"malformed").unwrap();
                symlink(
                    &outside,
                    root.join("flow-backups/replacement-link/replacements"),
                )
                .unwrap();

                assert!(backup.restore().is_err());
                assert_eq!(
                    std::fs::read(root.join("review.json")).unwrap(),
                    b"malformed"
                );
                assert_eq!(
                    serde_json::from_slice::<ErReview>(
                        &std::fs::read(outside.join("review.json")).unwrap()
                    )
                    .unwrap()
                    .diff_hash,
                    "outside"
                );
            });
        }
    }
}
