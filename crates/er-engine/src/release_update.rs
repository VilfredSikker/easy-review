//! The decisions behind "is there a newer release, and can this binary replace
//! itself with it". The front ends fetch: the engine has no HTTP client
//! (ADR 0026), and release assets are anonymous public downloads that need no
//! `gh` login. See `docs/adr/0043-in-app-updates.md`.

use anyhow::{anyhow, bail, Context, Result};
use er_store::{Artifact, Loaded, Mode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const RELEASES_LATEST_API: &str =
    "https://api.github.com/repos/VilfredSikker/easy-review/releases/latest";

/// Name of the checksum file the release workflow attaches next to the TUI archives.
pub const CHECKSUMS_ASSET: &str = "SHA256SUMS";

/// How long a launch reuses the last answer. The anonymous API allows 60
/// requests an hour per IP, shared with every other tool on the machine.
pub const CHECK_TTL: Duration = Duration::from_hours(6);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
            "{} was built with cargo; update it the same way:\n  cargo install --force --git https://github.com/VilfredSikker/easy-review er-tui\n  or, from a clone: git pull && cargo install --path crates/er-tui",
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

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub(crate) struct CheckCache {
    checked_at_unix: u64,
    release: LatestRelease,
}

impl Artifact for CheckCache {
    const KIND: &'static str = "update-check";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Enveloped;
    const FILE: &'static str = "update-check.json";
    type Slot = ();
}

pub(crate) fn register(
    registry: &mut er_store::Registry,
) -> std::result::Result<(), er_store::StoreError> {
    registry.register::<CheckCache>()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// The cached release, when one was checked within `ttl`.
pub fn load_cached_release(ttl: Duration) -> Option<LatestRelease> {
    let key = crate::store_registry::app_key::<CheckCache>().ok()?;
    let cache = match crate::store_registry::store().read(&key).ok()? {
        Loaded::Found(stored) => stored.value,
        Loaded::Absent | Loaded::Unreadable(_) => return None,
    };
    (unix_now().saturating_sub(cache.checked_at_unix) < ttl.as_secs()).then_some(cache.release)
}

/// Every worktree runs its own `er` and they can all save at once on launch;
/// the store writes each save through a unique temp file.
pub fn save_cached_release(release: &LatestRelease) -> Result<()> {
    let key = crate::store_registry::app_key::<CheckCache>()?;
    let cache = CheckCache {
        checked_at_unix: unix_now(),
        release: release.clone(),
    };
    crate::store_registry::store()
        .write(&key, &cache)
        .map_err(|error| anyhow!("save update check: {error}"))
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
        assert!(cargo.is_some_and(
            |m| m.contains("built with cargo") && m.contains("cargo install --force --git")
        ));
        let dev = self_update_blocker(Path::new("/repo/target/tui/debug/er"), "macos", "aarch64");
        assert!(dev.is_some_and(|m| m.contains("built with cargo")));
        let arm_linux = self_update_blocker(script, "linux", "aarch64");
        assert!(arm_linux.is_some_and(|m| m.contains("no prebuilt er for linux-aarch64")));
    }

    fn with_store<F: FnOnce(&std::path::Path) -> R, R>(f: F) -> R {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || f(dir.path()))
    }

    #[test]
    fn cache_round_trips_and_expires() {
        with_store(|_| {
            let rel = LatestRelease {
                tag: "v0.5.9".into(),
                html_url: "https://x".into(),
            };
            save_cached_release(&rel).unwrap();
            assert_eq!(load_cached_release(CHECK_TTL), Some(rel));
            assert_eq!(load_cached_release(Duration::ZERO), None);
        });
    }

    #[test]
    fn update_check_ttl_respected_through_store() {
        with_store(|_| {
            let fresh = LatestRelease {
                tag: "v1".into(),
                html_url: "https://x".into(),
            };
            save_cached_release(&fresh).unwrap();
            assert_eq!(load_cached_release(CHECK_TTL), Some(fresh));

            // Rewrite the stored check with an old timestamp: the TTL rejects it.
            let path = crate::store_registry::app_path::<CheckCache>().expect("record path");
            std::fs::write(
                &path,
                r#"{"kind":"update-check","version":1,"diff_hash":null,"producer":"host","written_at":"2026-01-01T00:00:00Z","payload":{"checked_at_unix":0,"release":{"tag":"v1","html_url":"https://x"}}}"#,
            )
            .unwrap();
            assert_eq!(load_cached_release(CHECK_TTL), None);
        });
    }

    #[test]
    #[expect(
        clippy::excessive_nesting,
        reason = "each writer thread scopes its own storage root"
    )]
    fn concurrent_cache_saves_never_leave_a_torn_file() {
        with_store(|root| {
            // A long tag makes each write big enough for a plain overwrite to be
            // observed half-written.
            let rel = LatestRelease {
                tag: format!("v0.5.9-{}", "x".repeat(256 * 1024)),
                html_url: "https://x".into(),
            };
            save_cached_release(&rel).unwrap();
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let root = root.to_path_buf();
            let writers: Vec<_> = (0..4)
                .map(|_| {
                    let (rel, stop, root) = (rel.clone(), stop.clone(), root.clone());
                    // Thread-local overrides do not cross a spawn, so each
                    // writer scopes the storage root itself.
                    std::thread::spawn(move || {
                        crate::storage::with_test_storage_root(&root, || {
                            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                                save_cached_release(&rel).unwrap();
                            }
                        });
                    })
                })
                .collect();
            for _ in 0..200 {
                assert_eq!(load_cached_release(CHECK_TTL).as_ref(), Some(&rel));
            }
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            for w in writers {
                w.join().unwrap();
            }
            let names: Vec<String> = std::fs::read_dir(root.join("app"))
                .unwrap()
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            assert!(
                !names.iter().any(|name| name.contains(".tmp")),
                "staged temp files left behind: {names:?}"
            );
            assert!(
                names.iter().any(|name| name == "update-check.json"),
                "{names:?}"
            );
        });
    }
}
