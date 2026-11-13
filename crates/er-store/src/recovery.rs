//! Snapshot, archive and restore for recovery paths.
//!
//! A typed read or write is the normal way to touch an artifact. Recovery is
//! the exception: an acceptance can replace a sidecar and then fail, and the
//! bytes that were there must come back exactly, including bytes the store
//! would call malformed. These operations write only bytes the store itself
//! read (an [`ArtifactSnapshot`]) or moved into a backup ([`BackupSnapshot`]),
//! so they cannot put arbitrary caller bytes at an artifact path. A public
//! function that took a byte buffer plus a path would be that hole; the
//! `er::store_bytes` gate rejects one.

use std::path::{Component, Path, PathBuf};

use crate::artifact::Artifact;
use crate::envelope::{self, Loaded, Stored, Unreadable};
use crate::error::StoreError;
use crate::key::{ArtifactKey, RelPath, Scope};
use crate::lock::{ScopeGuard, DEFAULT_LOCK_TIMEOUT};
use crate::store::{
    file_state, now_epoch_secs, serialize, ArtifactStore, FileState, MAX_ARTIFACT_BYTES,
};
use crate::time::format_rfc3339_utc;

/// The bytes one artifact held, tied to the key they were read from.
///
/// The fields are private and there is no public constructor: a caller can
/// only get one from [`ArtifactStore::snapshot`], so a restore puts back bytes
/// that were already at that path.
pub struct ArtifactSnapshot<T: Artifact> {
    key: ArtifactKey<T>,
    bytes: Vec<u8>,
}

impl<T: Artifact> std::fmt::Debug for ArtifactSnapshot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtifactSnapshot")
            .field("key", &self.key)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl<T: Artifact> ArtifactSnapshot<T> {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Parse the captured bytes with the same envelope and version rules a
    /// typed read applies.
    pub fn parse(&self) -> Loaded<T> {
        match serde_json::from_slice::<serde_json::Value>(&self.bytes) {
            Ok(value) => envelope::parse::<T>(value),
            Err(error) => Loaded::Unreadable(Unreadable::Malformed(error.to_string())),
        }
    }
}

/// A file the store moved into a backup directory, with where it came from.
///
/// Like [`ArtifactSnapshot`], this has no public constructor. Restoring it
/// copies the bytes the store archived back to the artifact path they were
/// moved from, and never clobbers a file that appeared since.
#[derive(Clone, Debug)]
pub struct BackupSnapshot {
    scope: Scope,
    root: PathBuf,
    origin: PathBuf,
    backup: PathBuf,
}

impl BackupSnapshot {
    /// Where the archived bytes live. The archive keeps them after a restore.
    pub fn backup_path(&self) -> &Path {
        &self.backup
    }

    /// The artifact path the bytes were moved from.
    pub fn origin_path(&self) -> &Path {
        &self.origin
    }
}

impl ArtifactStore {
    /// Read one artifact's raw bytes without parsing them.
    ///
    /// The size cap, the absent case and the "not a regular file" case match
    /// [`ArtifactStore::read`]. A symlink anywhere below the scope is an error,
    /// so a validation read cannot reach a file outside the bucket.
    pub fn snapshot<T: Artifact>(
        &self,
        key: &ArtifactKey<T>,
    ) -> Result<Loaded<ArtifactSnapshot<T>>, StoreError> {
        let root = match std::fs::canonicalize(self.scope_dir(key.scope())) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Loaded::Absent);
            }
            Err(error) => return Err(error.into()),
        };
        let path = checked_below(&root, key.relative_path().as_path())?;
        match file_state(&path)? {
            FileState::Missing => Ok(Loaded::Absent),
            FileState::NotFile => Ok(Loaded::Unreadable(Unreadable::Malformed(
                "not a regular file".to_string(),
            ))),
            FileState::TooLarge(bytes) => Ok(Loaded::Unreadable(Unreadable::TooLarge { bytes })),
            FileState::Readable => Ok(Loaded::Found(Stored {
                value: ArtifactSnapshot {
                    key: key.clone(),
                    bytes: std::fs::read(&path)?,
                },
                meta: None,
            })),
        }
    }

    /// Refuse a symlink on the way to the artifact without reading it.
    ///
    /// An archive call runs the same check under the lock; this lets a caller
    /// reject an unsafe path before it creates a backup directory.
    pub fn check_artifact_path<T: Artifact>(&self, key: &ArtifactKey<T>) -> Result<(), StoreError> {
        let root = match std::fs::canonicalize(self.scope_dir(key.scope())) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        checked_below(&root, key.relative_path().as_path())?;
        Ok(())
    }
}

impl ArtifactStore {
    /// Write `value` only when the artifact still holds the bytes `expected`
    /// captured. Returns the written snapshot, or `None` when it changed.
    ///
    /// The comparison and the write happen under one scope lock, so another
    /// store writer cannot slip a write between them.
    pub fn write_typed_if_unchanged<T: Artifact>(
        &self,
        expected: &ArtifactSnapshot<T>,
        value: &T,
    ) -> Result<Option<ArtifactSnapshot<T>>, StoreError> {
        let guard = self.lock_scope(expected.key.scope(), DEFAULT_LOCK_TIMEOUT)?;
        let rel = expected.key.relative_path().as_path();
        let path = checked_below(guard.scope_dir(), rel)?;
        if current_bytes(&path)?.as_deref() != Some(expected.bytes.as_slice()) {
            return Ok(None);
        }
        let text = serialize::<T>(value, &format_rfc3339_utc(now_epoch_secs()))?;
        let length = u64::try_from(text.len()).unwrap_or(u64::MAX);
        if length > MAX_ARTIFACT_BYTES {
            return Err(StoreError::TooLarge { bytes: length });
        }
        crate::atomic::write_atomic(&path, text.as_bytes())?;
        Ok(Some(ArtifactSnapshot {
            key: expected.key.clone(),
            bytes: text.into_bytes(),
        }))
    }

    /// Put `snapshot` back only when the artifact still holds the bytes
    /// `expected` captured. Returns whether it wrote.
    pub fn restore_if_unchanged<T: Artifact>(
        &self,
        snapshot: &ArtifactSnapshot<T>,
        expected: &ArtifactSnapshot<T>,
    ) -> Result<bool, StoreError> {
        let guard = self.lock_scope(snapshot.key.scope(), DEFAULT_LOCK_TIMEOUT)?;
        let rel = snapshot.key.relative_path().as_path();
        let path = checked_below(guard.scope_dir(), rel)?;
        if current_bytes(&path)?.as_deref() != Some(expected.bytes.as_slice()) {
            return Ok(false);
        }
        crate::atomic::write_atomic(&path, &snapshot.bytes)?;
        Ok(true)
    }

    /// Move the artifact into `backup_dir`, keeping its relative path below the
    /// directory. Returns `None` when the artifact is absent, and refuses to
    /// replace a backup that already exists.
    pub fn archive_to_dir<T: Artifact>(
        &self,
        key: &ArtifactKey<T>,
        backup_dir: &RelPath,
    ) -> Result<Option<BackupSnapshot>, StoreError> {
        let guard = self.lock_scope(key.scope(), DEFAULT_LOCK_TIMEOUT)?;
        Self::archive_locked(&guard, key, backup_dir)
    }

    fn archive_locked<T: Artifact>(
        guard: &ScopeGuard,
        key: &ArtifactKey<T>,
        backup_dir: &RelPath,
    ) -> Result<Option<BackupSnapshot>, StoreError> {
        let root = guard.scope_dir().to_path_buf();
        let origin = checked_below(&root, key.relative_path().as_path())?;
        let backup_rel = backup_dir.as_path().join(key.relative_path().as_path());
        let backup = checked_below(&root, &backup_rel)?;
        match std::fs::symlink_metadata(&origin) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(StoreError::SymlinkedPath { path: origin });
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(StoreError::NotARegularFile { path: origin });
            }
            Ok(_) => {}
        }
        if let Some(parent) = backup.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let backup = checked_below(&root, &backup_rel)?;
        if std::fs::symlink_metadata(&backup).is_ok() {
            return Err(StoreError::BackupExists { path: backup });
        }
        std::fs::rename(&origin, &backup)?;
        Ok(Some(BackupSnapshot {
            scope: key.scope().clone(),
            root,
            origin,
            backup,
        }))
    }

    /// Copy a snapshot's bytes into `backup_dir`, keeping its relative path.
    /// Refuses to replace a file that already exists.
    pub fn write_backup_to_dir<T: Artifact>(
        &self,
        snapshot: &ArtifactSnapshot<T>,
        backup_dir: &RelPath,
    ) -> Result<(), StoreError> {
        let guard = self.lock_scope(snapshot.key.scope(), DEFAULT_LOCK_TIMEOUT)?;
        let root = guard.scope_dir();
        let backup_rel = backup_dir
            .as_path()
            .join(snapshot.key.relative_path().as_path());
        let backup = checked_below(root, &backup_rel)?;
        if let Some(parent) = backup.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let backup = checked_below(root, &backup_rel)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)?;
        use std::io::Write;
        if let Err(error) = file
            .write_all(&snapshot.bytes)
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = std::fs::remove_file(&backup);
            return Err(error.into());
        }
        Ok(())
    }

    /// Copy an archived file back to the artifact path it was moved from. An
    /// existing destination is left alone; the backup is retained either way.
    pub fn restore_backup(&self, backup: &BackupSnapshot) -> Result<(), StoreError> {
        let guard = self.lock_scope(&backup.scope, DEFAULT_LOCK_TIMEOUT)?;
        let root = guard.scope_dir();
        let origin_rel = relative_to(&backup.root, &backup.origin)?;
        let backup_rel = relative_to(&backup.root, &backup.backup)?;
        let origin = checked_below(root, origin_rel)?;
        let backup_path = checked_below(root, backup_rel)?;
        let metadata = std::fs::symlink_metadata(&backup_path)?;
        if metadata.file_type().is_symlink() {
            return Err(StoreError::SymlinkedPath { path: backup_path });
        }
        if !metadata.is_file() {
            return Err(StoreError::NotARegularFile { path: backup_path });
        }
        let bytes = std::fs::read(&backup_path)?;
        if let Some(parent) = origin.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let origin = checked_below(root, origin_rel)?;
        let mut target = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&origin)
        {
            Ok(target) => target,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        use std::io::Write;
        if let Err(error) = target.write_all(&bytes).and_then(|()| target.sync_all()) {
            drop(target);
            let _ = std::fs::remove_file(&origin);
            return Err(error.into());
        }
        drop(target);
        let permissions = std::fs::metadata(&backup_path)?.permissions();
        std::fs::set_permissions(&origin, permissions)?;
        Ok(())
    }

    /// Put an archived file back only when the artifact still holds the bytes
    /// `expected` names. Returns whether it wrote.
    pub fn restore_backup_if_unchanged(
        &self,
        backup: &BackupSnapshot,
        expected: &[u8],
    ) -> Result<bool, StoreError> {
        let guard = self.lock_scope(&backup.scope, DEFAULT_LOCK_TIMEOUT)?;
        let root = guard.scope_dir();
        let origin_rel = relative_to(&backup.root, &backup.origin)?;
        let backup_rel = relative_to(&backup.root, &backup.backup)?;
        let origin = checked_below(root, origin_rel)?;
        if current_bytes(&origin)?.as_deref() != Some(expected) {
            return Ok(false);
        }
        let backup_path = checked_below(root, backup_rel)?;
        let metadata = std::fs::symlink_metadata(&backup_path)?;
        if metadata.file_type().is_symlink() {
            return Err(StoreError::SymlinkedPath { path: backup_path });
        }
        if !metadata.is_file() {
            return Err(StoreError::NotARegularFile { path: backup_path });
        }
        let bytes = std::fs::read(&backup_path)?;
        crate::atomic::write_atomic(&origin, &bytes)?;
        Ok(true)
    }

    /// Read an archived file's bytes. The backup path is checked for symlinks
    /// and for the store's size cap, like an artifact read.
    pub fn read_backup(&self, backup: &BackupSnapshot) -> Result<Loaded<Vec<u8>>, StoreError> {
        let backup_rel = relative_to(&backup.root, &backup.backup)?;
        let path = checked_below(&backup.root, backup_rel)?;
        match file_state(&path)? {
            FileState::Missing => Ok(Loaded::Absent),
            FileState::NotFile => Ok(Loaded::Unreadable(Unreadable::Malformed(
                "not a regular file".to_string(),
            ))),
            FileState::TooLarge(bytes) => Ok(Loaded::Unreadable(Unreadable::TooLarge { bytes })),
            FileState::Readable => Ok(Loaded::Found(Stored {
                value: std::fs::read(&path)?,
                meta: None,
            })),
        }
    }

    /// Create one backup directory, and its parents, refusing to reuse one.
    pub fn create_backup_dir(&self, scope: &Scope, dir: &RelPath) -> Result<(), StoreError> {
        let guard = self.lock_scope(scope, DEFAULT_LOCK_TIMEOUT)?;
        let root = guard.scope_dir();
        let path = checked_below(root, dir.as_path())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let path = checked_below(root, dir.as_path())?;
        std::fs::create_dir(&path)?;
        Ok(())
    }

    /// Remove a backup directory and its contents. A missing directory is not
    /// an error, so a cleanup after a failed archive is safe.
    pub fn remove_backup_dir(&self, scope: &Scope, dir: &RelPath) -> Result<(), StoreError> {
        let guard = self.lock_scope(scope, DEFAULT_LOCK_TIMEOUT)?;
        let path = checked_below(guard.scope_dir(), dir.as_path())?;
        match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Join `relative` below a canonical root, rejecting a symlink on the way.
/// A missing component ends the walk: nothing below it can exist yet.
fn checked_below(root: &Path, relative: &Path) -> Result<PathBuf, StoreError> {
    if relative.is_absolute() {
        return Err(StoreError::InvalidPath {
            value: relative.display().to_string(),
        });
    }
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(StoreError::InvalidPath {
                value: relative.display().to_string(),
            });
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(StoreError::SymlinkedPath { path: current });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(root.join(relative))
}

fn relative_to<'a>(root: &Path, path: &'a Path) -> Result<&'a Path, StoreError> {
    path.strip_prefix(root)
        .map_err(|_| StoreError::InvalidPath {
            value: path.display().to_string(),
        })
}

fn current_bytes(path: &Path) -> Result<Option<Vec<u8>>, StoreError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::test_support::{TestEnveloped, TestPayload};

    trait ParsedValue<T> {
        fn parsed_value(self) -> Option<T>;
    }

    impl<T: Artifact> ParsedValue<T> for Loaded<ArtifactSnapshot<T>> {
        fn parsed_value(self) -> Option<T> {
            match self {
                Loaded::Found(stored) => match stored.value.parse() {
                    Loaded::Found(parsed) => Some(parsed.value),
                    _ => None,
                },
                _ => None,
            }
        }
    }

    fn temp_store() -> (TempDir, ArtifactStore, Scope) {
        let directory = TempDir::new().unwrap();
        let store = ArtifactStore::new(directory.path());
        let scope = Scope::dir(directory.path()).unwrap();
        (directory, store, scope)
    }

    fn key<T: Artifact>(scope: &Scope, slot: T::Slot) -> ArtifactKey<T> {
        ArtifactKey::new(scope.clone(), slot).unwrap()
    }

    fn bare(value: u32) -> TestPayload {
        TestPayload {
            version: TestPayload::VERSION,
            diff_hash: "hash".to_string(),
            value,
        }
    }

    #[test]
    fn a_snapshot_holds_the_file_bytes_and_parses_them_like_a_read() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        assert!(matches!(store.snapshot(&key).unwrap(), Loaded::Absent));
        store.write(&key, &bare(7)).unwrap();

        match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => {
                let snapshot = stored.value;
                assert_eq!(
                    snapshot.bytes(),
                    std::fs::read(store.path(&key)).unwrap().as_slice()
                );
                match snapshot.parse() {
                    Loaded::Found(parsed) => {
                        assert_eq!(parsed.value, bare(7));
                        assert!(parsed.meta.is_some(), "an enveloped file carries metadata");
                    }
                    other => panic!("expected a parsed payload, got {other:?}"),
                }
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn a_snapshot_of_an_oversized_file_is_too_large() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        let file = std::fs::File::create(store.path(&key)).unwrap();
        file.set_len(MAX_ARTIFACT_BYTES + 1).unwrap();
        assert!(matches!(
            store.snapshot(&key).unwrap(),
            Loaded::Unreadable(Unreadable::TooLarge {
                bytes
            }) if bytes == MAX_ARTIFACT_BYTES + 1
        ));
    }

    #[test]
    fn a_typed_write_only_lands_when_the_bytes_match() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        store.write(&key, &bare(1)).unwrap();
        let expected = match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected Found, got {other:?}"),
        };

        std::fs::write(store.path(&key), "changed elsewhere").unwrap();
        assert!(store
            .write_typed_if_unchanged(&expected, &bare(2))
            .unwrap()
            .is_none());
        assert_eq!(
            std::fs::read_to_string(store.path(&key)).unwrap(),
            "changed elsewhere"
        );

        store.write(&key, &bare(1)).unwrap();
        // Each write stamps its own `written_at`, so the expectation to compare
        // against is this write's bytes, not the earlier snapshot's.
        let current = match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected Found, got {other:?}"),
        };
        let written = store
            .write_typed_if_unchanged(&current, &bare(3))
            .unwrap()
            .expect("the file still held the expected bytes");
        match written.parse() {
            Loaded::Found(parsed) => assert_eq!(parsed.value, bare(3)),
            other => panic!("expected a parsed payload, got {other:?}"),
        }
        assert!(
            store
                .write_typed_if_unchanged(&current, &bare(4))
                .unwrap()
                .is_none(),
            "the written bytes no longer match the old snapshot"
        );
    }

    #[test]
    fn a_restore_only_lands_when_the_expected_bytes_match() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        store.write(&key, &bare(1)).unwrap();
        let original = match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected Found, got {other:?}"),
        };
        store.write(&key, &bare(2)).unwrap();
        let changed = match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected Found, got {other:?}"),
        };
        store.write(&key, &bare(3)).unwrap();
        let third = match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected Found, got {other:?}"),
        };

        assert!(!store.restore_if_unchanged(&original, &changed).unwrap());
        assert_eq!(
            std::fs::read(store.path(&key)).unwrap().as_slice(),
            third.bytes()
        );
        store.write(&key, &bare(2)).unwrap();
        // The rewrite stamps a new `written_at`, so capture what it wrote.
        let changed_now = match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected Found, got {other:?}"),
        };
        assert!(store.restore_if_unchanged(&original, &changed_now).unwrap());
        assert_eq!(
            std::fs::read(store.path(&key)).unwrap().as_slice(),
            original.bytes()
        );
    }

    #[test]
    fn only_one_of_two_competing_restores_wins() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        store.write(&key, &bare(1)).unwrap();
        let original = match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected Found, got {other:?}"),
        };
        let store = std::sync::Arc::new(store);
        for _ in 0..32 {
            store.write(&key, &bare(2)).unwrap();
            // Each write stamps its own `written_at`, so the bytes to compare
            // against are the ones this round wrote, not an earlier snapshot's.
            let changed = match store.snapshot(&key).unwrap() {
                Loaded::Found(stored) => stored.value,
                other => panic!("expected Found, got {other:?}"),
            };
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let outcomes = std::thread::scope(|threads| {
                let handles: Vec<_> = (0..2)
                    .map(|_| {
                        let store = std::sync::Arc::clone(&store);
                        let original = &original;
                        let changed = &changed;
                        let barrier = std::sync::Arc::clone(&barrier);
                        threads.spawn(move || {
                            barrier.wait();
                            store.restore_if_unchanged(original, changed).unwrap()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect::<Vec<bool>>()
            });
            assert_eq!(
                outcomes.iter().filter(|wrote| **wrote).count(),
                1,
                "the compare-and-swap is serialized: {outcomes:?}"
            );
            assert_eq!(
                std::fs::read(store.path(&key)).unwrap().as_slice(),
                original.bytes()
            );
        }
    }

    #[test]
    fn a_snapshot_refuses_a_symlinked_artifact() {
        let (directory, store, scope) = temp_store();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("outside.json");
        std::fs::write(&target, "outside").unwrap();
        let key = key::<TestPayload>(&scope, ());
        std::os::unix::fs::symlink(&target, directory.path().join("test-payload.json")).unwrap();
        assert!(matches!(
            store.snapshot(&key),
            Err(StoreError::SymlinkedPath { .. })
        ));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "outside");
    }

    #[test]
    fn archiving_moves_the_file_and_refuses_a_second_archive() {
        let (directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        store.write(&key, &bare(1)).unwrap();
        let original = std::fs::read(store.path(&key)).unwrap();
        let backup_dir = RelPath::new("flow-backups/run-1").unwrap();
        store.create_backup_dir(&scope, &backup_dir).unwrap();

        let archived = store
            .archive_to_dir(&key, &backup_dir)
            .unwrap()
            .expect("the artifact existed");
        assert!(!store.path(&key).exists());
        assert_eq!(
            archived.backup_path(),
            std::fs::canonicalize(directory.path())
                .unwrap()
                .join("flow-backups/run-1/test-payload.json")
        );
        assert_eq!(std::fs::read(archived.backup_path()).unwrap(), original);
        assert!(store.archive_to_dir(&key, &backup_dir).unwrap().is_none());

        store.write(&key, &bare(2)).unwrap();
        assert!(matches!(
            store.archive_to_dir(&key, &backup_dir),
            Err(StoreError::BackupExists { .. })
        ));
    }

    #[test]
    fn a_backup_dir_is_created_once_and_removed() {
        let (_directory, store, scope) = temp_store();
        let dir = RelPath::new("flow-backups/run-1").unwrap();
        store.create_backup_dir(&scope, &dir).unwrap();
        assert!(matches!(
            store.create_backup_dir(&scope, &dir),
            Err(StoreError::Io(_))
        ));
        store.remove_backup_dir(&scope, &dir).unwrap();
        store.remove_backup_dir(&scope, &dir).unwrap();
    }

    #[test]
    fn a_backup_restore_never_clobbers_a_destination() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        store.write(&key, &bare(1)).unwrap();
        let dir = RelPath::new("flow-backups/run-1").unwrap();
        store.create_backup_dir(&scope, &dir).unwrap();
        let archived = store
            .archive_to_dir(&key, &dir)
            .unwrap()
            .expect("the artifact existed");

        store.write(&key, &bare(2)).unwrap();
        store.restore_backup(&archived).unwrap();
        assert_eq!(
            store.snapshot(&key).unwrap().parsed_value(),
            Some(bare(2)),
            "an existing destination is left alone"
        );
        assert!(archived.backup_path().is_file(), "the backup is retained");

        store.delete::<TestPayload>(&key).unwrap();
        store.restore_backup(&archived).unwrap();
        assert_eq!(store.snapshot(&key).unwrap().parsed_value(), Some(bare(1)));
    }

    #[test]
    fn a_backup_restore_refuses_a_symlinked_archive() {
        let (directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        store.write(&key, &bare(1)).unwrap();
        let dir = RelPath::new("flow-backups/run-1").unwrap();
        store.create_backup_dir(&scope, &dir).unwrap();
        let archived = store
            .archive_to_dir(&key, &dir)
            .unwrap()
            .expect("the artifact existed");
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("outside.json");
        std::fs::write(&target, "outside").unwrap();
        std::fs::remove_file(archived.backup_path()).unwrap();
        std::os::unix::fs::symlink(&target, archived.backup_path()).unwrap();

        assert!(matches!(
            store.restore_backup(&archived),
            Err(StoreError::SymlinkedPath { .. })
        ));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "outside");
        assert!(!directory.path().join("test-payload.json").exists());
    }

    #[test]
    fn a_backup_write_refuses_an_existing_file_and_reads_back() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        store.write(&key, &bare(1)).unwrap();
        let snapshot = match store.snapshot(&key).unwrap() {
            Loaded::Found(stored) => stored.value,
            other => panic!("expected Found, got {other:?}"),
        };
        let dir = RelPath::new("flow-backups/run-1").unwrap();
        store.write_backup_to_dir(&snapshot, &dir).unwrap();
        assert!(matches!(
            store.write_backup_to_dir(&snapshot, &dir),
            Err(StoreError::Io(_))
        ));

        let archived_dir = RelPath::new("flow-backups/run-2").unwrap();
        store.create_backup_dir(&scope, &archived_dir).unwrap();
        let archived = store
            .archive_to_dir(&key, &archived_dir)
            .unwrap()
            .expect("the artifact existed");
        assert_eq!(
            store.read_backup(&archived).unwrap(),
            Loaded::Found(Stored {
                value: snapshot.bytes().to_vec(),
                meta: None,
            })
        );
    }

    #[test]
    fn archive_refuses_a_symlinked_backup_component() {
        let (directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        store.write(&key, &bare(1)).unwrap();
        let outside = TempDir::new().unwrap();
        std::fs::create_dir_all(directory.path().join("flow-backups")).unwrap();
        std::os::unix::fs::symlink(outside.path(), directory.path().join("flow-backups/run-1"))
            .unwrap();
        let dir = RelPath::new("flow-backups/run-1").unwrap();

        assert!(matches!(
            store.archive_to_dir(&key, &dir),
            Err(StoreError::SymlinkedPath { .. })
        ));
        assert!(store.path(&key).is_file());
        assert!(!outside.path().join("test-payload.json").exists());
    }

    #[test]
    fn archive_refuses_a_directory_at_the_artifact_path() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestPayload>(&scope, ());
        std::fs::create_dir_all(store.path(&key)).unwrap();
        let dir = RelPath::new("flow-backups/run-1").unwrap();
        assert!(matches!(
            store.archive_to_dir(&key, &dir),
            Err(StoreError::NotARegularFile { .. })
        ));
    }

    #[test]
    fn a_snapshot_of_a_newer_enveloped_kind_is_still_bytes() {
        let (_directory, store, scope) = temp_store();
        let key = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        let raw = json!({"kind": "test-enveloped", "version": 99, "payload": {"value": 1}});
        std::fs::write(store.path(&key), raw.to_string()).unwrap();
        assert!(matches!(store.snapshot(&key).unwrap(), Loaded::Found(_)));
        assert_eq!(
            store.snapshot(&key).unwrap().parsed_value(),
            None,
            "parse still applies the version rule"
        );
    }
}
