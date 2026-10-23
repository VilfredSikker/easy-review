//! Archive and validate the producer sidecars owned by one review flow.

use anyhow::{bail, Context, Result};
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use crate::ai::{expert_by_id, read_sidecar, ErReview, ExpertReview, TriageReview};

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
}

/// A recoverable copy of the output that existed before a flow stage started.
///
/// The archived file is intentionally retained after restoration. Flow backups
/// are recovery material, not temporary files.
#[derive(Debug, Clone)]
pub(super) struct FlowOutputBackup {
    bucket_root: PathBuf,
    destination: PathBuf,
    archived: Option<PathBuf>,
    backup_dir: PathBuf,
    producer: FlowProducer,
}

impl FlowOutputBackup {
    pub(super) fn archive_path(&self) -> Option<&Path> {
        self.archived.as_deref()
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
        let destination_relative = self
            .destination
            .strip_prefix(&self.bucket_root)
            .context("flow output destination escaped its bucket")?;
        reject_symlink_components(&self.bucket_root, destination_relative)?;

        let Some(archived) = &self.archived else {
            if self.destination.exists()
                && !producer_output_is_well_formed(&self.destination, &self.producer)
            {
                self.preserve_replacement()?;
            }
            return Ok(());
        };
        let archived_relative = archived
            .strip_prefix(&self.bucket_root)
            .context("flow archive escaped its bucket")?;
        reject_symlink_components(&self.bucket_root, archived_relative)?;

        if self.destination.exists() {
            if producer_output_is_well_formed(&self.destination, &self.producer) {
                return Ok(());
            }
            self.preserve_replacement()?;
        }

        let parent = self
            .destination
            .parent()
            .context("flow output destination has no parent")?;
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        reject_symlink_components(&self.bucket_root, destination_relative)?;
        restore_copy_without_clobber(archived, &self.destination)
    }

    fn preserve_replacement(&self) -> Result<()> {
        let destination_relative = self
            .destination
            .strip_prefix(&self.bucket_root)
            .context("flow output destination escaped its bucket")?;
        reject_symlink_components(&self.bucket_root, destination_relative)?;
        if !self.destination.exists() {
            return Ok(());
        }
        let replacement = self
            .backup_dir
            .join("replacements")
            .join(self.producer.relative_output()?);
        let replacement_relative = replacement
            .strip_prefix(&self.bucket_root)
            .context("replacement archive escaped its bucket")?;
        reject_symlink_components(&self.bucket_root, replacement_relative)?;
        if replacement.exists() {
            bail!(
                "replacement archive already exists at {}",
                replacement.display()
            );
        }
        let parent = replacement
            .parent()
            .context("replacement archive has no parent")?;
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        reject_symlink_components(&self.bucket_root, replacement_relative)?;
        fs::rename(&self.destination, &replacement).with_context(|| {
            format!(
                "preserve invalid flow output at {}",
                self.destination.display()
            )
        })?;
        Ok(())
    }
}

fn restore_copy_without_clobber(archived: &Path, destination: &Path) -> Result<()> {
    let mut source = fs::File::open(archived)
        .with_context(|| format!("open archived flow output {}", archived.display()))?;
    let mut target = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
    {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("restore flow output to {}", destination.display()));
        }
    };
    if let Err(error) = io::copy(&mut source, &mut target).and_then(|_| target.flush()) {
        drop(target);
        let _ = fs::remove_file(destination);
        return Err(error)
            .with_context(|| format!("copy archived output to {}", destination.display()));
    }
    target
        .sync_all()
        .with_context(|| format!("sync restored flow output {}", destination.display()))?;
    let permissions = fs::metadata(archived)
        .with_context(|| format!("inspect archived output {}", archived.display()))?
        .permissions();
    fs::set_permissions(destination, permissions)
        .with_context(|| format!("restore permissions on {}", destination.display()))?;
    Ok(())
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
    let bucket_root = canonical_bucket_root(er_dir)?;
    let relative = producer.relative_output()?;
    reject_symlink_components(&bucket_root, &relative)?;
    let destination = bucket_root.join(&relative);
    let backup_relative = Path::new(BACKUP_DIR).join(run_key);
    reject_symlink_components(&bucket_root, &backup_relative)?;
    let backup_dir = bucket_root.join(&backup_relative);

    let had_output = match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                bail!(
                    "flow output is not a regular file: {}",
                    destination.display()
                );
            }
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| format!("inspect {}", destination.display()));
        }
    };

    fs::create_dir_all(bucket_root.join(BACKUP_DIR))
        .with_context(|| format!("create flow backup root in {}", bucket_root.display()))?;
    reject_symlink_components(&bucket_root, &backup_relative)?;
    fs::create_dir(&backup_dir)
        .with_context(|| format!("create unique flow backup {}", backup_dir.display()))?;

    let archived = if had_output {
        let archived = backup_dir.join(&relative);
        let archived_relative = archived
            .strip_prefix(&bucket_root)
            .context("flow archive escaped its bucket")?;
        reject_symlink_components(&bucket_root, archived_relative)?;
        let parent = archived
            .parent()
            .context("flow archive path has no parent")?;
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        reject_symlink_components(&bucket_root, archived_relative)?;
        if let Err(error) = fs::rename(&destination, &archived) {
            let _ = fs::remove_dir_all(&backup_dir);
            return Err(error)
                .with_context(|| format!("archive flow output {}", destination.display()));
        }
        Some(archived)
    } else {
        None
    };

    Ok(FlowOutputBackup {
        bucket_root,
        destination,
        archived,
        backup_dir,
        producer: producer.clone(),
    })
}

/// Parse the raw sidecar for `producer` and require the exact prepared diff
/// hash. Expert files must also identify the requested registered expert.
pub(super) fn validate_flow_output(
    er_dir: &Path,
    producer: &FlowProducer,
    diff_hash: &str,
) -> Result<()> {
    let bucket_root = canonical_bucket_root(er_dir)?;
    let relative = producer.relative_output()?;
    reject_symlink_components(&bucket_root, &relative)?;
    let path = bucket_root.join(relative);
    let contents =
        read_sidecar(&path).with_context(|| format!("read flow output {}", path.display()))?;
    let actual_hash = match producer {
        FlowProducer::Triage => {
            serde_json::from_str::<TriageReview>(&contents)
                .context("parse triage flow output")?
                .diff_hash
        }
        FlowProducer::General => {
            serde_json::from_str::<ErReview>(&contents)
                .context("parse general flow output")?
                .diff_hash
        }
        FlowProducer::Expert(expected_id) => {
            let output = serde_json::from_str::<ExpertReview>(&contents)
                .context("parse expert flow output")?;
            if output.expert_id != *expected_id {
                bail!(
                    "expert output identity mismatch: expected {expected_id}, got {}",
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

fn canonical_bucket_root(er_dir: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(er_dir)
        .with_context(|| format!("resolve review bucket {}", er_dir.display()))?;
    if !root.is_dir() {
        bail!("review bucket is not a directory: {}", root.display());
    }
    Ok(root)
}

/// Reject symlinks along a bucket-relative path. The bucket root is already
/// canonicalized, so system-level aliases such as `/tmp` do not count as
/// components below the managed bucket.
fn reject_symlink_components(bucket_root: &Path, relative: &Path) -> Result<()> {
    if relative.is_absolute() {
        bail!("flow sidecar path must be bucket-relative");
    }
    let mut current = bucket_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            bail!("unsafe component in flow sidecar path");
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "flow sidecar path contains a symlink: {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error).with_context(|| format!("inspect {}", current.display()));
            }
        }
    }
    Ok(())
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

fn producer_output_is_well_formed(path: &Path, producer: &FlowProducer) -> bool {
    let Ok(contents) = read_sidecar(path) else {
        return false;
    };
    match producer {
        FlowProducer::Triage => serde_json::from_str::<TriageReview>(&contents).is_ok(),
        FlowProducer::General => serde_json::from_str::<ErReview>(&contents).is_ok(),
        FlowProducer::Expert(id) => serde_json::from_str::<ExpertReview>(&contents)
            .is_ok_and(|output| output.expert_id == *id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    fn output<T: Serialize>(path: &Path, value: &T) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
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
            fs::canonicalize(root)
                .unwrap()
                .join("flow-backups/run-1/triage.json")
        );
        assert!(root.join("experts/security.json").exists());
        assert!(root.join("experts/testing.json").exists());
    }

    #[test]
    fn validates_hash_and_expert_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        output(&root.join("review.json"), &general("old"));
        assert!(validate_flow_output(root, &FlowProducer::General, "new").is_err());
        output(&root.join("review.json"), &general("new"));
        assert!(validate_flow_output(root, &FlowProducer::General, "new").is_ok());

        output(
            &root.join("experts/security.json"),
            &expert("testing", "new"),
        );
        assert!(
            validate_flow_output(root, &FlowProducer::Expert("security".into()), "new").is_err()
        );
        assert!(
            archive_flow_output(root, "run-2", &FlowProducer::Expert("unknown".into())).is_err()
        );
    }

    #[test]
    fn rejects_missing_malformed_and_oversized_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        assert!(validate_flow_output(root, &FlowProducer::General, "h").is_err());
        fs::write(root.join("review.json"), b"{").unwrap();
        assert!(validate_flow_output(root, &FlowProducer::General, "h").is_err());
        fs::write(root.join("review.json"), vec![b'x'; 10_000_001]).unwrap();
        assert!(validate_flow_output(root, &FlowProducer::General, "h").is_err());
    }

    #[test]
    fn restores_missing_and_preserves_invalid_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        output(&root.join("review.json"), &general("old"));
        let backup = archive_flow_output(root, "run-restore", &FlowProducer::General).unwrap();
        backup.restore().unwrap();
        assert_eq!(
            serde_json::from_slice::<ErReview>(&fs::read(root.join("review.json")).unwrap())
                .unwrap()
                .diff_hash,
            "old"
        );

        let backup = archive_flow_output(root, "run-invalid", &FlowProducer::General).unwrap();
        fs::write(root.join("review.json"), b"not json").unwrap();
        backup.restore().unwrap();
        assert_eq!(
            serde_json::from_slice::<ErReview>(&fs::read(root.join("review.json")).unwrap())
                .unwrap()
                .diff_hash,
            "old"
        );
        assert_eq!(
            fs::read(backup.backup_dir.join("replacements/review.json")).unwrap(),
            b"not json"
        );
    }

    #[test]
    fn leaves_valid_replacement_and_keeps_backup_as_a_separate_copy() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        output(&root.join("review.json"), &general("old"));
        let backup = archive_flow_output(root, "run-valid", &FlowProducer::General).unwrap();
        output(&root.join("review.json"), &general("new"));

        backup.restore().unwrap();

        let current: ErReview =
            serde_json::from_slice(&fs::read(root.join("review.json")).unwrap()).unwrap();
        let archived: ErReview =
            serde_json::from_slice(&fs::read(backup.archive_path().unwrap()).unwrap()).unwrap();
        assert_eq!(current.diff_hash, "new");
        assert_eq!(archived.diff_hash, "old");
    }

    #[test]
    fn refuses_archive_collision_and_unsafe_run_keys() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        archive_flow_output(root, "same", &FlowProducer::General).unwrap();
        assert!(archive_flow_output(root, "same", &FlowProducer::General).is_err());
        assert!(archive_flow_output(root, "../escape", &FlowProducer::General).is_err());
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
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&outside).unwrap();
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

            fs::remove_file(root.join("experts")).unwrap();
            fs::write(root.join("triage.json"), b"original").unwrap();
            symlink(&outside, root.join("flow-backups")).unwrap();
            assert!(archive_flow_output(&root, "backup-link", &FlowProducer::Triage).is_err());
            assert_eq!(fs::read(root.join("triage.json")).unwrap(), b"original");
            assert!(outside.join("security.json").exists());
        }

        #[test]
        fn validation_rejects_symlinked_output_without_reading_outside_file() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("bucket");
            let outside = temp.path().join("outside.json");
            fs::create_dir_all(&root).unwrap();
            output(&outside, &general("expected"));
            symlink(&outside, root.join("review.json")).unwrap();

            assert!(validate_flow_output(&root, &FlowProducer::General, "expected").is_err());
            assert_eq!(
                serde_json::from_slice::<ErReview>(&fs::read(outside).unwrap())
                    .unwrap()
                    .diff_hash,
                "expected"
            );
        }

        #[test]
        fn restore_rejects_symlinked_destination_and_archive() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("bucket");
            let outside = temp.path().join("outside");
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&outside).unwrap();
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

            fs::remove_dir(root.join("experts")).unwrap();
            output(
                &outside.join("security.json"),
                &expert("security", "outside"),
            );
            symlink(&outside, root.join("experts")).unwrap();
            assert!(backup.restore().is_err());
            assert_eq!(
                serde_json::from_slice::<ExpertReview>(
                    &fs::read(outside.join("security.json")).unwrap()
                )
                .unwrap()
                .diff_hash,
                "outside"
            );

            fs::remove_file(root.join("experts")).unwrap();
            fs::create_dir(root.join("experts")).unwrap();
            let archived = backup.archive_path().unwrap();
            fs::remove_file(archived).unwrap();
            symlink(outside.join("security.json"), archived).unwrap();
            assert!(backup.restore().is_err());
            assert_eq!(
                serde_json::from_slice::<ExpertReview>(
                    &fs::read(outside.join("security.json")).unwrap()
                )
                .unwrap()
                .diff_hash,
                "outside"
            );
        }

        #[test]
        fn replacement_preservation_rejects_symlinked_backup_component() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("bucket");
            let outside = temp.path().join("outside");
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&outside).unwrap();
            output(&root.join("review.json"), &general("old"));
            output(&outside.join("review.json"), &general("outside"));
            let backup =
                archive_flow_output(&root, "replacement-link", &FlowProducer::General).unwrap();
            fs::write(root.join("review.json"), b"malformed").unwrap();
            symlink(
                &outside,
                root.join("flow-backups/replacement-link/replacements"),
            )
            .unwrap();

            assert!(backup.restore().is_err());
            assert_eq!(fs::read(root.join("review.json")).unwrap(), b"malformed");
            assert_eq!(
                serde_json::from_slice::<ErReview>(&fs::read(outside.join("review.json")).unwrap())
                    .unwrap()
                    .diff_hash,
                "outside"
            );
        }
    }
}
