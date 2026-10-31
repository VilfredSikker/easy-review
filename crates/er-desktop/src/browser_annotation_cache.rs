//! Off-lock reads of the active view bucket's v2 browser annotations.
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use er_engine::ai::browser_annotations::{
    load_browser_annotations, BrowserAnnotation, BROWSER_ANNOTATIONS_FILE,
};
use er_engine::app::App;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrowserAnnotationsSnapshot {
    pub annotations: Vec<BrowserAnnotation>,
    pub load_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileKey {
    modified: Option<SystemTime>,
    len: u64,
    is_symlink: bool,
}

fn file_key(dir: &Path) -> Result<Option<FileKey>, String> {
    match std::fs::symlink_metadata(dir.join(BROWSER_ANNOTATIONS_FILE)) {
        Ok(m) => Ok(Some(FileKey {
            modified: m.modified().ok(),
            len: m.len(),
            is_symlink: m.file_type().is_symlink(),
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Default)]
struct Cache {
    dir: PathBuf,
    key: Option<Result<Option<FileKey>, String>>,
    value: BrowserAnnotationsSnapshot,
    revision: u64,
    checked_at: Option<Instant>,
}

#[derive(Default)]
struct AnnotationCache(Mutex<Cache>);

static CACHE: LazyLock<AnnotationCache> = LazyLock::new(AnnotationCache::default);
// File timestamps do not describe readability or every external rewrite.
const REVALIDATE_AFTER: Duration = Duration::from_secs(2);

impl AnnotationCache {
    fn refresh_with(
        &self,
        app: &Mutex<App>,
        now: Instant,
        load: impl FnOnce(&Path) -> BrowserAnnotationsSnapshot,
    ) -> bool {
        let Ok(app_guard) = app.lock() else {
            return false;
        };
        let dir = PathBuf::from(app_guard.tab().comments_dir());
        drop(app_guard);
        let key = file_key(&dir);
        if let Ok(cache) = self.0.lock() {
            if cache.dir == dir
                && cache.key.as_ref() == Some(&key)
                && cache.value.load_error.is_none()
                && cache
                    .checked_at
                    .is_some_and(|checked| now.duration_since(checked) < REVALIDATE_AFTER)
            {
                return false;
            }
        }
        let value = load(&dir);
        // Snapshot construction and its sent-revision bookkeeping hold App.
        // Publish under the same lock, after all disk work has finished, so a
        // response cannot mark a newer annotation generation as delivered.
        let Ok(app_guard) = app.lock() else {
            return false;
        };
        if Path::new(&app_guard.tab().comments_dir()) != dir {
            return false;
        }
        let Ok(mut cache) = self.0.lock() else {
            return false;
        };
        let changed = cache.dir != dir || cache.value != value;
        cache.dir = dir;
        cache.key = Some(key);
        cache.value = value;
        cache.checked_at = Some(now);
        if changed {
            cache.revision = cache.revision.wrapping_add(1);
        }
        changed
    }

    fn get(&self, dir: &str) -> (BrowserAnnotationsSnapshot, u64) {
        let Ok(cache) = self.0.lock() else {
            return (BrowserAnnotationsSnapshot::default(), 0);
        };
        if cache.dir == Path::new(dir) {
            (cache.value.clone(), cache.revision)
        } else {
            (BrowserAnnotationsSnapshot::default(), 0)
        }
    }
}

fn load(dir: &Path) -> BrowserAnnotationsSnapshot {
    match load_browser_annotations(dir) {
        Ok(annotations) => BrowserAnnotationsSnapshot {
            annotations,
            load_error: None,
        },
        Err(error) => BrowserAnnotationsSnapshot {
            annotations: Vec::new(),
            load_error: Some(error.to_string()),
        },
    }
}

/// Called on a worker without holding App; all disk work stays outside its lock.
pub fn refresh(app: &Mutex<App>) -> bool {
    CACHE.refresh_with(app, Instant::now(), load)
}

pub fn get(dir: &str) -> (BrowserAnnotationsSnapshot, u64) {
    CACHE.get(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{mpsc, Arc};

    fn app_for(dir: &Path) -> Mutex<App> {
        let mut app = App::new_for_test(Vec::new());
        set_bucket(&mut app, dir);
        Mutex::new(app)
    }

    fn set_bucket(app: &mut App, dir: &Path) {
        app.tab_mut().er_root = er_engine::paths::ErRoot::Managed {
            agent_dir: dir.to_string_lossy().into_owned(),
            session_dir: dir.to_string_lossy().into_owned(),
        };
    }

    fn unreadable(_: &Path) -> BrowserAnnotationsSnapshot {
        BrowserAnnotationsSnapshot {
            annotations: Vec::new(),
            load_error: Some("browser annotation I/O: transient read failure".into()),
        }
    }

    #[test]
    fn snapshot_wire_fixture_and_load_error_are_distinct_from_missing() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_for(dir.path());
        let cache = AnnotationCache::default();
        assert!(cache.refresh_with(&app, Instant::now(), load));
        let (missing, first) = cache.get(dir.path().to_str().unwrap());
        assert_eq!(
            serde_json::to_value(missing).unwrap(),
            json!({"annotations": [], "load_error": null})
        );
        assert!(!cache.refresh_with(&app, Instant::now(), load));
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../desktop-ui/src/lib/fixtures/browser-annotations.json"
        ))
        .unwrap();
        let metadata =
            json!({"version": 2, "next_number": 2, "annotations": fixture["annotations"]});
        std::fs::write(
            dir.path().join(BROWSER_ANNOTATIONS_FILE),
            metadata.to_string(),
        )
        .unwrap();
        assert!(cache.refresh_with(&app, Instant::now(), load));
        let (loaded, second) = cache.get(dir.path().to_str().unwrap());
        assert_ne!(first, second);
        assert_eq!(serde_json::to_value(loaded).unwrap(), fixture);
        std::fs::write(dir.path().join(BROWSER_ANNOTATIONS_FILE), b"broken").unwrap();
        assert!(cache.refresh_with(&app, Instant::now(), load));
        let (failed, third) = cache.get(dir.path().to_str().unwrap());
        assert_ne!(second, third);
        assert!(failed.annotations.is_empty());
        assert!(failed.load_error.is_some());
    }

    #[test]
    fn unchanged_file_key_recovers_from_transient_read_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(BROWSER_ANNOTATIONS_FILE);
        std::fs::write(&path, r#"{"version":2,"next_number":1,"annotations":[]}"#).unwrap();
        let app = app_for(dir.path());
        let cache = AnnotationCache::default();
        let now = Instant::now();
        let key = file_key(dir.path());
        assert!(cache.refresh_with(&app, now, unreadable));
        let (failed, failed_revision) = cache.get(dir.path().to_str().unwrap());
        assert!(failed.load_error.is_some());
        assert!(cache.refresh_with(&app, now, load));
        assert_eq!(file_key(dir.path()), key);
        let (recovered, recovered_revision) = cache.get(dir.path().to_str().unwrap());
        assert!(recovered.load_error.is_none());
        assert_ne!(recovered_revision, failed_revision);
    }

    #[test]
    fn unchanged_file_key_rechecks_readability_without_revision_churn() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_for(dir.path());
        let cache = AnnotationCache::default();
        let now = Instant::now();
        let key = file_key(dir.path());
        assert!(cache.refresh_with(&app, now, load));
        let (_, initial_revision) = cache.get(dir.path().to_str().unwrap());
        assert!(!cache.refresh_with(&app, now + REVALIDATE_AFTER, load));
        assert_eq!(cache.get(dir.path().to_str().unwrap()).1, initial_revision);
        assert!(cache.refresh_with(&app, now + REVALIDATE_AFTER * 2, unreadable));
        assert_eq!(file_key(dir.path()), key);
        assert!(cache
            .get(dir.path().to_str().unwrap())
            .0
            .load_error
            .is_some());
        assert!(!cache.refresh_with(&app, now + REVALIDATE_AFTER * 2, unreadable));
    }

    #[test]
    fn publication_waits_until_snapshot_and_sent_revision_are_complete() {
        let dir = tempfile::tempdir().unwrap();
        let app = Arc::new(app_for(dir.path()));
        let cache = Arc::new(AnnotationCache::default());
        assert!(cache.refresh_with(&app, Instant::now(), load));
        let (read_started_tx, read_started_rx) = mpsc::channel();
        let (finish_read_tx, finish_read_rx) = mpsc::channel();
        let (read_finished_tx, read_finished_rx) = mpsc::channel();
        let (published_tx, published_rx) = mpsc::channel();
        let worker = {
            let app = Arc::clone(&app);
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || {
                let changed = cache.refresh_with(&app, Instant::now() + REVALIDATE_AFTER, |dir| {
                    // The reader must run without App, even during a slow read.
                    assert!(app.try_lock().is_ok());
                    read_started_tx.send(()).unwrap();
                    finish_read_rx.recv().unwrap();
                    read_finished_tx.send(()).unwrap();
                    unreadable(dir)
                });
                published_tx.send(changed).unwrap();
            })
        };
        read_started_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let app_guard = app.lock().unwrap();
        let (snapshot, _) = cache.get(dir.path().to_str().unwrap());
        finish_read_tx.send(()).unwrap();
        read_finished_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(
            published_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        // This is the revision bookkeeping done after building a confirmed
        // response: it must still describe the payload captured above.
        let (still_current, sent_revision) = cache.get(dir.path().to_str().unwrap());
        assert_eq!(snapshot, still_current);
        drop(app_guard);
        assert!(published_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        worker.join().unwrap();
        let (next_snapshot, next_revision) = cache.get(dir.path().to_str().unwrap());
        assert_ne!(
            next_revision, sent_revision,
            "next poll must deliver the new content"
        );
        assert!(next_snapshot.load_error.is_some());
    }

    #[test]
    fn off_lock_read_is_discarded_if_active_bucket_changes() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let app = app_for(first.path());
        let cache = AnnotationCache::default();
        assert!(!cache.refresh_with(&app, Instant::now(), |_| {
            set_bucket(&mut app.lock().unwrap(), second.path());
            unreadable(first.path())
        }));
        let (value, revision) = cache.get(second.path().to_str().unwrap());
        assert_eq!(value, BrowserAnnotationsSnapshot::default());
        assert_eq!(revision, 0);
        assert!(cache.refresh_with(&app, Instant::now(), load));
        assert_eq!(cache.get(first.path().to_str().unwrap()).1, 0);
    }
}
