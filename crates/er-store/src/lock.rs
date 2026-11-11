//! Cross-process locks on a store bucket.
//!
//! The engine's `ARTIFACT_LOCK` (`ai/prepared_diff.rs`) is a separate in-process
//! mutex for `diff-tmp` and `diff-annotated`: agent inputs, not store artifacts.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::error::StoreError;
use crate::key::Scope;
use crate::store::{now_epoch_secs, ArtifactStore};

/// How long a caller waits for a busy bucket before `Locked` or `LeaseHeld`.
pub const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a waiting caller retries `try_lock`.
pub const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Who holds a lock, written into the lock file for a failed acquirer.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HolderInfo {
    pub pid: u32,
    /// Unix seconds when the lock was acquired.
    pub acquired_at_unix: u64,
    pub label: String,
}

impl HolderInfo {
    fn new(label: &str) -> Self {
        Self {
            pid: std::process::id(),
            acquired_at_unix: now_epoch_secs(),
            label: label.to_string(),
        }
    }
}

/// The two lock files a scope can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LockKind {
    Write,
    Lease,
}

impl LockKind {
    fn file_name(self) -> &'static str {
        match self {
            Self::Write => ".lock",
            Self::Lease => ".lease",
        }
    }
}

/// Process-wide re-entrancy map: the thread holding each canonical scope
/// directory for a lock kind. It is not a substitute for the OS lock; it turns
/// a same-thread re-lock into an immediate error. Another thread's entry is
/// ignored and the file poll handles it.
static HELD_LOCKS: OnceLock<Mutex<HashMap<(PathBuf, LockKind), std::thread::ThreadId>>> =
    OnceLock::new();

fn held_locks() -> &'static Mutex<HashMap<(PathBuf, LockKind), std::thread::ThreadId>> {
    HELD_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn release(dir: &Path, kind: LockKind) {
    let mut held = held_locks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    held.remove(&(dir.to_path_buf(), kind));
}

/// The write lock on one bucket. Dropping it unlocks the bucket.
pub struct ScopeGuard {
    file: File,
    scope: Scope,
    dir: PathBuf,
    kind: LockKind,
}

impl ScopeGuard {
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn scope_dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        release(&self.dir, self.kind);
        let _ = self.file.unlock();
    }
}

/// A lease held by a workflow across many writes. Dropping it releases the lease.
pub struct LeaseGuard {
    file: File,
    scope: Scope,
    dir: PathBuf,
    kind: LockKind,
    holder: HolderInfo,
}

impl LeaseGuard {
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn scope_dir(&self) -> &Path {
        &self.dir
    }

    pub fn holder(&self) -> &HolderInfo {
        &self.holder
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        release(&self.dir, self.kind);
        let _ = self.file.unlock();
    }
}

impl ArtifactStore {
    /// Take the scope's write lock, waiting up to `timeout` for a busy bucket.
    pub fn lock_scope(&self, scope: &Scope, timeout: Duration) -> Result<ScopeGuard, StoreError> {
        let (file, dir) = self.acquire(scope, LockKind::Write, timeout)?;
        if let Err(error) = write_holder(&file, &HolderInfo::new("write")) {
            release(&dir, LockKind::Write);
            return Err(error);
        }
        Ok(ScopeGuard {
            file,
            scope: scope.clone(),
            dir,
            kind: LockKind::Write,
        })
    }

    /// Take the scope's lease, waiting up to `timeout` for a busy bucket.
    pub fn lease(
        &self,
        scope: &Scope,
        timeout: Duration,
        label: &str,
    ) -> Result<LeaseGuard, StoreError> {
        let (file, dir) = self.acquire(scope, LockKind::Lease, timeout)?;
        let holder = HolderInfo::new(label);
        if let Err(error) = write_holder(&file, &holder) {
            release(&dir, LockKind::Lease);
            return Err(error);
        }
        Ok(LeaseGuard {
            file,
            scope: scope.clone(),
            dir,
            kind: LockKind::Lease,
            holder,
        })
    }

    fn acquire(
        &self,
        scope: &Scope,
        kind: LockKind,
        timeout: Duration,
    ) -> Result<(File, PathBuf), StoreError> {
        let raw = self.scope_dir(scope);
        std::fs::create_dir_all(&raw)?;
        let dir = std::fs::canonicalize(&raw)?;
        {
            let held = held_locks()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if held.get(&(dir.clone(), kind)) == Some(&std::thread::current().id()) {
                return Err(StoreError::AlreadyLocked { path: dir });
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(kind.file_name()))?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    if start.elapsed() >= timeout {
                        let holder = read_holder(&file);
                        return Err(match kind {
                            LockKind::Write => StoreError::Locked { path: dir, holder },
                            LockKind::Lease => StoreError::LeaseHeld { path: dir, holder },
                        });
                    }
                    std::thread::sleep(LOCK_POLL_INTERVAL);
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        held_locks()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert((dir.clone(), kind), std::thread::current().id());
        Ok((file, dir))
    }
}

fn write_holder(file: &File, holder: &HolderInfo) -> Result<(), StoreError> {
    let json = serde_json::to_string(holder)?;
    file.set_len(0)?;
    let mut handle = file;
    handle.write_all(json.as_bytes())?;
    Ok(())
}

fn read_holder(file: &File) -> Option<HolderInfo> {
    let mut text = String::new();
    let mut handle = file;
    handle.read_to_string(&mut text).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::{Duration, Instant};

    use tempfile::TempDir;

    use super::*;
    use crate::artifact::Artifact;
    use crate::envelope::{Loaded, Stored};
    use crate::error::UpdateError;
    use crate::key::{ArtifactKey, RelPath};
    use crate::registry::Registry;
    use crate::test_support::{
        ignore_patterns, TestBare, TestCounter, TestEnveloped, TestEnvelopedHash, TestGeneral,
    };

    const INCREMENT_ROUNDS: u32 = 25;

    struct ThreadHolder {
        handle: std::thread::JoinHandle<()>,
        ready: mpsc::Receiver<()>,
        release: mpsc::Sender<()>,
    }

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

    fn bare_value() -> TestBare {
        TestBare {
            version: TestBare::VERSION,
            diff_hash: "h".to_string(),
            value: 2,
        }
    }

    fn increment(current: Option<TestCounter>) -> Result<TestCounter, std::convert::Infallible> {
        // Widen the read-to-write window a lost update would need.
        std::thread::sleep(Duration::from_millis(2));
        Ok(TestCounter {
            version: TestCounter::VERSION,
            diff_hash: "h".to_string(),
            value: current.map_or(0, |counter| counter.value) + 1,
        })
    }

    fn hold_lock_in_thread(
        store: &ArtifactStore,
        scope: &Scope,
        kind: LockKind,
        label: &'static str,
    ) -> ThreadHolder {
        let (ready_tx, ready) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let store = store.clone();
        let scope = scope.clone();
        let handle = std::thread::spawn(move || match kind {
            LockKind::Write => {
                let guard = store.lock_scope(&scope, Duration::from_secs(5)).unwrap();
                ready_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                drop(guard);
            }
            LockKind::Lease => {
                let guard = store.lease(&scope, Duration::from_secs(5), label).unwrap();
                ready_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                drop(guard);
            }
        });
        ThreadHolder {
            handle,
            ready,
            release,
        }
    }

    fn spawn_helper(root: &Path, mode: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["lock_helper_child", "--nocapture"])
            .env("ER_STORE_LOCK_HELPER", root)
            .env("ER_STORE_LOCK_HELPER_MODE", mode)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn wait_for_line(mut child: std::process::Child, expected: &str) -> std::process::Child {
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(line) if line.trim() == expected => return child,
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    panic!("helper did not report {expected}")
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("helper stdout closed before {expected}")
                }
            }
        }
    }

    fn spawn_increment_helper(root: &Path, mode: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["lock_helper_child", "--nocapture"])
            .env("ER_STORE_LOCK_HELPER", root)
            .env("ER_STORE_LOCK_HELPER_MODE", mode)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    fn start_increment_helper(directory: &TempDir, mode: &str) -> std::process::Child {
        let child = spawn_increment_helper(directory.path(), mode);
        let ready = directory.path().join("ready");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(Instant::now() < deadline, "the helper never became ready");
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::write(directory.path().join("start"), "go").unwrap();
        child
    }

    #[test]
    fn second_writer_waits_for_the_first() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let first = key::<TestEnveloped>(&scope, ());
        let guard = store.lock_scope(&scope, Duration::from_secs(5)).unwrap();
        store
            .write_locked(&guard, &first, &TestEnveloped { value: 1 })
            .unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let (done_tx, done_rx) = mpsc::channel();
        let writer = {
            let store = store.clone();
            let scope = scope.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let key = ArtifactKey::<TestBare>::new(scope, ()).unwrap();
                let started = Instant::now();
                store.write(&key, &bare_value()).unwrap();
                done_tx.send(started.elapsed()).unwrap();
            })
        };
        barrier.wait();
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            done_rx.try_recv().is_err(),
            "the second write finished while the guard was held"
        );
        drop(guard);
        let waited = done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        writer.join().unwrap();
        assert!(
            waited >= Duration::from_millis(100),
            "waited only {waited:?}"
        );

        let second = key::<TestBare>(&scope, ());
        assert!(matches!(store.read(&first).unwrap(), Loaded::Found(_)));
        assert!(matches!(store.read(&second).unwrap(), Loaded::Found(_)));
    }

    #[test]
    fn two_threads_increment_under_update() {
        let (directory, store) = temp_store();
        let mut child = start_increment_helper(&directory, "increment");
        let counter = ArtifactKey::<TestCounter>::new(Scope::App, ()).unwrap();
        for _ in 0..INCREMENT_ROUNDS {
            store
                .update(&counter, Duration::from_secs(10), increment)
                .unwrap();
        }
        assert!(child.wait().unwrap().success());
        match store.read(&counter).unwrap() {
            Loaded::Found(stored) => assert_eq!(stored.value.value, 2 * INCREMENT_ROUNDS),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn two_threads_increment_under_update_durable() {
        let (directory, store) = temp_store();
        let mut child = start_increment_helper(&directory, "increment_durable");
        let counter = ArtifactKey::<TestCounter>::new(Scope::App, ()).unwrap();
        for _ in 0..INCREMENT_ROUNDS {
            store
                .update_durable(&counter, Duration::from_secs(10), increment)
                .unwrap();
        }
        assert!(child.wait().unwrap().success());
        match store.read(&counter).unwrap() {
            Loaded::Found(stored) => assert_eq!(stored.value.value, 2 * INCREMENT_ROUNDS),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn write_takes_the_scope_lock() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let holder = hold_lock_in_thread(&store, &scope, LockKind::Write, "write");
        holder.ready.recv().unwrap();
        let key = key::<TestEnveloped>(&scope, ());
        let error = store
            .write_with_timeout(
                &key,
                &TestEnveloped { value: 1 },
                Duration::from_millis(100),
            )
            .unwrap_err();
        assert!(matches!(error, StoreError::Locked { .. }), "{error:?}");
        holder.release.send(()).unwrap();
        holder.handle.join().unwrap();
        assert!(!store.path(&key).exists());
    }

    #[test]
    fn write_durable_takes_the_scope_lock() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        let holder = hold_lock_in_thread(&store, &scope, LockKind::Write, "write");
        holder.ready.recv().unwrap();
        let error = store
            .write_durable_with_timeout(
                &key,
                &TestEnveloped { value: 1 },
                Duration::from_millis(100),
            )
            .unwrap_err();
        assert!(matches!(error, StoreError::Locked { .. }), "{error:?}");
        holder.release.send(()).unwrap();
        holder.handle.join().unwrap();

        let guard = store.lock_scope(&scope, Duration::from_secs(5)).unwrap();
        store
            .write_durable_locked(&guard, &key, &TestEnveloped { value: 2 })
            .unwrap();
        drop(guard);
        match store.read(&key).unwrap() {
            Loaded::Found(stored) => assert_eq!(stored.value, TestEnveloped { value: 2 }),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn same_thread_relock_fails_at_once() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        let guard = store.lock_scope(&scope, Duration::from_secs(5)).unwrap();
        let started = Instant::now();
        let error = store.write(&key, &TestEnveloped { value: 1 }).unwrap_err();
        assert!(
            matches!(error, StoreError::AlreadyLocked { .. }),
            "{error:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(guard);
    }

    #[test]
    fn guard_commits_several_artifacts_under_one_hold() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let envelope = key::<TestEnveloped>(&scope, ());
        let bare = key::<TestBare>(&scope, ());
        let hash = key::<TestEnvelopedHash>(&scope, ());
        let summary = RelPath::new("summary.md").unwrap();
        let guard = store.lock_scope(&scope, Duration::from_secs(5)).unwrap();
        store
            .write_locked(&guard, &envelope, &TestEnveloped { value: 1 })
            .unwrap();
        store.write_locked(&guard, &bare, &bare_value()).unwrap();
        store
            .write_locked(
                &guard,
                &hash,
                &TestEnvelopedHash {
                    value: 2,
                    diff_hash: "h".to_string(),
                },
            )
            .unwrap();
        store.write_text_locked(&guard, &summary, "hi").unwrap();

        let (done_tx, done_rx) = mpsc::channel();
        let waiter = {
            let store = store.clone();
            let scope = scope.clone();
            std::thread::spawn(move || {
                let key = ArtifactKey::<TestEnvelopedHash>::new(scope, ()).unwrap();
                store
                    .write(
                        &key,
                        &TestEnvelopedHash {
                            value: 3,
                            diff_hash: "h".to_string(),
                        },
                    )
                    .unwrap();
                done_tx.send(()).unwrap();
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(done_rx.try_recv().is_err());
        drop(guard);
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        waiter.join().unwrap();

        assert!(matches!(store.read(&envelope).unwrap(), Loaded::Found(_)));
        assert!(matches!(store.read(&bare).unwrap(), Loaded::Found(_)));
        assert!(matches!(store.read(&hash).unwrap(), Loaded::Found(_)));
        assert_eq!(
            store.read_text(&scope, &summary).unwrap(),
            Loaded::Found(Stored {
                value: "hi".to_string(),
                meta: None,
            })
        );

        let other = TempDir::new().unwrap();
        let other_scope = Scope::dir(other.path()).unwrap();
        let other_guard = store
            .lock_scope(&other_scope, Duration::from_secs(5))
            .unwrap();
        assert!(matches!(
            store.write_locked(&other_guard, &envelope, &TestEnveloped { value: 4 }),
            Err(StoreError::WrongScope { .. })
        ));
    }

    #[test]
    fn guard_deletes_several_artifacts_under_one_hold() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let envelope = key::<TestEnveloped>(&scope, ());
        let bare = key::<TestBare>(&scope, ());
        let general = key::<TestGeneral>(&scope, ());
        let summary = RelPath::new("summary.md").unwrap();
        store.write(&envelope, &TestEnveloped { value: 1 }).unwrap();
        store.write(&bare, &bare_value()).unwrap();
        store.write(&general, &TestGeneral { value: 2 }).unwrap();
        store.write_text(&scope, &summary, "hi").unwrap();

        let guard = store.lock_scope(&scope, Duration::from_secs(5)).unwrap();
        store.delete_locked(&guard, &envelope).unwrap();
        store.delete_locked(&guard, &bare).unwrap();
        store.delete_locked(&guard, &general).unwrap();
        store.delete_text_locked(&guard, &summary).unwrap();
        drop(guard);

        assert!(!store.path(&envelope).is_file());
        assert!(!store.path(&bare).is_file());
        assert!(!store.path(&general).is_file());
        assert_eq!(store.read_text(&scope, &summary).unwrap(), Loaded::Absent);
    }

    #[test]
    fn lock_acquire_times_out_with_a_locked_error() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let holder = hold_lock_in_thread(&store, &scope, LockKind::Write, "write");
        holder.ready.recv().unwrap();
        let started = Instant::now();
        let error = match store.lock_scope(&scope, Duration::from_millis(100)) {
            Ok(_guard) => panic!("expected a timeout"),
            Err(error) => error,
        };
        assert!(started.elapsed() >= Duration::from_millis(100));
        match error {
            StoreError::Locked { path, holder } => {
                assert_eq!(
                    path,
                    std::fs::canonicalize(store.scope_dir(&scope)).unwrap()
                );
                if let Some(holder) = holder {
                    assert_eq!(holder.pid, std::process::id());
                    assert_eq!(holder.label, "write");
                } else {
                    // Windows may not allow reading the locked file.
                    #[cfg(not(windows))]
                    panic!("holder JSON missing");
                }
            }
            other => panic!("expected Locked, got {other:?}"),
        }
        holder.release.send(()).unwrap();
        holder.handle.join().unwrap();
    }

    #[test]
    fn update_refuses_a_newer_file() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        let newer = r#"{"kind":"test-enveloped","version":99,"payload":{"value":1}}"#;
        std::fs::write(store.path(&key), newer).unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let result = store.update(&key, Duration::from_secs(5), move |_current| {
            flag.store(true, Ordering::Relaxed);
            Ok::<_, std::convert::Infallible>(TestEnveloped { value: 2 })
        });
        assert!(matches!(
            result,
            Err(UpdateError::Store(StoreError::NewerVersion { .. }))
        ));
        assert!(!called.load(Ordering::Relaxed));
        assert_eq!(std::fs::read_to_string(store.path(&key)).unwrap(), newer);
    }

    #[test]
    fn update_durable_refuses_a_newer_file() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        std::fs::create_dir_all(store.path(&key).parent().unwrap()).unwrap();
        let newer = r#"{"kind":"test-enveloped","version":99,"payload":{"value":1}}"#;
        std::fs::write(store.path(&key), newer).unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let result = store.update_durable(&key, Duration::from_secs(5), move |_current| {
            flag.store(true, Ordering::Relaxed);
            Ok::<_, std::convert::Infallible>(TestEnveloped { value: 2 })
        });
        assert!(matches!(
            result,
            Err(UpdateError::Store(StoreError::NewerVersion { .. }))
        ));
        assert!(!called.load(Ordering::Relaxed));
        assert_eq!(std::fs::read_to_string(store.path(&key)).unwrap(), newer);
    }

    #[test]
    fn dead_holder_releases_the_lock() {
        let (directory, store) = temp_store();
        let mut child = wait_for_line(spawn_helper(directory.path(), "write"), "LOCKED");
        child.kill().unwrap();
        child.wait().unwrap();
        let guard = store
            .lock_scope(&Scope::App, Duration::from_secs(2))
            .unwrap();
        drop(guard);
    }

    #[cfg(windows)]
    #[test]
    fn windows_dead_holder_releases_the_lock() {
        // No CI job runs this; the Windows lock path is unverified.
        let (directory, store) = temp_store();
        let mut child = wait_for_line(spawn_helper(directory.path(), "write"), "LOCKED");
        child.kill().unwrap();
        child.wait().unwrap();
        let guard = store
            .lock_scope(&Scope::App, Duration::from_secs(2))
            .unwrap();
        drop(guard);
    }

    #[test]
    fn lease_reports_its_holder_when_held() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let holder = hold_lock_in_thread(&store, &scope, LockKind::Lease, "first");
        holder.ready.recv().unwrap();
        let error = match store.lease(&scope, Duration::from_millis(100), "second") {
            Ok(_lease) => panic!("expected a lease conflict"),
            Err(error) => error,
        };
        match error {
            StoreError::LeaseHeld { path, holder } => {
                assert_eq!(
                    path,
                    std::fs::canonicalize(store.scope_dir(&scope)).unwrap()
                );
                if let Some(holder) = holder {
                    assert_eq!(holder.pid, std::process::id());
                    assert_eq!(holder.label, "first");
                } else {
                    // Windows may not allow reading the locked file.
                    #[cfg(not(windows))]
                    panic!("holder JSON missing");
                }
            }
            other => panic!("expected LeaseHeld, got {other:?}"),
        }
        holder.release.send(()).unwrap();
        holder.handle.join().unwrap();
    }

    #[test]
    fn lease_is_free_after_its_holder_dies() {
        let (directory, store) = temp_store();
        let mut child = wait_for_line(spawn_helper(directory.path(), "lease"), "LOCKED");
        child.kill().unwrap();
        child.wait().unwrap();
        let lease = store
            .lease(&Scope::App, Duration::from_secs(2), "parent")
            .unwrap();
        drop(lease);
    }

    #[test]
    fn lease_and_write_lock_are_independent() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let key = key::<TestEnveloped>(&scope, ());
        let lease = store
            .lease(&scope, Duration::from_secs(5), "workflow")
            .unwrap();
        store.write(&key, &TestEnveloped { value: 1 }).unwrap();
        assert!(store.path(&key).is_file());
        // Sidecar writers use `update`, which takes the write lock; it must not
        // wait on the lease the flow holds. A zero timeout makes a shared lock
        // fail here instead of sleeping for the default.
        let (updated, _) = store
            .update(&key, Duration::ZERO, |current| {
                let mut value = current.expect("the record was just written");
                value.value += 1;
                Ok::<_, ()>(value)
            })
            .expect("update under the lease");
        assert_eq!(updated.value, 2);
        drop(lease);
    }

    #[test]
    fn zero_timeout_lease_try_does_not_wait() {
        let (directory, store) = temp_store();
        let scope = scope(&directory);
        let holder = hold_lock_in_thread(&store, &scope, LockKind::Lease, "holder");
        holder.ready.recv().unwrap();
        let start = Instant::now();
        let error = match store.lease(&scope, Duration::ZERO, "probe") {
            Ok(_lease) => panic!("expected a lease conflict"),
            Err(error) => error,
        };
        assert!(matches!(error, StoreError::LeaseHeld { .. }), "{error:?}");
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "a zero-timeout lease try waited {:?}",
            start.elapsed()
        );
        holder.release.send(()).unwrap();
        holder.handle.join().unwrap();
        let lease = store.lease(&scope, Duration::ZERO, "probe").unwrap();
        drop(lease);
    }

    #[test]
    fn registry_test_ignores_lock_files() {
        let (directory, _store) = temp_store();
        let root = directory.path();
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join(".lock"), "{}").unwrap();
        std::fs::write(root.join(".lease"), "{}").unwrap();
        std::fs::write(root.join("app/.lock"), "{}").unwrap();
        let registry = Registry::new();
        assert!(registry
            .unaccounted(root, &ignore_patterns())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn app_record_write_locks_the_app_scope() {
        let (directory, store) = temp_store();
        let key = key::<TestEnveloped>(&Scope::App, ());
        store.write(&key, &TestEnveloped { value: 1 }).unwrap();
        assert!(directory.path().join("app/.lock").is_file());
    }

    #[test]
    fn lock_helper_child() {
        let Ok(root) = std::env::var("ER_STORE_LOCK_HELPER") else {
            return;
        };
        let mode = std::env::var("ER_STORE_LOCK_HELPER_MODE").unwrap_or_default();
        let store = ArtifactStore::new(&root);
        match mode.as_str() {
            "write" => {
                let _guard = store
                    .lock_scope(&Scope::App, Duration::from_secs(30))
                    .unwrap();
                println!("LOCKED");
                std::thread::sleep(Duration::from_secs(30));
            }
            "lease" => {
                let _guard = store
                    .lease(&Scope::App, Duration::from_secs(30), "helper")
                    .unwrap();
                println!("LOCKED");
                std::thread::sleep(Duration::from_secs(30));
            }
            "increment" | "increment_durable" => {
                let durable = mode == "increment_durable";
                let key = ArtifactKey::<TestCounter>::new(Scope::App, ()).unwrap();
                std::fs::write(Path::new(&root).join("ready"), "ready").unwrap();
                let start = Path::new(&root).join("start");
                let deadline = Instant::now() + Duration::from_secs(10);
                while !start.exists() {
                    assert!(Instant::now() < deadline, "the parent never started");
                    std::thread::sleep(Duration::from_millis(5));
                }
                for _ in 0..INCREMENT_ROUNDS {
                    if durable {
                        store
                            .update_durable(&key, Duration::from_secs(30), increment)
                            .unwrap();
                    } else {
                        store
                            .update(&key, Duration::from_secs(30), increment)
                            .unwrap();
                    }
                }
            }
            _ => {}
        }
    }
}
