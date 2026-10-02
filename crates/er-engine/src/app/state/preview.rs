//! Complete document reads own their inputs so callers can release the app lock.
use super::{DiffMode, TabState};
use crate::git::FileStatus;
use anyhow::{bail, Context, Result};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Component, Path};
use std::process::Command;

pub const MAX_PREVIEW_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
enum Source {
    Diff(String, bool),
    Blob(String, String),
    Checkout(String, Option<std::time::SystemTime>),
    Remote(String, String, bool, Option<String>),
}

#[derive(Debug, Clone)]
pub struct FilePreviewRequest {
    pub path: String,
    pub preview_context_key: String,
    pub preview_key: String,
    source: Source,
}

fn fingerprint(value: impl Hash) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    format!("{:016x}", h.finish())
}

pub fn supports_preview(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "txt" | "text"
            )
        })
}

impl TabState {
    pub(super) fn load_diff_with_preview_head(
        &mut self,
        scope: &str,
        allow_preload: bool,
    ) -> Result<(String, Option<String>)> {
        self.load_diff_with_preview_head_using(scope, allow_preload, |tab, scope| {
            let head = tab.fetch_preview_head();
            let raw = tab.fetch_tab_raw_diff(scope)?;
            let confirmed_head = tab.fetch_preview_head();
            Ok((raw, if head == confirmed_head { head } else { None }))
        })
    }

    fn load_diff_with_preview_head_using(
        &mut self,
        scope: &str,
        allow_preload: bool,
        fetch: impl FnOnce(&Self, &str) -> Result<(String, Option<String>)>,
    ) -> Result<(String, Option<String>)> {
        if allow_preload {
            if let Some(raw) = self.take_preloaded_branch_raw() {
                return Ok((raw, self.preview_head_oid.clone()));
            }
        }
        fetch(self, scope)
    }

    fn fetch_preview_head(&self) -> Option<String> {
        if self.pr_number.is_some()
            && (self.is_remote()
                || self.mode == DiffMode::PrDiff
                || (self.mode == DiffMode::Tour && self.tour_is_pr)
                || (self.local_branch_view.is_some() && self.local_branch_checkout_root.is_none()))
        {
            self.remote_repo
                .as_deref()
                .and_then(|slug| slug.split_once('/'))
                .and_then(|(owner, repo)| {
                    crate::github::gh_pr_head_oid_remote(owner, repo, self.pr_number.unwrap_or(0))
                        .ok()
                })
        } else {
            None
        }
    }

    pub(super) fn capture_preview_rename_blobs(&mut self) {
        if self.is_remote() {
            return;
        }
        for file in &self.files {
            let FileStatus::Renamed(old_path) = &file.status else {
                continue;
            };
            if !supports_preview(&file.path) {
                continue;
            }
            let root = self
                .local_branch_checkout_root
                .as_deref()
                .unwrap_or(&self.repo_root);
            let spec = if self.mode == DiffMode::Staged && !self.committed_unpushed {
                format!("HEAD:{old_path}")
            } else {
                format!(
                    "{}:{}",
                    self.local_branch_view
                        .as_deref()
                        .or(self.pr_head_ref.as_deref())
                        .unwrap_or("HEAD"),
                    file.path
                )
            };
            if let Ok(oid) = crate::git::git_blob_oid(root, &spec) {
                self.preview_blob_ids.insert(file.path.clone(), oid);
            }
        }
    }

    pub fn preview_context_key(&self) -> String {
        let mut blobs: Vec<_> = if self.mode == DiffMode::History {
            Vec::new()
        } else {
            self.preview_blob_ids.iter().collect()
        };
        blobs.sort_by_key(|(path, _)| *path);
        let commit = self
            .history
            .as_ref()
            .and_then(|h| h.commits.get(h.selected_commit))
            .map(|c| &c.hash);
        fingerprint((
            &self.repo_root,
            &self.remote_repo,
            self.mode.git_mode(),
            &self.local_branch_checkout_root,
            &self.preview_head_oid,
            commit,
            if self.mode == DiffMode::History {
                None
            } else {
                self.raw_diff.as_ref()
            },
            if self.mode == DiffMode::History {
                ""
            } else {
                &self.diff_hash
            },
            blobs,
        ))
    }

    fn preview_section(&self, path: &str) -> Option<&str> {
        let raw = self.raw_diff.as_deref()?;
        let h = self.file_headers.iter().find(|h| h.path == path)?;
        raw.get(h.byte_offset..h.byte_offset + h.byte_length)
    }

    pub fn file_preview_key(&self, path: &str) -> String {
        if !supports_preview(path) {
            return String::new();
        }
        let commit = self
            .history
            .as_ref()
            .and_then(|h| h.commits.get(h.selected_commit))
            .map(|c| &c.hash);
        fingerprint((
            &self.repo_root,
            &self.remote_repo,
            self.mode.git_mode(),
            &self.preview_head_oid,
            &self.local_branch_checkout_root,
            commit,
            path,
            if self.mode == DiffMode::History {
                None
            } else {
                self.preview_blob_ids.get(path)
            },
            if self.mode == DiffMode::History {
                None
            } else {
                self.preview_section(path)
            },
            if self.mode == DiffMode::History {
                None
            } else {
                self.mtime_cache.get(path)
            },
        ))
    }

    pub fn capture_file_preview(&self, path: &str) -> Result<FilePreviewRequest> {
        if !supports_preview(path) {
            bail!("This file does not support document preview");
        }
        validate_path(path)?;
        let file = self
            .visible_files()
            .into_iter()
            .find(|(_, f)| f.path == path)
            .map(|(_, f)| f)
            .context("File is no longer in this review")?;
        let deleted = file.status == FileStatus::Deleted;
        let source = if self.mode == DiffMode::History {
            let h = self.history.as_ref().context("History is unavailable")?;
            let commit = &h
                .commits
                .get(h.selected_commit)
                .context("Commit is unavailable")?
                .hash;
            if let Some(repo) = self.remote_repo.as_ref().filter(|_| self.is_remote()) {
                Source::Remote(repo.clone(), commit.clone(), deleted, None)
            } else {
                Source::Blob(
                    self.repo_root.clone(),
                    format!(
                        "{}:{}",
                        if deleted {
                            format!("{commit}^1")
                        } else {
                            commit.clone()
                        },
                        path
                    ),
                )
            }
        } else {
            let section = self
                .preview_section(path)
                .context("Document source is unavailable")?;
            if matches!(file.status, FileStatus::Added | FileStatus::Deleted) {
                Source::Diff(section.to_string(), deleted)
            } else if let Some(repo) = self.remote_repo.as_ref().filter(|_| {
                self.is_remote()
                    || self.mode == DiffMode::PrDiff
                    || (self.mode == DiffMode::Tour && self.tour_is_pr)
                    || (self.pr_number.is_some()
                        && self.local_branch_view.is_some()
                        && self.local_branch_checkout_root.is_none())
            }) {
                Source::Remote(
                    repo.clone(),
                    self.preview_head_oid
                        .clone()
                        .context("PR commit is unavailable")?,
                    false,
                    section
                        .lines()
                        .find_map(|line| line.strip_prefix("index "))
                        .and_then(|line| line.split_whitespace().next())
                        .and_then(|line| line.split_once(".."))
                        .map(|(_, new)| new.to_string()),
                )
            } else {
                if self.mode == DiffMode::Unstaged
                    || (self.local_branch_checkout_root.is_some()
                        && (self.mode == DiffMode::Branch
                            || (self.mode == DiffMode::Tour && !self.tour_is_pr)))
                {
                    return Ok(FilePreviewRequest {
                        path: path.to_string(),
                        preview_context_key: self.preview_context_key(),
                        preview_key: self.file_preview_key(path),
                        source: Source::Checkout(
                            self.local_branch_checkout_root
                                .clone()
                                .unwrap_or_else(|| self.repo_root.clone()),
                            self.mtime_cache.get(path).copied(),
                        ),
                    });
                }
                let oid = section
                    .lines()
                    .find_map(|l| l.strip_prefix("index "))
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| s.split_once(".."))
                    .map(|(old, new)| if deleted { old } else { new });
                match oid.filter(|s| {
                    !s.is_empty()
                        && s.bytes().all(|b| b.is_ascii_hexdigit())
                        && !s.bytes().all(|b| b == b'0')
                }) {
                    Some(oid) => Source::Blob(self.repo_root.clone(), oid.to_string()),
                    None if self.preview_blob_ids.contains_key(path) => {
                        Source::Blob(self.repo_root.clone(), self.preview_blob_ids[path].clone())
                    }
                    None => Source::Checkout(
                        self.local_branch_checkout_root
                            .clone()
                            .unwrap_or_else(|| self.repo_root.clone()),
                        self.mtime_cache.get(path).copied(),
                    ),
                }
            }
        };
        Ok(FilePreviewRequest {
            path: path.to_string(),
            preview_context_key: self.preview_context_key(),
            preview_key: self.file_preview_key(path),
            source,
        })
    }
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("Document path must stay inside the reviewed checkout");
    }
    Ok(())
}

#[cfg(unix)]
fn open_checkout_file(root: &str, path: &str) -> Result<std::fs::File> {
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
fn open_checkout_file(root: &str, path: &str) -> Result<std::fs::File> {
    validate_path(path)?;
    let root = Path::new(root).canonicalize()?;
    let path = root.join(path).canonicalize()?;
    if !path.starts_with(root) {
        bail!("Document path escapes the reviewed checkout");
    }
    Ok(std::fs::File::open(path)?)
}

fn bounded_read(reader: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PREVIEW_BYTES {
        bail!("Document exceeds the 1 MiB preview limit");
    }
    Ok(bytes)
}

fn read_command(command: &mut Command) -> Result<Vec<u8>> {
    let output =
        crate::proc::run_with_bounded_stdout(command, crate::proc::GH_TIMEOUT, MAX_PREVIEW_BYTES)?;
    if !output.status.success() {
        bail!("Document source could not be read");
    }
    Ok(output.stdout)
}

fn reconstruct(section: &str, deleted: bool) -> Result<Vec<u8>> {
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
            if bytes.len() > MAX_PREVIEW_BYTES {
                bail!("Document exceeds the 1 MiB preview limit");
            }
        }
    }
    if section
        .lines()
        .any(|line| line.starts_with("Binary files ") || line == "GIT binary patch")
    {
        bail!("Binary document cannot be previewed");
    }
    let expected = section
        .lines()
        .find_map(|line| line.strip_prefix("index "))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|line| line.split_once(".."))
        .map(|(old, new)| if deleted { old } else { new })
        .filter(|oid| {
            oid.bytes().all(|b| b.is_ascii_hexdigit()) && !oid.bytes().all(|b| b == b'0')
        });
    if let Some(expected) = expected {
        verify_blob(&bytes, expected)?;
    } else if section.contains('\u{fffd}') {
        bail!("Document text encoding is unavailable");
    }
    Ok(bytes)
}

fn verify_blob(bytes: &[u8], expected: &str) -> Result<()> {
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
    pub fn read(&self) -> Result<String> {
        let bytes = match &self.source {
            Source::Diff(section, deleted) => reconstruct(section, *deleted)?,
            Source::Blob(root, oid) => crate::git::git_read_blob(root, oid, MAX_PREVIEW_BYTES)?,
            Source::Checkout(root, expected_mtime) => {
                let file = open_checkout_file(root, &self.path)?;
                let before = file.metadata()?;
                if !before.is_file() {
                    bail!("Document is not a regular file");
                }
                if before.len() > MAX_PREVIEW_BYTES as u64 {
                    bail!("Document exceeds the 1 MiB preview limit");
                }
                if expected_mtime.is_some_and(|m| before.modified().ok() != Some(m)) {
                    bail!("Document changed. Refresh the diff before previewing");
                }
                let bytes = bounded_read(&file)?;
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
                    let bytes = read_command(
                        Command::new("gh")
                            .args(["api", &format!("repos/{repo}/git/commits/{commit}")]),
                    )?;
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
                let bytes = read_command(Command::new("gh").args([
                    "api",
                    "-H",
                    "Accept: application/vnd.github.raw+json",
                    &format!("repos/{repo}/contents/{path}?ref={commit}"),
                ]))?;
                if let Some(expected) = expected_blob {
                    verify_blob(&bytes, expected)?;
                }
                bytes
            }
        };
        if bytes.contains(&0) {
            bail!("Binary document cannot be previewed");
        }
        String::from_utf8(bytes).context("Document is not UTF-8 text")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;

    fn tab(raw: &str, root: &Path, mode: DiffMode) -> TabState {
        let mut tab = super::super::tests::make_test_tab(git::parse_diff(raw));
        tab.repo_root = root.to_string_lossy().into_owned();
        tab.mode = mode;
        tab.raw_diff = Some(raw.to_string());
        tab.file_headers = git::parse_diff_headers(raw);
        assert!(!tab.files.is_empty(), "No files parsed from {raw:?}");
        tab.refresh_mtime_cache();
        tab
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args([
                "-c",
                "diff.noprefix=false",
                "-c",
                "diff.mnemonicPrefix=false",
                "-c",
                "color.ui=false",
            ])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout).unwrap();
        if args.first() == Some(&"diff") {
            text
        } else {
            text.trim_end().to_string()
        }
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        git(
            dir.path(),
            &["config", "user.email", "preview@example.invalid"],
        );
        git(dir.path(), &["config", "user.name", "Preview Tests"]);
        std::fs::write(dir.path().join("readme.md"), "old\nunchanged\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        dir
    }

    #[test]
    fn preview_reconstructs_additions_deletions_crlf_and_missing_newline() {
        assert_eq!(
            reconstruct(
                "@@ -0,0 +1,2 @@\n+one\r\n+two\n\\ No newline at end of file\n",
                false
            )
            .unwrap(),
            b"one\r\ntwo"
        );
        assert_eq!(
            reconstruct(
                "@@ -1,2 +0,0 @@\n-one\r\n-two\n\\ No newline at end of file\n",
                true
            )
            .unwrap(),
            b"one\r\ntwo"
        );
        assert_eq!(reconstruct("new file mode 100644\n", false).unwrap(), b"");
        assert!(reconstruct("Binary files a/a.md and b/a.md differ\n", false).is_err());
        assert!(reconstruct("@@ -0,0 +1 @@\n+invalid \u{fffd}\n", false).is_err());
    }

    #[test]
    fn preview_sha256_diff_reconstructs_additions_and_deletions() {
        let dir = tempfile::tempdir().unwrap();
        git(
            dir.path(),
            &["init", "-b", "main", "--object-format=sha256"],
        );
        git(
            dir.path(),
            &["config", "user.email", "preview@example.invalid"],
        );
        git(dir.path(), &["config", "user.name", "Preview Tests"]);
        git(dir.path(), &["config", "core.abbrev", "7"]);
        std::fs::write(dir.path().join("deleted.md"), "old\r\nlast").unwrap();
        git(dir.path(), &["add", "."]);
        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        std::fs::remove_file(dir.path().join("deleted.md")).unwrap();
        std::fs::write(dir.path().join("added.md"), "new\r\nlast").unwrap();
        git(dir.path(), &["add", "-A"]);
        let raw = git(
            dir.path(),
            &["diff", "--cached", "--no-ext-diff", "--no-color"],
        );
        let t = tab(&raw, dir.path(), DiffMode::Staged);
        assert_eq!(
            t.capture_file_preview("added.md").unwrap().read().unwrap(),
            "new\r\nlast"
        );
        assert_eq!(
            t.capture_file_preview("deleted.md")
                .unwrap()
                .read()
                .unwrap(),
            "old\r\nlast"
        );
        let changed = raw.replace("+new\r\n", "+wrong\r\n");
        assert_ne!(changed, raw);
        let t = tab(&changed, dir.path(), DiffMode::Staged);
        assert!(t.capture_file_preview("added.md").unwrap().read().is_err());
        let changed = raw.replace("-old\r\n", "-wrong\r\n");
        let t = tab(&changed, dir.path(), DiffMode::Staged);
        assert!(t
            .capture_file_preview("deleted.md")
            .unwrap()
            .read()
            .is_err());
        let oid = git(dir.path(), &["rev-parse", ":added.md"]);
        assert_eq!(oid.len(), 64);
        assert!(verify_blob(b"new\r\nlast", &oid).is_ok());
        let long_prefix: String = oid.chars().take(40).collect();
        assert!(verify_blob(b"new\r\nlast", &long_prefix).is_ok());
        assert!(verify_blob(b"wrong", &oid).is_err());
    }

    #[test]
    fn preview_diff_blob_validation_accepts_valid_replacement_and_rejects_lossy_text() {
        let dir = repo();
        let path = dir.path().join("replacement.md");
        std::fs::write(&path, "valid \u{fffd}\n").unwrap();
        git(dir.path(), &["add", "."]);
        let raw = git(dir.path(), &["diff", "--cached"]);
        let t = tab(&raw, dir.path(), DiffMode::Staged);
        assert_eq!(
            t.capture_file_preview("replacement.md")
                .unwrap()
                .read()
                .unwrap(),
            "valid \u{fffd}\n"
        );
        std::fs::write(&path, b"invalid \xff\n").unwrap();
        git(dir.path(), &["add", "."]);
        let out = Command::new("git")
            .current_dir(dir.path())
            .args(["diff", "--cached", "--no-ext-diff", "--no-color"])
            .output()
            .unwrap();
        let raw = String::from_utf8_lossy(&out.stdout);
        let t = tab(&raw, dir.path(), DiffMode::Staged);
        assert!(t
            .capture_file_preview("replacement.md")
            .unwrap()
            .read()
            .is_err());
    }

    #[test]
    fn preview_staged_branch_and_working_tree_use_distinct_complete_sources() {
        let dir = repo();
        std::fs::write(dir.path().join("readme.md"), "staged\nunchanged\n").unwrap();
        git(dir.path(), &["add", "."]);
        let staged = git(dir.path(), &["diff", "--cached"]);
        let mut t = tab(&staged, dir.path(), DiffMode::Staged);
        t.remote_repo = Some("owner/repo".into());
        t.local_branch_view = Some("feature".into());
        t.local_branch_checkout_root = Some(dir.path().to_string_lossy().into_owned());
        std::fs::write(dir.path().join("readme.md"), "working\nunchanged\n").unwrap();
        assert_eq!(
            t.capture_file_preview("readme.md").unwrap().read().unwrap(),
            "staged\nunchanged\n"
        );
        let raw = git(dir.path(), &["diff"]);
        let mut t = tab(&raw, dir.path(), DiffMode::Unstaged);
        t.remote_repo = Some("owner/repo".into());
        t.local_branch_view = Some("feature".into());
        assert_eq!(
            t.capture_file_preview("readme.md").unwrap().read().unwrap(),
            "working\nunchanged\n"
        );
        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "staged"],
        );
        let raw = git(dir.path(), &["diff", "HEAD~1", "HEAD"]);
        let t = tab(&raw, dir.path(), DiffMode::Branch);
        assert_eq!(
            t.capture_file_preview("readme.md").unwrap().read().unwrap(),
            "staged\nunchanged\n"
        );
    }

    #[test]
    fn preview_rejects_bad_text_limits_and_checkout_escape() {
        let dir = repo();
        let raw = "diff --git a/readme.md b/readme.md\nindex abcdef0..0000000 100644\n--- a/readme.md\n+++ b/readme.md\n@@ -1 +1 @@\n-old\n+new\n";
        for bytes in [vec![0], vec![0xff], vec![b'a'; MAX_PREVIEW_BYTES + 1]] {
            std::fs::write(dir.path().join("readme.md"), bytes).unwrap();
            let t = tab(raw, dir.path(), DiffMode::Unstaged);
            assert!(t.capture_file_preview("readme.md").unwrap().read().is_err());
        }
        assert!(validate_path("../readme.md").is_err());
        assert!(validate_path("/readme.md").is_err());
        #[cfg(unix)]
        {
            let outside = tempfile::NamedTempFile::new().unwrap();
            std::fs::remove_file(dir.path().join("readme.md")).unwrap();
            std::os::unix::fs::symlink(outside.path(), dir.path().join("readme.md")).unwrap();
            let t = tab(raw, dir.path(), DiffMode::Unstaged);
            assert!(t.capture_file_preview("readme.md").unwrap().read().is_err());
        }
    }

    #[test]
    fn preview_keys_include_blob_metadata_and_checkout_and_reject_changed_reads() {
        let dir = repo();
        let raw = "diff --git a/readme.md b/readme.md\nindex abcdef0..0000000 100644\n--- a/readme.md\n+++ b/readme.md\n@@ -1 +1 @@\n-old\n+new\n";
        let mut t = tab(raw, dir.path(), DiffMode::Unstaged);
        let key = t.file_preview_key("readme.md");
        let context = t.preview_context_key();
        let request = t.capture_file_preview("readme.md").unwrap();
        std::fs::write(
            dir.path().join("readme.md"),
            "different complete document\n",
        )
        .unwrap();
        assert!(request.read().is_err());
        t.raw_diff = Some(raw.replace("abcdef0", "1234567"));
        assert_ne!(context, t.preview_context_key());
        assert_ne!(key, t.file_preview_key("readme.md"));
        let key = t.file_preview_key("readme.md");
        t.local_branch_checkout_root = Some("/different/checkout".into());
        assert_ne!(key, t.file_preview_key("readme.md"));
    }

    #[test]
    fn preview_extensions_are_case_insensitive_and_pr_reads_are_immutable() {
        for path in [
            "A.MD",
            "a.markdown",
            "a.mdown",
            "a.mkd",
            "a.mkdn",
            "a.TXT",
            "a.text",
        ] {
            assert!(supports_preview(path));
        }
        assert!(!supports_preview("a.html"));
        let dir = repo();
        let raw = "diff --git a/readme.md b/readme.md\nindex abcdef0..1234567 100644\n--- a/readme.md\n+++ b/readme.md\n@@ -1 +1 @@\n-old\n+new\n";
        let mut t = tab(raw, dir.path(), DiffMode::PrDiff);
        t.remote_repo = Some("owner/repo".into());
        t.preview_head_oid = Some("a".repeat(40));
        let request = t.capture_file_preview("readme.md").unwrap();
        assert!(
            matches!(request.source, Source::Remote(_, ref sha, false, _) if sha == &"a".repeat(40))
        );
        t.preview_head_oid = Some("b".repeat(40));
        assert_ne!(request.preview_context_key, t.preview_context_key());
    }

    #[test]
    fn preview_history_reads_complete_commit_content_after_cache_reuse() {
        use super::super::{DiffCache, HistoryState};
        let dir = repo();
        std::fs::write(
            dir.path().join("readme.md"),
            "new\r\ncomplete\r\nwithout final newline",
        )
        .unwrap();
        git(dir.path(), &["add", "."]);
        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "changed"],
        );
        let hash = git(dir.path(), &["rev-parse", "HEAD"]);
        let raw = git(dir.path(), &["diff", "HEAD~1", "HEAD"]);
        let mut t = tab(&raw, dir.path(), DiffMode::History);
        let files = t.files.clone();
        let mut cache = DiffCache::new(5);
        cache.insert(hash.clone(), files.clone());
        let cached = cache.get(&hash).unwrap().clone();
        t.history = Some(HistoryState {
            commits: vec![git::CommitInfo {
                hash: hash.clone(),
                short_hash: hash.chars().take(7).collect(),
                subject: String::new(),
                author: String::new(),
                date: String::new(),
                relative_date: String::new(),
                file_count: 1,
                adds: 1,
                dels: 1,
                is_merge: false,
            }],
            selected_commit: 0,
            commit_files: cached,
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            diff_scroll: 0,
            h_scroll: 0,
            all_loaded: true,
            diff_cache: cache,
        });
        std::fs::write(dir.path().join("readme.md"), "unrelated live edit").unwrap();
        let request = t.capture_file_preview("readme.md").unwrap();
        assert_eq!(
            request.read().unwrap(),
            "new\r\ncomplete\r\nwithout final newline"
        );
        let context = t.preview_context_key();
        t.raw_diff = Some("unrelated branch diff".into());
        t.diff_hash = "unrelated hash".into();
        assert_eq!(context, t.preview_context_key());
        std::fs::remove_file(dir.path().join("readme.md")).unwrap();
        git(dir.path(), &["add", "."]);
        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "deleted"],
        );
        let deleted_hash = git(dir.path(), &["rev-parse", "HEAD"]);
        let raw = git(dir.path(), &["diff", "HEAD~1", "HEAD"]);
        let h = t.history.as_mut().unwrap();
        h.commits[0].hash = deleted_hash;
        h.commit_files = git::parse_diff(&raw);
        assert_eq!(
            t.capture_file_preview("readme.md").unwrap().read().unwrap(),
            "new\r\ncomplete\r\nwithout final newline"
        );
    }

    #[test]
    fn preview_rename_reads_resulting_path_and_preload_retains_head() {
        let dir = repo();
        git(dir.path(), &["mv", "readme.md", "renamed.md"]);
        let raw = git(dir.path(), &["diff", "--cached", "--find-renames"]);
        let mut t = tab(&raw, dir.path(), DiffMode::Staged);
        std::fs::write(dir.path().join("renamed.md"), "unrelated live edit").unwrap();
        assert_eq!(
            t.capture_file_preview("renamed.md")
                .unwrap()
                .read()
                .unwrap(),
            "old\nunchanged\n"
        );
        let head = "a".repeat(40);
        t.preloaded_branch_raw = Some(super::super::preload::PreloadedBranchRaw {
            raw: raw.clone(),
            preview_head_oid: Some(head.clone()),
            base_branch: t.base_branch.clone(),
            pr_number: t.pr_number,
            local_branch_view: t.local_branch_view.clone(),
            checkout_root: t.local_branch_checkout_root.clone(),
            remote_repo: t.remote_repo.clone(),
            pr_head_ref: t.pr_head_ref.clone(),
            parity: true,
        });
        assert_eq!(t.take_preloaded_branch_raw(), Some(raw));
        assert_eq!(t.preview_head_oid, Some(head));
    }

    #[test]
    fn preview_fresh_fetch_replaces_rejected_or_ignored_preload_source() {
        let dir = repo();
        let raw = "diff --git a/readme.md b/readme.md\nindex abcdef0..1234567 100644\n--- a/readme.md\n+++ b/readme.md\n@@ -1 +1 @@\n-old\n+new\n";
        for allow_preload in [false, true] {
            let mut t = tab(raw, dir.path(), DiffMode::PrDiff);
            t.preview_head_oid = Some("old-head".into());
            t.preloaded_branch_raw = Some(super::super::preload::PreloadedBranchRaw {
                raw: "stale preload".into(),
                preview_head_oid: Some("preloaded-head".into()),
                base_branch: "different-base".into(),
                pr_number: t.pr_number,
                local_branch_view: t.local_branch_view.clone(),
                checkout_root: None,
                remote_repo: None,
                pr_head_ref: None,
                parity: true,
            });
            let (fresh_raw, head) = t
                .load_diff_with_preview_head_using("branch", allow_preload, |_, _| {
                    Ok(("fresh fetched diff".into(), Some("fresh-head".into())))
                })
                .unwrap();
            assert_eq!(fresh_raw, "fresh fetched diff");
            assert_eq!(head.as_deref(), Some("fresh-head"));
            assert_eq!(t.preloaded_branch_raw.is_some(), !allow_preload);
            // A failed head lookup is represented by None, clearing the old
            // source when the caller installs the newly-fetched diff.
            let (_, head) = t
                .load_diff_with_preview_head_using("branch", false, |_, _| {
                    Ok(("new diff without confirmed head".into(), None))
                })
                .unwrap();
            assert!(head.is_none());
        }
    }
}
