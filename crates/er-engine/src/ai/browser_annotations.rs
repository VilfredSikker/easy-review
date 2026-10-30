//! Synchronous v2 persistence for a caller-supplied managed view bucket (ADR 0004).
//! Callers must perform this disk I/O outside the desktop App mutex (ADR 0015).
//! No runtime caller, URL matching policy, or v1 migration is installed here.

mod assets;
mod types;
mod validation;

use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub use assets::{MAX_PNG_BYTES, MAX_PNG_SIDE};
use sha2::{Digest, Sha256};
pub use types::*;
pub use validation::{MAX_DOM_BYTES, MAX_NOTE_BYTES};

pub const BROWSER_ANNOTATIONS_FILE: &str = "browser-annotations.json";
pub const BROWSER_ANNOTATIONS_VERSION: u32 = 2;
type Result<T> = std::result::Result<T, BrowserAnnotationsError>;

#[derive(Debug)]
pub enum BrowserAnnotationsError {
    Io(std::io::Error),
    Malformed(String),
    UnsupportedVersion(serde_json::Value),
    InvalidRecord { id: String, reason: String },
    InvalidAsset(String),
    NumberExhausted,
    LockPoisoned,
}
impl std::fmt::Display for BrowserAnnotationsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "browser annotation I/O: {e}"),
            Self::Malformed(reason) => write!(f, "malformed browser annotations: {reason}"),
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported browser annotations version: {v}")
            }
            Self::InvalidRecord { id, reason } => {
                write!(f, "invalid browser annotation {id}: {reason}")
            }
            Self::InvalidAsset(reason) => write!(f, "invalid browser asset: {reason}"),
            Self::NumberExhausted => f.write_str("browser annotation numbers exhausted"),
            Self::LockPoisoned => f.write_str("browser annotation bucket lock poisoned"),
        }
    }
}
impl std::error::Error for BrowserAnnotationsError {}
impl From<std::io::Error> for BrowserAnnotationsError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

fn canonical_bucket(dir: &Path) -> Result<PathBuf> {
    if dir.exists() {
        return Ok(dir.canonicalize()?);
    }
    let absolute = std::path::absolute(dir)?;
    let parent = absolute
        .parent()
        .ok_or_else(|| BrowserAnnotationsError::Malformed("invalid bucket".into()))?;
    let name = absolute
        .file_name()
        .ok_or_else(|| BrowserAnnotationsError::Malformed("invalid bucket".into()))?;
    Ok(canonical_bucket(parent)?.join(name))
}

fn in_bucket<T>(dir: &Path, operation: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    type Locks = HashMap<PathBuf, Weak<Mutex<()>>>;
    static LOCKS: OnceLock<Mutex<Locks>> = OnceLock::new();
    let dir = canonical_bucket(dir)?;
    let lock = {
        let mut locks = LOCKS
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| BrowserAnnotationsError::LockPoisoned)?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&dir).and_then(Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(Mutex::new(()));
            locks.insert(dir.clone(), Arc::downgrade(&lock));
            lock
        }
    };
    let _guard = lock
        .lock()
        .map_err(|_| BrowserAnnotationsError::LockPoisoned)?;
    operation(&dir)
}

fn empty_file() -> BrowserAnnotationsFile {
    BrowserAnnotationsFile {
        version: BROWSER_ANNOTATIONS_VERSION,
        next_number: 1,
        annotations: Vec::new(),
        extra: Extensions::new(),
    }
}

fn load_file(dir: &Path) -> Result<BrowserAnnotationsFile> {
    let path = dir.join(BROWSER_ANNOTATIONS_FILE);
    assets::reject_symlink(&path)?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(empty_file()),
        Err(e) => return Err(e.into()),
    };
    if !file.metadata()?.is_file() {
        return Err(BrowserAnnotationsError::Malformed(
            "metadata is not a file".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(validation::MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > validation::MAX_METADATA_BYTES {
        return Err(BrowserAnnotationsError::Malformed(
            "metadata exceeds size limit".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(malformed)?;
    let version = value
        .get("version")
        .ok_or_else(|| BrowserAnnotationsError::Malformed("missing version".into()))?;
    if version != &serde_json::json!(BROWSER_ANNOTATIONS_VERSION) {
        return Err(BrowserAnnotationsError::UnsupportedVersion(version.clone()));
    }
    // Parse the original bytes so duplicate known fields cannot be hidden by Value.
    let file: BrowserAnnotationsFile = serde_json::from_slice(&bytes).map_err(malformed)?;
    validation::document(&file)?;
    Ok(file)
}
fn malformed(error: serde_json::Error) -> BrowserAnnotationsError {
    BrowserAnnotationsError::Malformed(error.to_string())
}

fn write_file(dir: &Path, file: &BrowserAnnotationsFile) -> Result<()> {
    validation::document(file)?;
    let bytes = serde_json::to_vec_pretty(file).map_err(malformed)?;
    if bytes.len() as u64 > validation::MAX_METADATA_BYTES {
        return Err(BrowserAnnotationsError::Malformed(
            "metadata exceeds size limit".into(),
        ));
    }
    // Reject extension fields colliding with authoritative fields before touching disk.
    let _: BrowserAnnotationsFile = serde_json::from_slice(&bytes).map_err(malformed)?;
    assets::reject_symlink(&dir.join(BROWSER_ANNOTATIONS_FILE))?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    #[cfg(test)]
    tests::metadata_write_checkpoint(dir)?;
    temp.persist(dir.join(BROWSER_ANNOTATIONS_FILE))
        .map_err(|e| BrowserAnnotationsError::Io(e.error))?;
    Ok(())
}

fn host_hex() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let entropy = format!(
        "{:?}-{}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let digest = Sha256::digest(entropy.as_bytes());
    format!("{digest:x}").chars().take(32).collect()
}

pub fn load_browser_annotations(dir: &Path) -> Result<Vec<BrowserAnnotation>> {
    in_bucket(dir, |dir| Ok(load_file(dir)?.annotations))
}

/// Atomically writes a new asset before metadata. A retry of a committed key
/// returns its original record, regardless of the retry's payload or image bytes.
/// Serialization is process-wide per canonical bucket, including path aliases.
pub fn save_browser_annotation(
    dir: &Path,
    new: NewBrowserAnnotation,
    image: Option<&[u8]>,
) -> Result<BrowserAnnotation> {
    in_bucket(dir, |dir| {
        let mut file = load_file(dir)?;
        if let Some(existing) = file
            .annotations
            .iter()
            .find(|a| a.idempotency_key == new.idempotency_key)
        {
            return Ok(existing.clone());
        }
        let next = file
            .next_number
            .checked_add(1)
            .ok_or(BrowserAnnotationsError::NumberExhausted)?;
        let hex = host_hex();
        let asset_id = format!("ba-asset-{hex}");
        let evidence = match (new.image, image) {
            (NewImageEvidence::None, None) => ImageEvidence::None {
                extra: Extensions::new(),
            },
            (NewImageEvidence::Unavailable { reason }, None) => ImageEvidence::Unavailable {
                reason,
                extra: Extensions::new(),
            },
            (NewImageEvidence::Captured(capture), Some(_)) => ImageEvidence::Captured {
                asset_id: asset_id.clone(),
                capture,
                extra: Extensions::new(),
            },
            _ => {
                return Err(BrowserAnnotationsError::InvalidAsset(
                    "image evidence and bytes disagree".into(),
                ))
            }
        };
        let annotation = BrowserAnnotation {
            id: format!("ba-{hex}"),
            number: file.next_number,
            idempotency_key: new.idempotency_key,
            created_at: new.created_at,
            author: new.author,
            text: new.text,
            page: new.page,
            locator: new.locator,
            target: new.target,
            captured: new.captured,
            dom_context: new.dom_context,
            resolution: new.resolution,
            image: evidence,
            extra: Extensions::new(),
        };
        file.next_number = next;
        file.annotations.push(annotation.clone());
        validation::document(&file)?;
        fs::create_dir_all(dir)?;
        let asset = image
            .map(|bytes| assets::write_asset(dir, &asset_id, bytes))
            .transpose()?;
        if let Err(error) = write_file(dir, &file) {
            if let Some(path) = asset {
                fs::remove_file(path)?;
            }
            return Err(error);
        }
        Ok(annotation)
    })
}

/// Commits the removal before cleaning its asset. Missing assets do not prevent
/// deleting metadata; unsafe asset paths are rejected before any metadata changes.
pub fn delete_browser_annotation(dir: &Path, id: &str) -> Result<bool> {
    in_bucket(dir, |dir| {
        let mut file = load_file(dir)?;
        let Some(index) = file.annotations.iter().position(|a| a.id == id) else {
            return Ok(false);
        };
        let asset = if let ImageEvidence::Captured { asset_id, .. } = &file.annotations[index].image
        {
            match assets::asset_path(dir, asset_id) {
                Ok(path) => Some(path),
                Err(BrowserAnnotationsError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    None
                }
                Err(e) => return Err(e),
            }
        } else {
            None
        };
        file.annotations.remove(index);
        write_file(dir, &file)?;
        if let Some(path) = asset {
            match fs::remove_file(path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
        Ok(true)
    })
}

pub fn update_resolution(
    dir: &Path,
    id: &str,
    resolution: Resolution,
) -> Result<Option<BrowserAnnotation>> {
    in_bucket(dir, |dir| {
        let mut file = load_file(dir)?;
        let Some(annotation) = file.annotations.iter_mut().find(|a| a.id == id) else {
            return Ok(None);
        };
        let mut resolution = resolution;
        for (key, value) in &annotation.resolution.extra {
            resolution
                .extra
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        annotation.resolution = resolution;
        let result = annotation.clone();
        write_file(dir, &file)?;
        Ok(Some(result))
    })
}

/// Reads validated PNG bytes by a relative asset ID. A delete racing a separate
/// metadata read can produce NotFound; consumers must omit that incomplete record.
pub fn read_browser_asset(dir: &Path, asset_id: &str) -> Result<Vec<u8>> {
    in_bucket(dir, |dir| assets::read_asset(dir, asset_id))
}

#[cfg(test)]
mod tests;
