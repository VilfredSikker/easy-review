//! Complete document reads, for the document and image previews. A capture
//! owns its inputs, so the reader releases whatever lock it read them under
//! before it touches the disk, git or GitHub.

use std::io::Read;
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::{bail, Context, Result};

use crate::git::FileStatus;
use crate::preview_key::{
    file_preview_key, image_mime, preview_context_key, supports_preview, PreviewKeyInputs,
};
use crate::tab_identity::DiffMode;

pub const MAX_PREVIEW_BYTES: usize = 1024 * 1024;
/// Screenshots and diagrams routinely pass the 1 MiB text cap.
pub const MAX_IMAGE_PREVIEW_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) enum Source {
    Diff(String, bool),
    Blob(String, String),
    Checkout(String, Option<std::time::SystemTime>),
    Remote(String, String, bool, Option<String>),
}

#[derive(Clone)]
pub struct FilePreviewRequest {
    pub path: String,
    pub preview_context_key: String,
    pub preview_key: String,
    pub(crate) source: Source,
    pub(crate) github: Arc<dyn er_github::GitHub>,
}

impl std::fmt::Debug for FilePreviewRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePreviewRequest")
            .field("path", &self.path)
            .field("preview_context_key", &self.preview_context_key)
            .field("preview_key", &self.preview_key)
            .finish_non_exhaustive()
    }
}

/// What a capture reads: the view's preview keys, and the facts about the
/// tab that pick where a document is read from. The App's tab and the
/// workspace's content each build one, so both read a document by one rule.
pub struct PreviewSource<'a> {
    pub keys: PreviewKeyInputs<'a>,
    /// A PR reviewed without a clone, which has no checkout to read.
    pub is_remote: bool,
    pub pr_number: Option<u64>,
    /// A read-only view of a branch.
    pub branch_view: bool,
    /// A tour entered from the PR diff.
    pub tour_is_pr: bool,
    pub github: Arc<dyn er_github::GitHub>,
}

/// Where `path`'s text is read from in the view `src` shows. `status` is
/// the file's status in the view, none when the view does not list it.
pub fn capture_file_preview(
    src: &PreviewSource<'_>,
    path: &str,
    status: Option<FileStatus>,
) -> Result<FilePreviewRequest> {
    if !supports_preview(path) {
        bail!("This file does not support document preview");
    }
    capture(src, path, status).map(|(request, _)| request)
}

/// Both sides of an image change. A modified image's earlier side is read
/// from the old blob, which a remote review cannot reach locally.
pub fn capture_image_preview(
    src: &PreviewSource<'_>,
    path: &str,
    status: Option<FileStatus>,
) -> Result<ImagePreviewRequest> {
    if image_mime(path).is_none() {
        bail!("This file is not an image");
    }
    let (request, status) = capture(src, path, status)?;
    if status == FileStatus::Deleted {
        return Ok(ImagePreviewRequest {
            before: Some(request),
            after: None,
        });
    }
    let keys = &src.keys;
    let before = if status == FileStatus::Added || src.is_remote {
        None
    } else if keys.mode == DiffMode::History {
        let commit = keys.history_commit.context("Commit is unavailable")?;
        let old_path = match &status {
            FileStatus::Renamed(old) => old.as_str(),
            _ => path,
        };
        Some(format!("{commit}^1:{old_path}"))
    } else {
        keys.section(path)
            .and_then(|section| index_oids(section).map(|(old, _)| old.to_string()))
            .filter(|oid| valid_oid(oid))
    }
    .map(|spec| FilePreviewRequest {
        source: Source::Blob(keys.repo_root.to_string(), spec),
        ..request.clone()
    });
    Ok(ImagePreviewRequest {
        before,
        after: Some(request),
    })
}

fn capture(
    src: &PreviewSource<'_>,
    path: &str,
    status: Option<FileStatus>,
) -> Result<(FilePreviewRequest, FileStatus)> {
    validate_path(path)?;
    let status = status.context("File is no longer in this review")?;
    let keys = &src.keys;
    let root = || keys.repo_root.to_string();
    let checkout = || keys.checkout_root.unwrap_or(keys.repo_root).to_string();
    let deleted = status == FileStatus::Deleted;
    let source = if keys.mode == DiffMode::History {
        let commit = keys.history_commit.context("Commit is unavailable")?;
        if let Some(repo) = keys.remote_repo.filter(|_| src.is_remote) {
            Source::Remote(repo.to_string(), commit.to_string(), deleted, None)
        } else {
            let revision = if deleted {
                format!("{commit}^1")
            } else {
                commit.to_string()
            };
            Source::Blob(root(), format!("{revision}:{path}"))
        }
    } else {
        let section = keys
            .section(path)
            .context("Document source is unavailable")?;
        let blob = index_oids(section)
            .map(|(old, new)| if deleted { old } else { new })
            .filter(|oid| valid_oid(oid));
        // A binary image section carries no lines to rebuild the file from.
        if matches!(status, FileStatus::Added | FileStatus::Deleted)
            && (image_mime(path).is_none() || !is_binary(section))
        {
            Source::Diff(section.to_string(), deleted)
        } else if let (true, false, Some(oid)) = (deleted, src.is_remote, blob) {
            // The PR head's parent need not hold a file the PR deleted.
            Source::Blob(root(), oid.to_string())
        } else if let Some(repo) = keys.remote_repo.filter(|_| {
            src.is_remote
                || keys.mode == DiffMode::PrDiff
                || (keys.mode == DiffMode::Tour && src.tour_is_pr)
                || (src.pr_number.is_some() && src.branch_view && keys.checkout_root.is_none())
        }) {
            Source::Remote(
                repo.to_string(),
                keys.preview_head_oid
                    .map(str::to_string)
                    .context("PR commit is unavailable")?,
                deleted,
                blob.map(str::to_string),
            )
        } else if !deleted
            && (keys.mode == DiffMode::Unstaged
                || (keys.checkout_root.is_some()
                    && (keys.mode == DiffMode::Branch
                        || (keys.mode == DiffMode::Tour && !src.tour_is_pr))))
        {
            Source::Checkout(checkout(), keys.mtimes.get(path).copied())
        } else {
            match blob {
                Some(oid) => Source::Blob(root(), oid.to_string()),
                None => match keys.blob_ids.get(path) {
                    Some(oid) => Source::Blob(root(), oid.clone()),
                    None => Source::Checkout(checkout(), keys.mtimes.get(path).copied()),
                },
            }
        }
    };
    let request = FilePreviewRequest {
        path: path.to_string(),
        preview_context_key: preview_context_key(keys),
        preview_key: file_preview_key(keys, path),
        source,
        github: Arc::clone(&src.github),
    };
    Ok((request, status))
}

pub(crate) fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("Document path must stay inside the reviewed checkout");
    }
    Ok(())
}

/// The old and new blob ids from a diff section's `index` line.
pub(crate) fn index_oids(section: &str) -> Option<(&str, &str)> {
    section
        .lines()
        .find_map(|line| line.strip_prefix("index "))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|line| line.split_once(".."))
}

/// False for the all-zero id git writes on the missing side of an add or delete.
pub(crate) fn valid_oid(oid: &str) -> bool {
    !oid.is_empty() && oid.bytes().all(|b| b.is_ascii_hexdigit()) && !oid.bytes().all(|b| b == b'0')
}

pub(crate) fn is_binary(section: &str) -> bool {
    section
        .lines()
        .any(|line| line.starts_with("Binary files ") || line == "GIT binary patch")
}

pub(crate) fn too_large(limit: usize) -> anyhow::Error {
    anyhow::anyhow!(
        "File exceeds the {} MiB preview limit",
        limit / (1024 * 1024)
    )
}

#[cfg(unix)]
pub(crate) fn open_checkout_file(root: &str, path: &str) -> Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    validate_path(path)?;
    let mut directory = std::fs::File::open(Path::new(root).canonicalize()?)?;
    let components: Vec<_> = Path::new(path).components().collect();
    for (i, component) in components.iter().enumerate() {
        let name = std::ffi::CString::new(component.as_os_str().as_bytes())?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if i + 1 < components.len() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: directory owns a live descriptor and name is a valid CString.
        // openat returns a new descriptor. O_NOFOLLOW prevents symlink escapes.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: fd is a new successful openat descriptor with no other owner.
        directory = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    Ok(directory)
}

#[cfg(not(unix))]
pub(crate) fn open_checkout_file(root: &str, path: &str) -> Result<std::fs::File> {
    validate_path(path)?;
    let root = Path::new(root).canonicalize()?;
    let path = root.join(path).canonicalize()?;
    if !path.starts_with(root) {
        bail!("Document path escapes the reviewed checkout");
    }
    Ok(std::fs::File::open(path)?)
}

pub(crate) fn bounded_read(reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(too_large(limit));
    }
    Ok(bytes)
}

pub(crate) fn gh_read_error(error: crate::github::GhError, limit: usize) -> anyhow::Error {
    match error {
        crate::github::GhError::TooLarge { .. } => too_large(limit),
        error => anyhow::anyhow!("Document source could not be read: {error}"),
    }
}

pub(crate) fn reconstruct(section: &str, deleted: bool, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let prefix = if deleted { b'-' } else { b'+' };
    let mut in_hunk = false;
    for line in section.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if line.starts_with("\\ No newline at end of file") {
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
            }
        } else if line.as_bytes().first() == Some(&prefix) {
            bytes.extend_from_slice(&line.as_bytes()[1..]);
            if bytes.len() > limit {
                return Err(too_large(limit));
            }
        }
    }
    if is_binary(section) {
        bail!("Binary document cannot be previewed");
    }
    let expected = index_oids(section)
        .map(|(old, new)| if deleted { old } else { new })
        .filter(|oid| valid_oid(oid));
    if let Some(expected) = expected {
        verify_blob(&bytes, expected)?;
    } else if section.contains('\u{fffd}') {
        bail!("Document text encoding is unavailable");
    }
    Ok(bytes)
}

pub(crate) fn verify_blob(bytes: &[u8], expected: &str) -> Result<()> {
    use sha1::{Digest, Sha1};
    let header = format!("blob {}\0", bytes.len());
    // Raw diff index IDs are abbreviated even in SHA-256 repositories.
    // Short prefixes do not identify the hash algorithm, so check both.
    let sha1_matches = if expected.len() <= 40 {
        let mut hasher = Sha1::new();
        hasher.update(&header);
        hasher.update(bytes);
        format!("{:x}", hasher.finalize()).starts_with(expected)
    } else {
        false
    };
    let sha256_matches = if expected.len() <= 64 {
        let mut hasher = sha2::Sha256::new();
        hasher.update(&header);
        hasher.update(bytes);
        format!("{:x}", hasher.finalize()).starts_with(expected)
    } else {
        false
    };
    if expected.is_empty()
        || !expected.bytes().all(|b| b.is_ascii_hexdigit())
        || !(sha1_matches || sha256_matches)
    {
        bail!("Document does not match the retained diff. Refresh the review");
    }
    Ok(())
}

impl FilePreviewRequest {
    pub fn read(&self, git: &dyn er_git::Git) -> Result<String> {
        let bytes = self.read_bytes(git, MAX_PREVIEW_BYTES)?;
        if bytes.contains(&0) {
            bail!("Binary document cannot be previewed");
        }
        String::from_utf8(bytes).context("Document is not UTF-8 text")
    }

    pub fn read_bytes(&self, git: &dyn er_git::Git, limit: usize) -> Result<Vec<u8>> {
        Ok(match &self.source {
            Source::Diff(section, deleted) => reconstruct(section, *deleted, limit)?,
            Source::Blob(root, oid) => git.read_blob(root, oid, limit)?,
            Source::Checkout(root, expected_mtime) => {
                let file = open_checkout_file(root, &self.path)?;
                let before = file.metadata()?;
                if !before.is_file() {
                    bail!("Document is not a regular file");
                }
                if before.len() > limit as u64 {
                    return Err(too_large(limit));
                }
                if expected_mtime.is_some_and(|m| before.modified().ok() != Some(m)) {
                    bail!("Document changed. Refresh the diff before previewing");
                }
                let bytes = bounded_read(&file, limit)?;
                let after = file.metadata()?;
                if before.modified().ok() != after.modified().ok() || before.len() != after.len() {
                    bail!("Document changed while reading");
                }
                bytes
            }
            Source::Remote(repo, commit, deleted, expected_blob) => {
                if !commit.bytes().all(|b| b.is_ascii_hexdigit()) || commit.len() != 40 {
                    bail!("PR commit is unavailable");
                }
                let parent;
                let commit = if *deleted {
                    let bytes = self
                        .github
                        .api_get_bounded(
                            &format!("repos/{repo}/git/commits/{commit}"),
                            MAX_PREVIEW_BYTES,
                        )
                        .map_err(|error| gh_read_error(error, MAX_PREVIEW_BYTES))?;
                    let metadata: serde_json::Value = serde_json::from_slice(&bytes)?;
                    parent = metadata["parents"][0]["sha"]
                        .as_str()
                        .context("Commit parent is unavailable")?
                        .to_string();
                    &parent
                } else {
                    commit
                };
                let path = self
                    .path
                    .bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.') {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect::<String>();
                let bytes = self
                    .github
                    .api_get_bounded(&format!("repos/{repo}/contents/{path}?ref={commit}"), limit)
                    .map_err(|error| gh_read_error(error, limit))?;
                if let Some(expected) = expected_blob {
                    verify_blob(&bytes, expected)?;
                }
                bytes
            }
        })
    }
}

/// Both sides of an image change. Each is `None` when that side does not exist
/// or cannot be read locally.
#[derive(Debug, Clone)]
pub struct ImagePreviewRequest {
    pub before: Option<FilePreviewRequest>,
    pub after: Option<FilePreviewRequest>,
}
