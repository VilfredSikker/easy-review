use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use super::{validation::host_id, BrowserAnnotationsError, Result};

pub const MAX_PNG_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PNG_SIDE: u32 = 4096;
const MAX_DECODE_BYTES: usize = 128 * 1024 * 1024;
pub(super) const ASSETS_DIR: &str = "browser-assets";

fn invalid(reason: &str) -> BrowserAnnotationsError {
    BrowserAnnotationsError::InvalidAsset(reason.into())
}

pub(super) fn validate_png(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_PNG_BYTES || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(invalid("expected a PNG of at most 4 MiB"));
    }
    let decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: MAX_DECODE_BYTES,
        },
    );
    let mut reader = decoder
        .read_info()
        .map_err(|_| invalid("invalid PNG header"))?;
    let info = reader.info();
    if info.width == 0
        || info.height == 0
        || info.width > MAX_PNG_SIDE
        || info.height > MAX_PNG_SIDE
        || info.animation_control.is_some()
    {
        return Err(invalid("PNG dimensions exceed bounds or PNG is animated"));
    }
    let size = reader
        .output_buffer_size()
        .filter(|n| *n <= MAX_DECODE_BYTES)
        .ok_or_else(|| invalid("PNG decoding budget exceeded"))?;
    let mut output = vec![0; size];
    reader
        .next_frame(&mut output)
        .map_err(|_| invalid("corrupt PNG image data"))?;
    reader.finish().map_err(|_| invalid("corrupt PNG end"))?;
    Ok(())
}

pub(super) fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(invalid("symlinks are not allowed")),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn asset_path(dir: &Path, asset_id: &str) -> Result<PathBuf> {
    if !host_id(asset_id, "ba-asset-") {
        return Err(invalid("invalid asset ID"));
    }
    let root = dir.join(ASSETS_DIR);
    reject_symlink(&root)?;
    let canonical = root.canonicalize()?;
    if canonical.parent() != Some(dir) {
        return Err(invalid("asset directory escapes bucket"));
    }
    let path = canonical.join(format!("{asset_id}.png"));
    reject_symlink(&path)?;
    if path.exists() && path.canonicalize()?.parent() != Some(canonical.as_path()) {
        return Err(invalid("asset escapes directory"));
    }
    Ok(path)
}

pub(super) fn write_asset(dir: &Path, asset_id: &str, bytes: &[u8]) -> Result<PathBuf> {
    validate_png(bytes)?;
    reject_symlink(&dir.join(ASSETS_DIR))?;
    fs::create_dir_all(dir.join(ASSETS_DIR))?;
    let path = asset_path(dir, asset_id)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir.join(ASSETS_DIR))?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(&path)
        .map_err(|e| BrowserAnnotationsError::Io(e.error))?;
    Ok(path)
}

pub(super) fn read_asset(dir: &Path, asset_id: &str) -> Result<Vec<u8>> {
    let path = asset_path(dir, asset_id)?;
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("asset is not a file"));
    }
    let mut bytes = Vec::new();
    file.take((MAX_PNG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    validate_png(&bytes)?;
    Ok(bytes)
}
