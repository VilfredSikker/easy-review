//! One atomic file write: a unique temporary, then a rename.
//!
//! Two writers of the same path are normal — a cancel and a run's supervisor,
//! two settings commands — so the temporary name carries the pid and a
//! process-wide counter. A shared fixed name would let one rename consume the
//! file the other still means to rename. A failed rename removes the
//! temporary it named.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Write `bytes` to `path` by renaming a fully written temporary file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = unique_tmp_path(path);
    std::fs::write(&tmp, bytes)?;
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

/// The temporary path a write uses: `<file>.tmp.<pid>.<counter>`.
///
/// Shared with the store's durable writer so the whole workspace builds the
/// same shape.
pub(crate) fn unique_tmp_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nth = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");
    path.with_file_name(format!("{name}.tmp.{}.{nth}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two writers of one file must both succeed: concurrent saves of the
    /// same artifact are normal.
    #[test]
    fn concurrent_writes_to_one_path_both_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        for _ in 0..64 {
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let handles: Vec<_> = (0..2)
                .map(|writer| {
                    let path = path.clone();
                    let barrier = std::sync::Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        barrier.wait();
                        write_atomic(&path, format!("writer-{writer}").as_bytes())
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap().unwrap();
            }
            let contents = std::fs::read_to_string(&path).unwrap();
            assert!(
                contents == "writer-0" || contents == "writer-1",
                "{contents}"
            );
        }
        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn a_write_replaces_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("answer.txt");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
    }
}
