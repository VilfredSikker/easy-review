//! Off-lock reads of the active view bucket's v2 browser annotations.
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use er_engine::ai::browser_annotations::{
    load_browser_annotations, BrowserAnnotation, BROWSER_ANNOTATIONS_FILE,
};

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
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
}
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache::default()));

/// Called on a worker, without the App lock. Returns whether the cache changed.
pub fn refresh(dir: &Path) -> bool {
    let key = file_key(dir);
    if let Ok(cache) = CACHE.lock() {
        if cache.dir == dir && cache.key.as_ref() == Some(&key) {
            return false;
        }
    }
    let value = match load_browser_annotations(dir) {
        Ok(annotations) => BrowserAnnotationsSnapshot {
            annotations,
            load_error: None,
        },
        Err(error) => BrowserAnnotationsSnapshot {
            annotations: Vec::new(),
            load_error: Some(error.to_string()),
        },
    };
    let Ok(mut cache) = CACHE.lock() else {
        return false;
    };
    cache.dir = dir.to_path_buf();
    cache.key = Some(key);
    cache.value = value;
    cache.revision = cache.revision.wrapping_add(1);
    true
}

pub fn get(dir: &str) -> (BrowserAnnotationsSnapshot, u64) {
    let Ok(cache) = CACHE.lock() else {
        return (BrowserAnnotationsSnapshot::default(), 0);
    };
    if cache.dir == Path::new(dir) {
        (cache.value.clone(), cache.revision)
    } else {
        (BrowserAnnotationsSnapshot::default(), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn snapshot_wire_fixture_and_load_error_are_distinct_from_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(refresh(dir.path()));
        let (missing, first) = get(dir.path().to_str().unwrap());
        assert_eq!(
            serde_json::to_value(missing).unwrap(),
            json!({"annotations": [], "load_error": null})
        );
        assert!(!refresh(dir.path()));
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
        assert!(refresh(dir.path()));
        let (loaded, second) = get(dir.path().to_str().unwrap());
        assert_ne!(first, second);
        assert_eq!(serde_json::to_value(loaded).unwrap(), fixture);
        std::fs::write(dir.path().join(BROWSER_ANNOTATIONS_FILE), b"broken").unwrap();
        assert!(refresh(dir.path()));
        let (failed, third) = get(dir.path().to_str().unwrap());
        assert_ne!(second, third);
        assert!(failed.annotations.is_empty());
        assert!(failed.load_error.is_some());
    }
}
