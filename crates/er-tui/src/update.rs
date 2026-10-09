//! Release check for the status-bar hint, and the `er update` self-install.
//! The decisions live in `er_engine::release_update`; this module only fetches.

use anyhow::{anyhow, bail, Context, Result};
use er_engine::release_update::{self, LatestRelease};
use std::io::Read;
use std::time::Duration;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(timeout)
        .user_agent(concat!("easy-review-tui/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn fetch_latest() -> Result<LatestRelease> {
    let body = agent(Duration::from_secs(8))
        .get(release_update::RELEASES_LATEST_API)
        .set("Accept", "application/vnd.github+json")
        .call()
        .context("GitHub releases request failed")?
        .into_string()?;
    release_update::parse_latest_release(&body)
}

fn download(url: &str) -> Result<Vec<u8>> {
    let resp = agent(Duration::from_secs(120))
        .get(url)
        .call()
        .with_context(|| format!("download {url}"))?;
    let mut bytes = Vec::new();
    resp.into_reader().read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Check off the render thread. The receiver yields a version only when one is
/// newer; offline or rate-limited stays silent.
pub fn spawn_background_check() -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let cache = release_update::check_cache_path();
        let release = match release_update::load_cached_release(&cache, release_update::CHECK_TTL)
        {
            Some(r) => r,
            None => match fetch_latest() {
                Ok(r) => {
                    let _ = release_update::save_cached_release(&cache, &r);
                    r
                }
                Err(_) => return,
            },
        };
        if release_update::version_is_newer(release.version(), env!("CARGO_PKG_VERSION")) {
            let _ = tx.send(release.version().to_string());
        }
    });
    rx
}

/// `er update`: download the latest release for this platform, verify it
/// against the release's SHA256SUMS, and swap it in for the running binary.
pub fn run() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let exe = std::env::current_exe()?.canonicalize()?;
    if release_update::is_cargo_install(&exe) {
        bail!(
            "{} was built with cargo; update it the same way:\n  git pull && cargo install --path crates/er-tui",
            exe.display()
        );
    }
    let asset = release_update::tui_asset_name(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or_else(|| {
            anyhow!(
                "no prebuilt er for {}-{}; build from source instead",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        })?;

    println!("Checking for updates…");
    let release = fetch_latest()?;
    if !release_update::version_is_newer(release.version(), current) {
        println!("er {current} is the latest version.");
        return Ok(());
    }

    println!("Downloading er {} …", release.version());
    let sums = String::from_utf8(download(&release.asset_url(release_update::CHECKSUMS_ASSET))?)
        .context("SHA256SUMS is not text")?;
    let expected = release_update::checksum_for(&sums, &asset)
        .ok_or_else(|| anyhow!("{asset} is not listed in the release's SHA256SUMS"))?;
    let archive = download(&release.asset_url(&asset))?;
    release_update::verify_sha256(&archive, &expected)?;

    let work = tempfile::tempdir()?;
    let archive_path = work.path().join(&asset);
    std::fs::write(&archive_path, &archive)?;
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&archive_path)
        .arg("-C")
        .arg(work.path())
        .status()
        .context("run tar")?;
    if !status.success() {
        bail!("tar could not extract {asset}");
    }
    release_update::replace_executable(&exe, &work.path().join("er"))?;
    let _ = release_update::save_cached_release(&release_update::check_cache_path(), &release);

    println!("Updated er {current} → {} at {}", release.version(), exe.display());
    Ok(())
}
