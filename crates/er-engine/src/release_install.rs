//! Install a downloaded `er` binary in place of the running one.
//!
//! The update *record* lives in the store (see [`crate::release_update`]); this
//! module is only the file swap, which is why it is not on the sidecar guard
//! list.

use anyhow::{anyhow, Context, Result};
use std::path::Path;

/// Atomically replace `target` with `new_bin`. The copy is staged next to the
/// target so the final rename never crosses a filesystem; a running process
/// keeps its old inode and picks up the new binary on its next launch.
pub fn replace_executable(target: &Path, new_bin: &Path) -> Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", target.display()))?;
    let staged = tempfile::Builder::new()
        .prefix(".er-update-")
        .tempfile_in(dir)
        .with_context(|| format!("cannot write to {}", dir.display()))?;
    std::fs::copy(new_bin, staged.path())
        .with_context(|| format!("copy {} into {}", new_bin.display(), dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staged.path(), std::fs::Permissions::from_mode(0o755))?;
    }
    // Flushed before the rename, or a crash right after it can leave an empty `er`.
    staged.as_file().sync_all()?;
    staged
        .persist(target)
        .map_err(|e| anyhow!("replace {}: {}", target.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_swaps_contents_and_marks_executable() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("er");
        std::fs::write(&target, b"old").unwrap();
        let new_bin = dir.path().join("downloaded");
        std::fs::write(&new_bin, b"new").unwrap();

        replace_executable(&target, &new_bin).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".er-update-"))
            .collect();
        assert!(leftovers.is_empty(), "staged temp file left behind");
    }

    #[cfg(unix)]
    #[test]
    fn replace_into_read_only_dir_fails_and_keeps_old_binary() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();
        let target = bin_dir.join("er");
        std::fs::write(&target, b"old").unwrap();
        let new_bin = dir.path().join("downloaded");
        std::fs::write(&new_bin, b"new").unwrap();
        std::fs::set_permissions(&bin_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let err = replace_executable(&target, &new_bin).unwrap_err();

        std::fs::set_permissions(&bin_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(err.to_string().contains("cannot write to"), "{err}");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }
}
