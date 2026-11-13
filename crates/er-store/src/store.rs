use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::artifact::Artifact;
use crate::envelope::{self, Envelope, Loaded, Stored, Unreadable};
use crate::error::{StoreError, UpdateError};
use crate::key::{ArtifactKey, RelPath, Scope, SlotName};
use crate::lock::{ScopeGuard, DEFAULT_LOCK_TIMEOUT};
use crate::time::format_rfc3339_utc;

/// The largest artifact or text file a read accepts and a write produces.
pub const MAX_ARTIFACT_BYTES: u64 = 10_000_000;

pub(crate) fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub(crate) enum FileState {
    Missing,
    NotFile,
    TooLarge(u64),
    Readable,
}

pub(crate) fn file_state(path: &Path) -> Result<FileState, StoreError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileState::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Ok(FileState::NotFile);
    }
    if metadata.len() > MAX_ARTIFACT_BYTES {
        return Ok(FileState::TooLarge(metadata.len()));
    }
    Ok(FileState::Readable)
}

fn remove_file(path: &Path) -> Result<(), StoreError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn serialize<T: Artifact>(value: &T, written_at: &str) -> Result<String, StoreError> {
    Ok(serde_json::to_string_pretty(&Envelope {
        kind: T::KIND,
        version: T::VERSION,
        diff_hash: value.diff_hash(),
        producer: Some("host"),
        written_at: written_at.to_string(),
        payload: value,
    })?)
}

/// A typed view over one store root.
#[derive(Clone, Debug)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn scope_dir(&self, scope: &Scope) -> PathBuf {
        match scope {
            Scope::Branch { repo, branch, view } => self
                .root
                .join("repos")
                .join(repo.as_str())
                .join("branches")
                .join(branch.as_str())
                .join("view-buckets")
                .join(view.as_str()),
            Scope::Pr { repo, number } => self
                .root
                .join("repos")
                .join(repo.as_str())
                .join("prs")
                .join(format!("pr-{number}")),
            Scope::App => self.root.join("app"),
            Scope::Dir(dir) => dir.as_path().to_path_buf(),
        }
    }

    pub fn path<T: Artifact>(&self, key: &ArtifactKey<T>) -> PathBuf {
        self.scope_dir(key.scope())
            .join(key.relative_path().as_path())
    }

    pub fn exists<T: Artifact>(&self, key: &ArtifactKey<T>) -> bool {
        self.path(key).is_file()
    }

    pub fn read<T: Artifact>(&self, key: &ArtifactKey<T>) -> Result<Loaded<T>, StoreError> {
        let path = self.path(key);
        match file_state(&path)? {
            FileState::Missing => Ok(Loaded::Absent),
            FileState::NotFile => Ok(Loaded::Unreadable(Unreadable::Malformed(
                "not a regular file".to_string(),
            ))),
            FileState::TooLarge(bytes) => Ok(Loaded::Unreadable(Unreadable::TooLarge { bytes })),
            FileState::Readable => {
                let bytes = std::fs::read(&path)?;
                match serde_json::from_slice(&bytes) {
                    Ok(value) => Ok(envelope::parse::<T>(value)),
                    Err(error) => Ok(Loaded::Unreadable(Unreadable::Malformed(error.to_string()))),
                }
            }
        }
    }

    pub fn write<T: Artifact>(&self, key: &ArtifactKey<T>, value: &T) -> Result<(), StoreError> {
        self.write_with_timeout(key, value, DEFAULT_LOCK_TIMEOUT)
    }

    pub fn write_with_timeout<T: Artifact>(
        &self,
        key: &ArtifactKey<T>,
        value: &T,
        timeout: Duration,
    ) -> Result<(), StoreError> {
        let guard = self.lock_scope(key.scope(), timeout)?;
        self.write_locked(&guard, key, value)
    }

    pub fn write_locked<T: Artifact>(
        &self,
        guard: &ScopeGuard,
        key: &ArtifactKey<T>,
        value: &T,
    ) -> Result<(), StoreError> {
        self.check_guard(guard, key.scope())?;
        self.write_body(key, value, false)
    }

    pub fn write_durable<T: Artifact>(
        &self,
        key: &ArtifactKey<T>,
        value: &T,
    ) -> Result<(), StoreError> {
        self.write_durable_with_timeout(key, value, DEFAULT_LOCK_TIMEOUT)
    }

    pub fn write_durable_with_timeout<T: Artifact>(
        &self,
        key: &ArtifactKey<T>,
        value: &T,
        timeout: Duration,
    ) -> Result<(), StoreError> {
        let guard = self.lock_scope(key.scope(), timeout)?;
        self.write_durable_locked(&guard, key, value)
    }

    pub fn write_durable_locked<T: Artifact>(
        &self,
        guard: &ScopeGuard,
        key: &ArtifactKey<T>,
        value: &T,
    ) -> Result<(), StoreError> {
        self.check_guard(guard, key.scope())?;
        self.write_body(key, value, true)
    }

    fn check_guard(&self, guard: &ScopeGuard, scope: &Scope) -> Result<(), StoreError> {
        let raw = self.scope_dir(scope);
        let canonical = std::fs::canonicalize(&raw).map_err(|_| StoreError::WrongScope {
            guard: guard.scope_dir().to_path_buf(),
            key: raw.clone(),
        })?;
        if canonical != guard.scope_dir() {
            return Err(StoreError::WrongScope {
                guard: guard.scope_dir().to_path_buf(),
                key: canonical,
            });
        }
        Ok(())
    }

    fn write_body<T: Artifact>(
        &self,
        key: &ArtifactKey<T>,
        value: &T,
        durable: bool,
    ) -> Result<(), StoreError> {
        if let Loaded::Unreadable(Unreadable::NewerVersion { found }) = self.read(key)? {
            return Err(StoreError::NewerVersion {
                path: self.path(key),
                found,
                expected: T::VERSION,
            });
        }
        let written_at = format_rfc3339_utc(now_epoch_secs());
        let text = serialize::<T>(value, &written_at)?;
        let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
        if bytes > MAX_ARTIFACT_BYTES {
            return Err(StoreError::TooLarge { bytes });
        }
        Self::write_bytes(&self.path(key), text.as_bytes(), durable)
    }

    /// Read, apply and write under one hold, so a second writer cannot lose
    /// an update between the read and the write.
    pub fn update<T, E, F>(
        &self,
        key: &ArtifactKey<T>,
        timeout: Duration,
        f: F,
    ) -> Result<(T, Option<Unreadable>), UpdateError<E>>
    where
        T: Artifact,
        F: FnOnce(Option<T>) -> Result<T, E>,
    {
        self.update_with(key, timeout, false, f)
    }

    /// [`Self::update`] with the durable write path.
    pub fn update_durable<T, E, F>(
        &self,
        key: &ArtifactKey<T>,
        timeout: Duration,
        f: F,
    ) -> Result<(T, Option<Unreadable>), UpdateError<E>>
    where
        T: Artifact,
        F: FnOnce(Option<T>) -> Result<T, E>,
    {
        self.update_with(key, timeout, true, f)
    }

    fn update_with<T, E, F>(
        &self,
        key: &ArtifactKey<T>,
        timeout: Duration,
        durable: bool,
        f: F,
    ) -> Result<(T, Option<Unreadable>), UpdateError<E>>
    where
        T: Artifact,
        F: FnOnce(Option<T>) -> Result<T, E>,
    {
        let guard = self.lock_scope(key.scope(), timeout)?;
        let (current, unreadable) = match self.read(key)? {
            Loaded::Found(stored) => (Some(stored.value), None),
            Loaded::Absent => (None, None),
            Loaded::Unreadable(Unreadable::NewerVersion { found }) => {
                return Err(UpdateError::Store(StoreError::NewerVersion {
                    path: self.path(key),
                    found,
                    expected: T::VERSION,
                }));
            }
            Loaded::Unreadable(reason) => (None, Some(reason)),
        };
        let value = f(current).map_err(UpdateError::Apply)?;
        // The guard must stay held from the read through the write. Dropping
        // it after `f` but before the write lets a second writer interleave a
        // read-modify-write between them, and no test reliably catches that
        // window. Keep it alive until both write paths return.
        if durable {
            self.write_durable_locked(&guard, key, &value)?;
        } else {
            self.write_locked(&guard, key, &value)?;
        }
        Ok((value, unreadable))
    }

    fn write_bytes(path: &Path, bytes: &[u8], durable: bool) -> Result<(), StoreError> {
        path.parent().ok_or_else(|| StoreError::InvalidPath {
            value: path.display().to_string(),
        })?;
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| StoreError::InvalidPath {
                value: path.display().to_string(),
            })?;
        if durable {
            crate::atomic::write_atomic_durable(path, bytes)?;
        } else {
            crate::atomic::write_atomic(path, bytes)?;
        }
        Ok(())
    }

    pub fn delete<T: Artifact>(&self, key: &ArtifactKey<T>) -> Result<(), StoreError> {
        let guard = self.lock_scope(key.scope(), DEFAULT_LOCK_TIMEOUT)?;
        self.delete_locked(&guard, key)
    }

    pub fn delete_locked<T: Artifact>(
        &self,
        guard: &ScopeGuard,
        key: &ArtifactKey<T>,
    ) -> Result<(), StoreError> {
        self.check_guard(guard, key.scope())?;
        remove_file(&self.path(key))
    }

    pub fn list<T: Artifact>(&self, scope: &Scope) -> Result<Vec<T::Slot>, StoreError> {
        let Some(single) = T::Slot::singleton() else {
            return self.list_slots::<T>(scope);
        };
        let key = ArtifactKey::<T>::new(scope.clone(), single.clone())?;
        Ok(if self.path(&key).is_file() {
            vec![single]
        } else {
            Vec::new()
        })
    }

    fn list_slots<T: Artifact>(&self, scope: &Scope) -> Result<Vec<T::Slot>, StoreError> {
        let template = T::FILE;
        let Some((prefix, suffix)) = template.split_once("{slot}") else {
            return Err(StoreError::InvalidPath {
                value: template.to_string(),
            });
        };
        let (directory, file_prefix) = match prefix.rsplit_once('/') {
            Some((directory, file_prefix)) => (Some(directory), file_prefix),
            None => (None, prefix),
        };
        let directory = match directory {
            Some(directory) => self.scope_dir(scope).join(directory),
            None => self.scope_dir(scope),
        };
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut slots = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(stem) = name
                .strip_prefix(file_prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
            else {
                continue;
            };
            if let Some(slot) = T::Slot::parse(stem) {
                slots.push(slot);
            }
        }
        slots.sort();
        slots.dedup();
        Ok(slots)
    }

    pub fn read_text(&self, scope: &Scope, file: &RelPath) -> Result<Loaded<String>, StoreError> {
        let path = self.text_path(scope, file);
        match file_state(&path)? {
            FileState::Missing => Ok(Loaded::Absent),
            FileState::NotFile => Ok(Loaded::Unreadable(Unreadable::Malformed(
                "not a regular file".to_string(),
            ))),
            FileState::TooLarge(bytes) => Ok(Loaded::Unreadable(Unreadable::TooLarge { bytes })),
            FileState::Readable => {
                let bytes = std::fs::read(&path)?;
                match String::from_utf8(bytes) {
                    Ok(value) => Ok(Loaded::Found(Stored { value, meta: None })),
                    Err(_) => Ok(Loaded::Unreadable(Unreadable::Malformed(
                        "text is not UTF-8".to_string(),
                    ))),
                }
            }
        }
    }

    pub fn write_text(&self, scope: &Scope, file: &RelPath, text: &str) -> Result<(), StoreError> {
        let guard = self.lock_scope(scope, DEFAULT_LOCK_TIMEOUT)?;
        self.write_text_locked(&guard, file, text)
    }

    pub fn write_text_locked(
        &self,
        guard: &ScopeGuard,
        file: &RelPath,
        text: &str,
    ) -> Result<(), StoreError> {
        let bytes = text.as_bytes();
        let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if length > MAX_ARTIFACT_BYTES {
            return Err(StoreError::TooLarge { bytes: length });
        }
        Self::write_bytes(&guard.scope_dir().join(file.as_path()), bytes, false)
    }

    pub fn delete_text(&self, scope: &Scope, file: &RelPath) -> Result<(), StoreError> {
        let guard = self.lock_scope(scope, DEFAULT_LOCK_TIMEOUT)?;
        self.delete_text_locked(&guard, file)
    }

    pub fn delete_text_locked(&self, guard: &ScopeGuard, file: &RelPath) -> Result<(), StoreError> {
        remove_file(&guard.scope_dir().join(file.as_path()))
    }

    pub fn text_path(&self, scope: &Scope, file: &RelPath) -> PathBuf {
        self.scope_dir(scope).join(file.as_path())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::key::{ArtifactKey, BranchView, ExpertId, RelPath, Scope, Slug};
    use crate::test_support::{
        TestAbsoluteTemplate, TestBadMulti, TestBadSingleton, TestPayload, TestBig, TestBigEnveloped,
        TestDotDotTemplate, TestEnveloped, TestEnvelopedHash, TestExpert, TestExpertAlt,
        TestExpertSingleton, TestGeneral, TestMigrateErr,
    };

    fn temp_store() -> (TempDir, ArtifactStore) {
        let directory = TempDir::new().unwrap();
        let store = ArtifactStore::new(directory.path());
        (directory, store)
    }

    fn scope(directory: &TempDir) -> Scope {
        Scope::dir(directory.path()).unwrap()
    }

    fn key<T: Artifact>(scope: &Scope, slot: T::Slot) -> ArtifactKey<T> {
        ArtifactKey::new(scope.clone(), slot).unwrap()
    }

    fn bare_value() -> TestPayload {
        TestPayload {
            version: TestPayload::VERSION,
            diff_hash: "abc".to_string(),
            value: 9,
        }
    }

    #[test]
    fn enveloped_write_then_read_round_trips_and_has_every_envelope_field() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        let value = TestEnveloped { value: 7 };
        store.write(&key, &value).unwrap();

        let raw = std::fs::read_to_string(store.path(&key)).unwrap();
        let document: serde_json::Value = serde_json::from_str(&raw).unwrap();
        for field in [
            "kind",
            "version",
            "diff_hash",
            "producer",
            "written_at",
            "payload",
        ] {
            assert!(document.get(field).is_some(), "missing {field}");
        }
        assert_eq!(document["kind"], "test-enveloped");
        assert_eq!(document["version"], TestEnveloped::VERSION);
        assert_eq!(document["producer"], "host");
        assert_eq!(document["payload"]["value"], 7);

        match store.read(&key).unwrap() {
            Loaded::Found(stored) => {
                assert_eq!(stored.value, value);
                let meta = stored.meta.expect("envelope metadata");
                assert_eq!(meta.version, TestEnveloped::VERSION);
                assert_eq!(meta.producer.as_deref(), Some("host"));
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn envelope_bytes_keep_the_field_order_and_have_no_trailing_newline() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        store.write(&key, &TestEnveloped { value: 7 }).unwrap();

        let raw = std::fs::read_to_string(store.path(&key)).unwrap();
        assert!(!raw.ends_with('\n'), "envelope must not end with a newline");
        assert!(
            raw.starts_with(
                "{\n  \"kind\": \"test-enveloped\",\n  \"version\": 2,\n  \"diff_hash\": null,\n  \"producer\": \"host\",\n  \"written_at\": \""
            ),
            "{raw}"
        );
        assert!(
            raw.ends_with("\"payload\": {\n    \"value\": 7\n  }\n}"),
            "{raw}"
        );
    }

    #[test]
    fn the_envelope_carries_the_payload_diff_hash() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnvelopedHash>(&scope, ());
        let value = TestEnvelopedHash {
            value: 4,
            diff_hash: "abc123".to_string(),
        };
        store.write(&key, &value).unwrap();

        let raw = std::fs::read_to_string(store.path(&key)).unwrap();
        let document: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(document["diff_hash"], "abc123");

        match store.read(&key).unwrap() {
            Loaded::Found(stored) => {
                assert_eq!(stored.value, value);
                assert_eq!(
                    stored.meta.expect("envelope metadata").diff_hash.as_deref(),
                    Some("abc123")
                );
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn unknown_envelope_keys_are_ignored() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        std::fs::write(
            store.path(&key),
            r#"{"kind":"test-enveloped","version":2,"diff_hash":null,"producer":"host","written_at":"2026-11-06T09:30:00Z","future":true,"payload":{"value":5}}"#,
        )
        .unwrap();

        match store.read(&key).unwrap() {
            Loaded::Found(stored) => assert_eq!(stored.value, TestEnveloped { value: 5 }),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn write_durable_round_trips() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        let value = TestEnveloped { value: 11 };
        store.write_durable(&key, &value).unwrap();

        match store.read(&key).unwrap() {
            Loaded::Found(stored) => assert_eq!(stored.value, value),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_payload_is_unreadable_and_names_the_missing_envelope() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestPayload>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        std::fs::write(
            store.path(&key),
            r#"{"version":2,"diff_hash":"agent","value":5}"#,
        )
        .unwrap();

        match store.read(&key).unwrap() {
            Loaded::Unreadable(Unreadable::Malformed(reason)) => {
                assert!(reason.contains("missing envelope"), "{reason}");
            }
            other => panic!("expected a missing-envelope error, got {other:?}"),
        }
    }

    #[test]
    fn unknown_version_is_unreadable_not_a_partial_read() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let enveloped = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&enveloped).parent().unwrap()).unwrap();
        std::fs::write(
            store.path(&enveloped),
            r#"{"kind":"test-enveloped","version":1,"payload":{"value":1}}"#,
        )
        .unwrap();
        assert_eq!(
            store.read(&enveloped).unwrap(),
            Loaded::Unreadable(Unreadable::UnknownVersion { found: 1 })
        );

        let payload = key::<TestPayload>(&scope, ());
        std::fs::write(
            store.path(&payload),
            r#"{"kind":"test-payload","version":1,"payload":{"version":1,"diff_hash":"h","value":1}}"#,
        )
        .unwrap();
        assert_eq!(
            store.read(&payload).unwrap(),
            Loaded::Unreadable(Unreadable::UnknownVersion { found: 1 })
        );
    }

    #[test]
    fn a_payload_without_an_envelope_is_unreadable() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestPayload>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        std::fs::write(store.path(&key), r#"{"diff_hash":"h","value":1}"#).unwrap();
        assert!(matches!(
            store.read(&key).unwrap(),
            Loaded::Unreadable(Unreadable::Malformed(_))
        ));
    }

    #[test]
    fn envelope_of_another_kind_is_unreadable() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        std::fs::write(
            store.path(&key),
            r#"{"kind":"other","version":2,"payload":{}}"#,
        )
        .unwrap();
        assert_eq!(
            store.read(&key).unwrap(),
            Loaded::Unreadable(Unreadable::WrongKind {
                found: "other".to_string()
            })
        );
    }

    #[test]
    fn malformed_json_is_unreadable_and_the_file_is_left_untouched() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestPayload>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        std::fs::write(store.path(&key), "{not json").unwrap();
        let before = std::fs::read(store.path(&key)).unwrap();
        assert!(matches!(
            store.read(&key).unwrap(),
            Loaded::Unreadable(Unreadable::Malformed(_))
        ));
        assert_eq!(std::fs::read(store.path(&key)).unwrap(), before);
    }

    #[test]
    fn a_file_over_the_size_limit_is_too_large_and_is_not_read() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestPayload>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        let file = std::fs::File::create(store.path(&key)).unwrap();
        file.set_len(MAX_ARTIFACT_BYTES + 1).unwrap();
        assert_eq!(
            store.read(&key).unwrap(),
            Loaded::Unreadable(Unreadable::TooLarge {
                bytes: MAX_ARTIFACT_BYTES + 1
            })
        );
    }

    #[test]
    fn a_missing_file_is_absent() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        assert_eq!(store.read(&key).unwrap(), Loaded::Absent);
    }

    #[test]
    fn a_successful_write_leaves_no_tmp_file_and_a_failed_rename_cleans_up() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        store.write(&key, &TestEnveloped { value: 1 }).unwrap();
        let parent = store.path(&key).parent().unwrap().to_path_buf();
        let mut names: Vec<String> = std::fs::read_dir(&parent)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![".lock".to_string(), "test-enveloped.json".to_string()]
        );

        std::fs::remove_file(store.path(&key)).unwrap();
        std::fs::create_dir_all(store.path(&key)).unwrap();
        assert!(store.write(&key, &TestEnveloped { value: 2 }).is_err());
        let leftovers: Vec<String> = std::fs::read_dir(&parent)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "left {leftovers:?}");
    }

    #[test]
    fn two_writes_in_one_process_use_different_tmp_names() {
        let path = Path::new("/tmp/a.json");
        assert_ne!(
            crate::atomic::unique_tmp_path(path),
            crate::atomic::unique_tmp_path(path)
        );
    }

    #[test]
    fn dir_scope_accepts_any_directory_and_rejects_empty_and_dotdot() {
        let bucket = TempDir::new().unwrap();
        let scope = Scope::dir(bucket.path()).unwrap();
        let store = ArtifactStore::new("/somewhere/else");
        let key = key::<TestEnveloped>(&scope, ());
        assert_eq!(store.path(&key), bucket.path().join("test-enveloped.json"));
        assert!(Scope::dir("").is_err());
        assert!(Scope::dir("a/../b").is_err());
    }

    #[test]
    fn scope_paths_follow_the_documented_layout() {
        let store = ArtifactStore::new("/root");
        let branch = Scope::Branch {
            repo: Slug::new("my-repo").unwrap(),
            branch: Slug::new("feature-x").unwrap(),
            view: BranchView::Unstaged,
        };
        assert_eq!(
            store.scope_dir(&branch),
            Path::new("/root/repos/my-repo/branches/feature-x/view-buckets/unstaged")
        );
        let pr = Scope::Pr {
            repo: Slug::new("owner-repo").unwrap(),
            number: 42,
        };
        assert_eq!(
            store.scope_dir(&pr),
            Path::new("/root/repos/owner-repo/prs/pr-42")
        );
        assert_eq!(store.scope_dir(&Scope::App), Path::new("/root/app"));
    }

    #[test]
    fn a_bare_file_with_another_diff_hash_is_found() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestPayload>(&scope, ());
        let value = TestPayload {
            version: TestPayload::VERSION,
            diff_hash: "old".to_string(),
            value: 3,
        };
        store.write(&key, &value).unwrap();
        match store.read(&key).unwrap() {
            Loaded::Found(stored) => assert_eq!(stored.value.diff_hash, "old"),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn list_returns_sorted_slots_and_skips_tmp_and_foreign_files() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let experts = store.scope_dir(&scope).join("experts");
        std::fs::create_dir_all(&experts).unwrap();
        for name in [
            "security.json",
            "api.json",
            ".x.json",
            "x.prev.json",
            "x.txt",
            ".x.json.tmp.123.0",
        ] {
            std::fs::write(experts.join(name), "{}").unwrap();
        }
        let slots = store.list::<TestExpert>(&scope).unwrap();
        assert_eq!(
            slots,
            vec![
                ExpertId::new("api").unwrap(),
                ExpertId::new("security").unwrap()
            ]
        );
    }

    #[test]
    fn delete_removes_the_file_and_is_idempotent() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        store.write(&key, &TestEnveloped { value: 1 }).unwrap();
        assert!(store.exists(&key));
        store.delete(&key).unwrap();
        assert!(!store.exists(&key));
        store.delete(&key).unwrap();
    }

    #[test]
    fn path_matches_the_file_that_was_written() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        assert_eq!(
            store.path(&key),
            directory.path().join("test-enveloped.json")
        );
        store.write(&key, &TestEnveloped { value: 1 }).unwrap();
        assert!(store.path(&key).is_file());
    }

    #[test]
    fn a_directory_at_the_target_path_is_unreadable() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&key)).unwrap();
        assert_eq!(
            store.read(&key).unwrap(),
            Loaded::Unreadable(Unreadable::Malformed("not a regular file".to_string()))
        );
    }

    #[test]
    fn migrate_returning_another_error_is_malformed() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestMigrateErr>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        std::fs::write(store.path(&key), r#"{"version":1,"value":1}"#).unwrap();
        assert!(matches!(
            store.read(&key).unwrap(),
            Loaded::Unreadable(Unreadable::Malformed(_))
        ));
    }

    #[test]
    fn a_newer_envelope_version_is_unreadable() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        std::fs::write(
            store.path(&key),
            r#"{"kind":"test-enveloped","version":99,"payload":{"value":1}}"#,
        )
        .unwrap();
        assert_eq!(
            store.read(&key).unwrap(),
            Loaded::Unreadable(Unreadable::NewerVersion { found: 99 })
        );
    }

    #[test]
    fn write_refuses_to_replace_a_newer_file() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        let newer = r#"{"kind":"test-enveloped","version":99,"payload":{"value":1}}"#;
        std::fs::write(store.path(&key), newer).unwrap();
        let error = store.write(&key, &TestEnveloped { value: 2 }).unwrap_err();
        match error {
            StoreError::NewerVersion {
                found, expected, ..
            } => {
                assert_eq!(found, 99);
                assert_eq!(expected, TestEnveloped::VERSION);
            }
            other => panic!("expected NewerVersion, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(store.path(&key)).unwrap(), newer);
    }

    #[test]
    fn write_refuses_an_envelope_over_the_size_limit() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestBigEnveloped>(&scope, ());
        let blob = "x".repeat(usize::try_from(MAX_ARTIFACT_BYTES).unwrap() - 100);
        let value = TestBigEnveloped { blob };
        let payload_bytes =
            u64::try_from(serde_json::to_string_pretty(&value).unwrap().len()).unwrap();
        assert!(payload_bytes <= MAX_ARTIFACT_BYTES, "{payload_bytes}");

        assert!(matches!(
            store.write(&key, &value),
            Err(StoreError::TooLarge { .. })
        ));
        assert!(!store.path(&key).exists());
    }

    #[test]
    fn write_refuses_a_payload_over_the_size_limit() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestBig>(&scope, ());
        let blob = "x".repeat(usize::try_from(MAX_ARTIFACT_BYTES).unwrap() + 1);
        let value = TestBig {
            version: TestBig::VERSION,
            diff_hash: "h".to_string(),
            blob,
        };
        assert!(matches!(
            store.write(&key, &value),
            Err(StoreError::TooLarge { .. })
        ));
        assert!(!store.path(&key).exists());
    }

    #[test]
    fn app_scope_path_is_root_app_file() {
        let store = ArtifactStore::new("/root");
        let key = key::<TestEnveloped>(&Scope::App, ());
        assert_eq!(store.path(&key), Path::new("/root/app/test-enveloped.json"));
    }

    #[test]
    fn text_write_then_read_round_trips_without_an_envelope() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let file = RelPath::new("summary.md").unwrap();
        store.write_text(&scope, &file, "hello").unwrap();
        assert_eq!(
            std::fs::read_to_string(store.text_path(&scope, &file)).unwrap(),
            "hello"
        );
        match store.read_text(&scope, &file).unwrap() {
            Loaded::Found(stored) => {
                assert_eq!(stored.value, "hello");
                assert!(stored.meta.is_none());
            }
            other => panic!("expected Found, got {other:?}"),
        }
        assert_eq!(
            store
                .read_text(&scope, &RelPath::new("missing.md").unwrap())
                .unwrap(),
            Loaded::Absent
        );

        let large = RelPath::new("large.md").unwrap();
        let handle = std::fs::File::create(store.text_path(&scope, &large)).unwrap();
        handle.set_len(MAX_ARTIFACT_BYTES + 1).unwrap();
        assert_eq!(
            store.read_text(&scope, &large).unwrap(),
            Loaded::Unreadable(Unreadable::TooLarge {
                bytes: MAX_ARTIFACT_BYTES + 1
            })
        );

        store.delete_text(&scope, &file).unwrap();
        store.delete_text(&scope, &file).unwrap();
        assert_eq!(store.read_text(&scope, &file).unwrap(), Loaded::Absent);
    }

    #[test]
    fn default_migrate_rejects_every_version() {
        assert!(TestPayload::migrate(0, json!({})).is_err());
    }

    #[test]
    fn registry_flags_a_file_no_kind_claims() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        std::fs::create_dir_all(store.scope_dir(&scope).join("experts")).unwrap();
        std::fs::write(store.scope_dir(&scope).join("experts/security.json"), "{}").unwrap();
        std::fs::write(store.scope_dir(&scope).join("junk.txt"), "{}").unwrap();
        let mut registry = crate::registry::Registry::new();
        registry.register::<TestExpert>().unwrap();
        let unaccounted = registry.unaccounted(&store.scope_dir(&scope), &[]).unwrap();
        assert_eq!(unaccounted, vec![Path::new("junk.txt").to_path_buf()]);
    }

    #[test]
    fn registry_accepts_registered_slot_patterns_and_ignored_files() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        std::fs::create_dir_all(store.scope_dir(&scope).join("experts")).unwrap();
        std::fs::write(store.scope_dir(&scope).join("experts/security.json"), "{}").unwrap();
        std::fs::write(store.scope_dir(&scope).join(".diff-tmp.sha256"), "h").unwrap();
        let mut registry = crate::registry::Registry::new();
        registry.register::<TestExpert>().unwrap();
        let ignore = vec![crate::registry::Pattern::new(".diff-tmp.sha256")];
        assert!(registry
            .unaccounted(&store.scope_dir(&scope), &ignore)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn registry_rejects_a_duplicate_kind_with_another_pattern() {
        let mut registry = crate::registry::Registry::new();
        registry.register::<TestExpert>().unwrap();
        assert!(matches!(
            registry.register::<TestExpertAlt>(),
            Err(StoreError::DuplicateRegistration { kind }) if kind == "test-expert"
        ));
    }

    #[test]
    fn registry_ignores_flow_backups() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let backup = store.scope_dir(&scope).join("flow-backups/accept-1");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("review.json"), "{}").unwrap();
        let mut registry = crate::registry::Registry::new();
        registry.register::<TestEnveloped>().unwrap();
        let ignore = vec![crate::registry::Pattern::new("flow-backups/**")];
        assert!(registry
            .unaccounted(&store.scope_dir(&scope), &ignore)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn registry_rejects_overlapping_patterns_in_both_orders() {
        let mut registry = crate::registry::Registry::new();
        registry.register::<TestExpert>().unwrap();
        assert!(registry.register::<TestExpertSingleton>().is_err());

        let mut registry = crate::registry::Registry::new();
        registry.register::<TestExpertSingleton>().unwrap();
        assert!(registry.register::<TestExpert>().is_err());
    }

    #[test]
    fn registry_accepts_a_slot_pattern_next_to_a_root_file() {
        let mut registry = crate::registry::Registry::new();
        registry.register::<TestExpert>().unwrap();
        registry.register::<TestGeneral>().unwrap();
    }

    #[test]
    fn registry_rejects_an_absolute_template() {
        let mut registry = crate::registry::Registry::new();
        assert!(matches!(
            registry.register::<TestAbsoluteTemplate>(),
            Err(StoreError::InvalidPath { .. })
        ));
    }

    #[test]
    fn registry_rejects_a_dotdot_template() {
        let mut registry = crate::registry::Registry::new();
        assert!(matches!(
            registry.register::<TestDotDotTemplate>(),
            Err(StoreError::InvalidPath { .. })
        ));
    }

    #[test]
    fn registry_rejects_a_singleton_template_with_a_slot() {
        let mut registry = crate::registry::Registry::new();
        assert!(matches!(
            registry.register::<TestBadSingleton>(),
            Err(StoreError::InvalidPath { .. })
        ));
    }

    #[test]
    fn registry_rejects_a_multi_template_without_a_slot() {
        let mut registry = crate::registry::Registry::new();
        assert!(matches!(
            registry.register::<TestBadMulti>(),
            Err(StoreError::InvalidPath { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn registry_reports_a_symlinked_file() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let target = store.scope_dir(&scope).join("target.bin");
        std::fs::write(&target, "x").unwrap();
        std::os::unix::fs::symlink(&target, store.scope_dir(&scope).join("link.bin")).unwrap();
        let registry = crate::registry::Registry::new();
        let unaccounted = registry.unaccounted(&store.scope_dir(&scope), &[]).unwrap();
        assert_eq!(
            unaccounted,
            vec![
                Path::new("link.bin").to_path_buf(),
                Path::new("target.bin").to_path_buf()
            ]
        );
    }

    #[test]
    fn registry_accepts_a_registered_text_file() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        std::fs::write(store.scope_dir(&scope).join("summary.md"), "hi").unwrap();
        let mut registry = crate::registry::Registry::new();
        registry.register_text("summary", "summary.md").unwrap();
        assert!(registry
            .unaccounted(&store.scope_dir(&scope), &[])
            .unwrap()
            .is_empty());
    }
}
