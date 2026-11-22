//! Complete document reads own their inputs so callers can release the app lock.
use super::{DiffMode, TabState};
use crate::git::FileStatus;
use crate::preview_read::{self, PreviewSource};
use anyhow::{bail, Context, Result};

use crate::preview_key::PreviewKeyInputs;
pub use crate::preview_key::{image_mime, supports_preview};
pub use crate::preview_read::{
    FilePreviewRequest, ImagePreviewRequest, MAX_IMAGE_PREVIEW_BYTES, MAX_PREVIEW_BYTES,
};

impl TabState {
    pub(super) fn load_diff_with_preview_head(
        &mut self,
        scope: &str,
        allow_preload: bool,
    ) -> Result<(String, Option<String>)> {
        self.load_diff_with_preview_head_using(scope, allow_preload, |tab, scope| {
            crate::diff_load::fetch_raw_with_preview_head(
                &tab.ports,
                &tab.identity(),
                scope,
                tab.committed_unpushed,
            )
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

    /// The keys' inputs as this tab holds them.
    pub(super) fn preview_key_inputs(&self) -> PreviewKeyInputs<'_> {
        PreviewKeyInputs {
            repo_root: &self.repo_root,
            remote_repo: self.remote_repo.as_deref(),
            mode: self.mode,
            checkout_root: self.local_branch_checkout_root.as_deref(),
            preview_head_oid: self.preview_head_oid.as_deref(),
            history_commit: self
                .history
                .as_ref()
                .and_then(|h| h.commits.get(h.selected_commit))
                .map(|c| c.hash.as_str()),
            raw: self.raw_diff.as_deref(),
            diff_hash: &self.diff_hash,
            headers: &self.file_headers,
            mtimes: &self.mtime_cache,
            blob_ids: &self.preview_blob_ids,
        }
    }

    pub fn preview_context_key(&self) -> String {
        crate::preview_key::preview_context_key(&self.preview_key_inputs())
    }

    fn preview_section(&self, path: &str) -> Option<&str> {
        self.preview_key_inputs().section(path)
    }

    /// The key of a file this tab lists but has not parsed.
    pub fn stub_key(&self, path: &str) -> u64 {
        crate::file_rows::stub_key(path, self.preview_section(path))
    }

    pub fn file_preview_key(&self, path: &str) -> String {
        crate::preview_key::file_preview_key(&self.preview_key_inputs(), path)
    }

    pub fn capture_file_preview(&self, path: &str) -> Result<FilePreviewRequest> {
        if !supports_preview(path) {
            bail!("This file does not support document preview");
        }
        let status = self.capture_status(path)?;
        preview_read::capture_file_preview(&self.preview_source(), path, status)
    }

    /// Both sides of an image change. A modified image's earlier side is read
    /// from the old blob, which a remote review cannot reach locally.
    pub fn capture_image_preview(&self, path: &str) -> Result<ImagePreviewRequest> {
        if image_mime(path).is_none() {
            bail!("This file is not an image");
        }
        let status = self.capture_status(path)?;
        preview_read::capture_image_preview(&self.preview_source(), path, status)
    }

    fn preview_source(&self) -> PreviewSource<'_> {
        PreviewSource {
            keys: self.preview_key_inputs(),
            is_remote: self.is_remote(),
            pr_number: self.pr_number,
            branch_view: self.local_branch_view.is_some(),
            tour_is_pr: self.tour_is_pr,
            github: self.ports.github.clone(),
        }
    }

    /// The file's status in this view, after the checks a capture makes of
    /// the tab itself, in the order the reads made them.
    fn capture_status(&self, path: &str) -> Result<Option<FileStatus>> {
        let status = self.preview_file_status(path)?;
        if self.mode == DiffMode::History {
            self.history.as_ref().context("History is unavailable")?;
        }
        Ok(Some(status))
    }

    fn preview_file_status(&self, path: &str) -> Result<FileStatus> {
        preview_read::validate_path(path)?;
        self.visible_files()
            .into_iter()
            .find(|(_, f)| f.path == path)
            .map(|(_, f)| f.status.clone())
            .context("File is no longer in this review")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;
    use crate::preview_read::{gh_read_error, reconstruct, validate_path, verify_blob, Source};
    use std::path::Path;

    #[test]
    fn a_too_large_github_read_reports_the_preview_limit() {
        let error = gh_read_error(
            crate::github::GhError::TooLarge {
                message: "command output exceeds byte limit".into(),
            },
            2 * 1024 * 1024,
        );
        assert_eq!(error.to_string(), "File exceeds the 2 MiB preview limit");
    }

    #[test]
    fn other_github_read_errors_keep_their_reason() {
        let error = gh_read_error(
            crate::github::GhError::NotFound {
                message: "Not Found".into(),
            },
            1024,
        );
        assert_eq!(
            error.to_string(),
            "Document source could not be read: Not Found"
        );
    }

    fn tab(raw: &str, root: &Path, mode: DiffMode) -> TabState {
        let mut tab = super::super::tests::make_test_tab(git::parse_diff(raw));
        tab.ports = crate::ports::Ports::cli();
        tab.repo_root = root.to_string_lossy().into_owned();
        tab.mode = mode;
        tab.raw_diff = Some(raw.to_string());
        tab.file_headers = git::parse_diff_headers(raw);
        assert!(!tab.files.is_empty(), "No files parsed from {raw:?}");
        tab.refresh_mtime_cache();
        tab
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
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
                false,
                MAX_PREVIEW_BYTES
            )
            .unwrap(),
            b"one\r\ntwo"
        );
        assert_eq!(
            reconstruct(
                "@@ -1,2 +0,0 @@\n-one\r\n-two\n\\ No newline at end of file\n",
                true,
                MAX_PREVIEW_BYTES
            )
            .unwrap(),
            b"one\r\ntwo"
        );
        assert_eq!(
            reconstruct("new file mode 100644\n", false, MAX_PREVIEW_BYTES).unwrap(),
            b""
        );
        assert!(reconstruct(
            "Binary files a/a.md and b/a.md differ\n",
            false,
            MAX_PREVIEW_BYTES
        )
        .is_err());
        assert!(reconstruct(
            "@@ -0,0 +1 @@\n+invalid \u{fffd}\n",
            false,
            MAX_PREVIEW_BYTES
        )
        .is_err());
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
            t.capture_file_preview("added.md")
                .unwrap()
                .read(&er_git::CliGit)
                .unwrap(),
            "new\r\nlast"
        );
        assert_eq!(
            t.capture_file_preview("deleted.md")
                .unwrap()
                .read(&er_git::CliGit)
                .unwrap(),
            "old\r\nlast"
        );
        let changed = raw.replace("+new\r\n", "+wrong\r\n");
        assert_ne!(changed, raw);
        let t = tab(&changed, dir.path(), DiffMode::Staged);
        assert!(t
            .capture_file_preview("added.md")
            .unwrap()
            .read(&er_git::CliGit)
            .is_err());
        let changed = raw.replace("-old\r\n", "-wrong\r\n");
        let t = tab(&changed, dir.path(), DiffMode::Staged);
        assert!(t
            .capture_file_preview("deleted.md")
            .unwrap()
            .read(&er_git::CliGit)
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
                .read(&er_git::CliGit)
                .unwrap(),
            "valid \u{fffd}\n"
        );
        std::fs::write(&path, b"invalid \xff\n").unwrap();
        git(dir.path(), &["add", "."]);
        let out = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["diff", "--cached", "--no-ext-diff", "--no-color"])
            .output()
            .unwrap();
        let raw = String::from_utf8_lossy(&out.stdout);
        let t = tab(&raw, dir.path(), DiffMode::Staged);
        assert!(t
            .capture_file_preview("replacement.md")
            .unwrap()
            .read(&er_git::CliGit)
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
            t.capture_file_preview("readme.md")
                .unwrap()
                .read(&er_git::CliGit)
                .unwrap(),
            "staged\nunchanged\n"
        );
        let raw = git(dir.path(), &["diff"]);
        let mut t = tab(&raw, dir.path(), DiffMode::Unstaged);
        t.remote_repo = Some("owner/repo".into());
        t.local_branch_view = Some("feature".into());
        assert_eq!(
            t.capture_file_preview("readme.md")
                .unwrap()
                .read(&er_git::CliGit)
                .unwrap(),
            "working\nunchanged\n"
        );
        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "staged"],
        );
        let raw = git(dir.path(), &["diff", "HEAD~1", "HEAD"]);
        let t = tab(&raw, dir.path(), DiffMode::Branch);
        assert_eq!(
            t.capture_file_preview("readme.md")
                .unwrap()
                .read(&er_git::CliGit)
                .unwrap(),
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
            assert!(t
                .capture_file_preview("readme.md")
                .unwrap()
                .read(&er_git::CliGit)
                .is_err());
        }
        assert!(validate_path("../readme.md").is_err());
        assert!(validate_path("/readme.md").is_err());
        #[cfg(unix)]
        {
            let outside = tempfile::NamedTempFile::new().unwrap();
            std::fs::remove_file(dir.path().join("readme.md")).unwrap();
            std::os::unix::fs::symlink(outside.path(), dir.path().join("readme.md")).unwrap();
            let t = tab(raw, dir.path(), DiffMode::Unstaged);
            assert!(t
                .capture_file_preview("readme.md")
                .unwrap()
                .read(&er_git::CliGit)
                .is_err());
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
        assert!(request.read(&er_git::CliGit).is_err());
        t.raw_diff = Some(raw.replace("abcdef0", "1234567"));
        assert_ne!(context, t.preview_context_key());
        assert_ne!(key, t.file_preview_key("readme.md"));
        let key = t.file_preview_key("readme.md");
        t.local_branch_checkout_root = Some("/different/checkout".into());
        assert_ne!(key, t.file_preview_key("readme.md"));
    }

    /// A tab with every key input fixed, so its keys never depend on the disk.
    pub(crate) fn keyed_tab(mode: DiffMode) -> TabState {
        let raw = "diff --git a/readme.md b/readme.md\nindex abcdef0..1234567 100644\n--- a/readme.md\n+++ b/readme.md\n@@ -1 +1 @@\n-old\n+new\n";
        let mut t = super::super::tests::make_test_tab(git::parse_diff(raw));
        t.repo_root = "/keyed/repo".into();
        t.remote_repo = Some("owner/repo".into());
        t.mode = mode;
        t.raw_diff = Some(raw.to_string());
        t.file_headers = git::parse_diff_headers(raw);
        t.diff_hash = "diff-hash".into();
        t.preview_head_oid = Some("c".repeat(40));
        t.local_branch_checkout_root = Some("/keyed/checkout".into());
        t.preview_blob_ids = [("readme.md".to_string(), "d".repeat(40))].into();
        t.mtime_cache = [(
            "readme.md".to_string(),
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
        )]
        .into();
        t
    }

    /// The keys a fixed tab gives, recorded before the key steps moved out of
    /// `TabState`. A change here changes every key the desktop compares.
    #[test]
    fn preview_keys_stay_byte_identical() {
        let unstaged = keyed_tab(DiffMode::Unstaged);
        let history = keyed_tab(DiffMode::History);
        assert_eq!(
            [
                unstaged.preview_context_key(),
                unstaged.file_preview_key("readme.md"),
                history.preview_context_key(),
                history.file_preview_key("readme.md"),
                unstaged.file_preview_key("src/main.rs"),
            ],
            [
                "7b8ab87aa2d52f42",
                "61ef0459ef1d3eb0",
                "fb19f6ab31b58428",
                "3eef7decc2ae9e35",
                "",
            ]
            .map(str::to_string)
        );
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

    fn image_sides(t: &TabState, path: &str) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let request = t.capture_image_preview(path).unwrap();
        let read = |side: Option<FilePreviewRequest>| {
            side.map(|r| {
                r.read_bytes(&er_git::CliGit, MAX_IMAGE_PREVIEW_BYTES)
                    .unwrap()
            })
        };
        (read(request.before), read(request.after))
    }

    #[test]
    fn image_preview_reads_both_sides_of_binary_changes_in_each_mode() {
        let old = b"\x89PNG\r\n\x1a\n\0old".to_vec();
        let new = b"\x89PNG\r\n\x1a\n\0new".to_vec();
        let dir = repo();
        std::fs::write(dir.path().join("logo.png"), &old).unwrap();
        git(dir.path(), &["add", "."]);
        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "logo"],
        );
        std::fs::write(dir.path().join("logo.png"), &new).unwrap();

        let t = tab(&git(dir.path(), &["diff"]), dir.path(), DiffMode::Unstaged);
        assert!(t.files[0].hunks.is_empty());
        assert!(!t.file_preview_key("logo.png").is_empty());
        assert_eq!(
            image_sides(&t, "logo.png"),
            (Some(old.clone()), Some(new.clone()))
        );
        assert!(t.capture_file_preview("logo.png").is_err());

        git(dir.path(), &["add", "."]);
        let t = tab(
            &git(dir.path(), &["diff", "--cached"]),
            dir.path(),
            DiffMode::Staged,
        );
        assert_eq!(
            image_sides(&t, "logo.png"),
            (Some(old.clone()), Some(new.clone()))
        );

        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "new logo"],
        );
        let t = tab(
            &git(dir.path(), &["diff", "HEAD~1", "HEAD"]),
            dir.path(),
            DiffMode::Branch,
        );
        assert_eq!(
            image_sides(&t, "logo.png"),
            (Some(old.clone()), Some(new.clone()))
        );
        // The text reader still refuses image bytes.
        let request = t.capture_image_preview("logo.png").unwrap();
        assert!(request.after.unwrap().read(&er_git::CliGit).is_err());
    }

    #[test]
    fn image_preview_reads_added_and_deleted_binaries_from_their_blobs() {
        let bytes = b"GIF89a\0\x01\x02".to_vec();
        let dir = repo();
        std::fs::write(dir.path().join("added.gif"), &bytes).unwrap();
        git(dir.path(), &["add", "."]);
        let t = tab(
            &git(dir.path(), &["diff", "--cached"]),
            dir.path(),
            DiffMode::Staged,
        );
        // The checkout copy differs, so a match proves the read came from the staged blob.
        std::fs::write(dir.path().join("added.gif"), b"GIF89a\0changed").unwrap();
        assert_eq!(image_sides(&t, "added.gif"), (None, Some(bytes.clone())));

        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "gif"],
        );
        std::fs::remove_file(dir.path().join("added.gif")).unwrap();
        let t = tab(&git(dir.path(), &["diff"]), dir.path(), DiffMode::Unstaged);
        assert_eq!(image_sides(&t, "added.gif"), (Some(bytes), None));
    }

    #[test]
    fn image_preview_rebuilds_an_added_svg_from_its_text_diff() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>\n";
        let dir = repo();
        std::fs::write(dir.path().join("icon.svg"), svg).unwrap();
        git(dir.path(), &["add", "."]);
        let t = tab(
            &git(dir.path(), &["diff", "--cached"]),
            dir.path(),
            DiffMode::Staged,
        );
        let request = t.capture_image_preview("icon.svg").unwrap();
        assert!(request.before.is_none());
        let after = request.after.unwrap();
        assert!(matches!(after.source, Source::Diff(_, false)));
        assert_eq!(
            after
                .read_bytes(&er_git::CliGit, MAX_IMAGE_PREVIEW_BYTES)
                .unwrap(),
            svg.as_bytes()
        );
    }

    #[test]
    fn image_preview_reads_a_pr_deleted_image_from_the_local_old_blob() {
        let bytes = b"\x89PNG\r\n\x1a\n\0gone".to_vec();
        let dir = repo();
        std::fs::write(dir.path().join("logo.png"), &bytes).unwrap();
        git(dir.path(), &["add", "."]);
        git(
            dir.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "logo"],
        );
        git(dir.path(), &["rm", "-q", "logo.png"]);
        let mut t = tab(
            &git(dir.path(), &["diff", "--cached"]),
            dir.path(),
            DiffMode::PrDiff,
        );
        t.remote_repo = Some("owner/repo".into());
        t.local_branch_view = Some("feature".into());
        t.preview_head_oid = Some("a".repeat(40));
        let request = t.capture_image_preview("logo.png").unwrap();
        let before = request.before.unwrap();
        assert!(matches!(before.source, Source::Blob(..)));
        assert_eq!(
            before
                .read_bytes(&er_git::CliGit, MAX_IMAGE_PREVIEW_BYTES)
                .unwrap(),
            bytes
        );
    }

    #[test]
    fn preview_still_rejects_an_added_binary_document() {
        let dir = repo();
        std::fs::write(dir.path().join("notes.md"), b"text\0more\n").unwrap();
        git(dir.path(), &["add", "."]);
        let t = tab(
            &git(dir.path(), &["diff", "--cached"]),
            dir.path(),
            DiffMode::Staged,
        );
        let request = t.capture_file_preview("notes.md").unwrap();
        assert!(matches!(request.source, Source::Diff(..)));
        assert!(request.read(&er_git::CliGit).is_err());
    }

    #[test]
    fn image_mime_is_case_insensitive_and_limited_to_images() {
        assert_eq!(image_mime("a/Logo.PNG"), Some("image/png"));
        assert_eq!(image_mime("photo.jpeg"), Some("image/jpeg"));
        assert_eq!(image_mime("icon.svg"), Some("image/svg+xml"));
        assert_eq!(image_mime("readme.md"), None);
        assert_eq!(image_mime("png"), None);
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
                diffstat_known: true,
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
            request.read(&er_git::CliGit).unwrap(),
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
            t.capture_file_preview("readme.md")
                .unwrap()
                .read(&er_git::CliGit)
                .unwrap(),
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
                .read(&er_git::CliGit)
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
