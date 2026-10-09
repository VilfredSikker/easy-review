//! The decisions behind "is there a newer release, and can this binary replace
//! itself with it". The front ends fetch: the engine has no HTTP client
//! (ADR 0026), and release assets are anonymous public downloads that need no
//! `gh` login. See `docs/adr/0043-in-app-updates.md`.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const RELEASES_LATEST_API: &str =
    "https://api.github.com/repos/VilfredSikker/easy-review/releases/latest";

/// Name of the checksum file the release workflow attaches next to the TUI archives.
pub const CHECKSUMS_ASSET: &str = "SHA256SUMS";

/// How long a launch reuses the last answer. The anonymous API allows 60
/// requests an hour per IP, shared with every other tool on the machine.
pub const CHECK_TTL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestRelease {
    pub tag: String,
    pub html_url: String,
}

impl LatestRelease {
    pub fn version(&self) -> &str {
        self.tag.trim().trim_start_matches('v')
    }

    pub fn asset_url(&self, asset: &str) -> String {
        format!(
            "https://github.com/VilfredSikker/easy-review/releases/download/{}/{}",
            self.tag, asset
        )
    }
}

/// Parse a `releases/latest` response. Drafts and prereleases are not offered.
pub fn parse_latest_release(json: &str) -> Result<LatestRelease> {
    #[derive(Deserialize)]
    struct GhRelease {
        tag_name: String,
        html_url: String,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }
    let body: GhRelease = serde_json::from_str(json).context("parse GitHub release JSON")?;
    if body.draft || body.prerelease {
        bail!("latest release is a draft or prerelease");
    }
    let tag = body.tag_name.trim().to_string();
    if tag.is_empty() {
        bail!("release has an empty tag");
    }
    Ok(LatestRelease {
        tag,
        html_url: body.html_url,
    })
}

fn parse_version_parts(raw: &str) -> Option<Vec<u64>> {
    // Pre-release / build metadata does not order ("0.4.7-rc.1" → 0.4.7). Cut it
    // before splitting, or its own dots read as extra version components.
    let s = raw.trim().trim_start_matches('v');
    let core = s.split(['-', '+']).next().unwrap_or_default();
    if core.is_empty() {
        return None;
    }
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// True when `latest` is strictly newer than `current`. Unparseable input is
/// never newer, so a malformed tag cannot trigger an install.
pub fn version_is_newer(latest: &str, current: &str) -> bool {
    let (Some(mut a), Some(mut b)) = (parse_version_parts(latest), parse_version_parts(current))
    else {
        return false;
    };
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// Archive name the release workflow publishes for this platform, or `None`
/// when no prebuilt binary exists for it.
pub fn tui_asset_name(os: &str, arch: &str) -> Option<String> {
    let triple = match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return None,
    };
    Some(format!("er-{triple}.tar.gz"))
}

/// Look up `asset` in `sha256sum` output (`<hex>  <name>`, or `<hex> *<name>`).
pub fn checksum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hex, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == asset).then(|| hex.to_ascii_lowercase())
    })
}

pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected_hex.trim().to_ascii_lowercase() {
        bail!("checksum mismatch: expected {expected_hex}, got {actual}");
    }
    Ok(())
}

/// A cargo-built `er` is updated by rebuilding. Swapping in a release binary
/// would silently drop whatever the user built from source.
pub fn is_cargo_install(exe: &Path) -> bool {
    let s = exe.to_string_lossy();
    s.contains("/.cargo/bin/") || s.contains("/target/")
}

/// Why `er update` cannot replace `exe`, or `None` when it can. The status-bar
/// hint asks the same question, so it never names a command that would refuse.
pub fn self_update_blocker(exe: &Path, os: &str, arch: &str) -> Option<String> {
    if is_cargo_install(exe) {
        return Some(format!(
            "{} was built with cargo; update it the same way:\n  git pull && cargo install --path crates/er-tui",
            exe.display()
        ));
    }
    if tui_asset_name(os, arch).is_none() {
        return Some(format!(
            "no prebuilt er for {os}-{arch}; build from source instead"
        ));
    }
    None
}

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

#[derive(Debug, Serialize, Deserialize)]
struct CheckCache {
    checked_at_unix: u64,
    release: LatestRelease,
}

pub fn check_cache_path() -> PathBuf {
    crate::storage::storage_root().join("update-check.json")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn load_cached_release(path: &Path, ttl: Duration) -> Option<LatestRelease> {
    let raw = std::fs::read_to_string(path).ok()?;
    let cache: CheckCache = serde_json::from_str(&raw).ok()?;
    (unix_now().saturating_sub(cache.checked_at_unix) < ttl.as_secs()).then_some(cache.release)
}

/// Written through a uniquely named temp file: every worktree runs its own
/// `er`, and they can all save at once on launch.
pub fn save_cached_release(path: &Path, release: &LatestRelease) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(dir)?;
    let body = serde_json::to_string(&CheckCache {
        checked_at_unix: unix_now(),
        release: release.clone(),
    })?;
    let mut staged = tempfile::Builder::new()
        .prefix(".update-check-")
        .tempfile_in(dir)?;
    std::io::Write::write_all(&mut staged, body.as_bytes())?;
    staged
        .persist(path)
        .map_err(|e| anyhow!("write {}: {}", path.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_compares_numerically_not_lexically() {
        assert!(version_is_newer("0.5.10", "0.5.9"));
        assert!(version_is_newer("v1.0.0", "0.9.9"));
        assert!(version_is_newer("0.6", "0.5.8"));
        assert!(version_is_newer("0.4.7", "0.4"));
        assert!(!version_is_newer("0.5.8", "0.5.8"));
        assert!(!version_is_newer("0.5.7", "0.5.8"));
        assert!(!version_is_newer("0.5.8-rc.1", "0.5.8"));
    }

    #[test]
    fn unparseable_version_is_never_newer() {
        assert!(!version_is_newer("nightly", "0.5.8"));
        assert!(!version_is_newer("", "0.5.8"));
        assert!(!version_is_newer("9.9.9", "garbage"));
    }

    #[test]
    fn release_parse_rejects_drafts_prereleases_and_empty_tags() {
        let ok = r#"{"tag_name":"v0.5.9","html_url":"https://x/v0.5.9"}"#;
        let rel = parse_latest_release(ok).unwrap();
        assert_eq!(rel.version(), "0.5.9");
        assert_eq!(
            rel.asset_url("SHA256SUMS"),
            "https://github.com/VilfredSikker/easy-review/releases/download/v0.5.9/SHA256SUMS"
        );
        assert!(
            parse_latest_release(r#"{"tag_name":"v1","html_url":"u","prerelease":true}"#).is_err()
        );
        assert!(parse_latest_release(r#"{"tag_name":"v1","html_url":"u","draft":true}"#).is_err());
        assert!(parse_latest_release(r#"{"tag_name":"  ","html_url":"u"}"#).is_err());
    }

    #[test]
    fn asset_names_match_the_release_matrix() {
        assert_eq!(
            tui_asset_name("macos", "aarch64").as_deref(),
            Some("er-aarch64-apple-darwin.tar.gz")
        );
        assert_eq!(
            tui_asset_name("linux", "x86_64").as_deref(),
            Some("er-x86_64-unknown-linux-gnu.tar.gz")
        );
        assert_eq!(tui_asset_name("linux", "aarch64"), None);
        assert_eq!(tui_asset_name("windows", "x86_64"), None);
    }

    #[test]
    fn checksum_lookup_handles_text_and_binary_markers() {
        let sums =
            "ABC123  er-aarch64-apple-darwin.tar.gz\ndef456 *er-x86_64-unknown-linux-gnu.tar.gz\n";
        assert_eq!(
            checksum_for(sums, "er-aarch64-apple-darwin.tar.gz").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            checksum_for(sums, "er-x86_64-unknown-linux-gnu.tar.gz").as_deref(),
            Some("def456")
        );
        assert_eq!(checksum_for(sums, "er-x86_64-apple-darwin.tar.gz"), None);
    }

    #[test]
    fn sha256_verify_accepts_match_and_rejects_tampering() {
        // sha256("hello")
        let hex = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        verify_sha256(b"hello", hex).unwrap();
        verify_sha256(b"hello", &hex.to_ascii_uppercase()).unwrap();
        assert!(verify_sha256(b"hellO", hex).is_err());
    }

    #[test]
    fn cargo_builds_are_not_replaced_from_releases() {
        assert!(!is_cargo_install(Path::new("/Users/a/.local/bin/er")));
        assert!(!is_cargo_install(Path::new("/usr/local/bin/er")));
        assert!(is_cargo_install(Path::new("/Users/a/.cargo/bin/er")));
        assert!(is_cargo_install(Path::new("/repo/target/tui/debug/er")));
    }

    #[test]
    fn self_update_blocked_for_cargo_builds_and_unsupported_platforms() {
        let script = Path::new("/Users/a/.local/bin/er");
        assert_eq!(self_update_blocker(script, "macos", "aarch64"), None);
        assert_eq!(self_update_blocker(script, "linux", "x86_64"), None);

        let cargo = self_update_blocker(Path::new("/Users/a/.cargo/bin/er"), "macos", "aarch64");
        assert!(cargo.is_some_and(|m| m.contains("built with cargo")));
        let dev = self_update_blocker(Path::new("/repo/target/tui/debug/er"), "macos", "aarch64");
        assert!(dev.is_some_and(|m| m.contains("built with cargo")));
        let arm_linux = self_update_blocker(script, "linux", "aarch64");
        assert!(arm_linux.is_some_and(|m| m.contains("no prebuilt er for linux-aarch64")));
    }

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

    #[test]
    fn cache_round_trips_and_expires() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("update-check.json");
        let rel = LatestRelease {
            tag: "v0.5.9".into(),
            html_url: "https://x".into(),
        };
        save_cached_release(&path, &rel).unwrap();
        assert_eq!(load_cached_release(&path, CHECK_TTL), Some(rel));
        assert_eq!(load_cached_release(&path, Duration::ZERO), None);
    }

    #[test]
    fn concurrent_cache_saves_never_leave_a_torn_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        // A long tag makes each write big enough for a plain overwrite to be
        // observed half-written.
        let rel = LatestRelease {
            tag: format!("v0.5.9-{}", "x".repeat(256 * 1024)),
            html_url: "https://x".into(),
        };
        save_cached_release(&path, &rel).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writers: Vec<_> = (0..4)
            .map(|_| {
                let (path, rel, stop) = (path.clone(), rel.clone(), stop.clone());
                std::thread::spawn(move || {
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        save_cached_release(&path, &rel).unwrap();
                    }
                })
            })
            .collect();
        for _ in 0..200 {
            assert_eq!(load_cached_release(&path, CHECK_TTL).as_ref(), Some(&rel));
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for w in writers {
            w.join().unwrap();
        }
        let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(leftovers, 1, "staged temp files left behind");
    }
}
