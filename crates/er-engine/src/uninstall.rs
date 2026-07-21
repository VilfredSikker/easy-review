//! Uninstall Easy Review user data and (optionally) installed apps.
//!
//! Shared by the TUI (`er uninstall`) and Desktop settings. Removes config,
//! managed review storage, legacy cache, and discovered binaries / app bundles.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::storage;

/// What kind of artifact an uninstall target represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallKind {
    Config,
    Data,
    Cache,
    Binary,
    DesktopApp,
}

impl UninstallKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Config => "Config",
            Self::Data => "Review data",
            Self::Cache => "Legacy cache",
            Self::Binary => "Terminal binary",
            Self::DesktopApp => "Desktop app",
        }
    }
}

/// One path that uninstall may remove.
#[derive(Debug, Clone)]
pub struct UninstallTarget {
    pub kind: UninstallKind,
    pub path: PathBuf,
    pub exists: bool,
}

impl UninstallTarget {
    pub fn description(&self) -> String {
        format!("{} — {}", self.kind.label(), self.path.display())
    }
}

/// Options controlling which uninstall categories are included.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UninstallOptions {
    pub remove_config: bool,
    pub remove_data: bool,
    pub remove_cache: bool,
    pub remove_binaries: bool,
    pub remove_desktop_app: bool,
}

impl Default for UninstallOptions {
    fn default() -> Self {
        Self::full()
    }
}

impl UninstallOptions {
    /// Remove config, review data, cache, binaries, and desktop app when found.
    pub fn full() -> Self {
        Self {
            remove_config: true,
            remove_data: true,
            remove_cache: true,
            remove_binaries: true,
            remove_desktop_app: true,
        }
    }
}

/// Result of executing an uninstall plan.
#[derive(Debug, Clone, Default)]
pub struct UninstallReport {
    pub removed: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    pub deferred: Vec<PathBuf>,
    pub missing: Vec<PathBuf>,
}

impl UninstallReport {
    pub fn is_success(&self) -> bool {
        self.failed.is_empty()
    }

    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for p in &self.removed {
            lines.push(format!("removed  {}", p.display()));
        }
        for p in &self.deferred {
            lines.push(format!("deferred {}", p.display()));
        }
        for p in &self.missing {
            lines.push(format!("missing  {}", p.display()));
        }
        for (p, err) in &self.failed {
            lines.push(format!("failed   {}: {err}", p.display()));
        }
        lines
    }
}

/// All known config directory locations (shared resolver + platform config dir).
pub fn config_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = crate::config::global_config_dir() {
        out.push(dir);
    }
    // Also cover the platform config dir when it differs from XDG/HOME resolution
    // (e.g. macOS `~/Library/Application Support/er`).
    if let Some(platform) = dirs::config_dir().map(|d| d.join("er")) {
        if !out.iter().any(|d| paths_same(d, &platform)) {
            out.push(platform);
        }
    }
    out
}

/// Extra app-data dirs Tauri may create under the bundle identifier.
pub fn bundle_data_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(data) = dirs::data_dir() {
        out.push(data.join("com.reshape.easy-review"));
        out.push(data.join("Easy Review"));
    }
    if let Some(cache) = dirs::cache_dir() {
        out.push(cache.join("com.reshape.easy-review"));
        out.push(cache.join("Easy Review"));
        out.push(cache.join("er"));
    }
    out
}

/// Legacy cache directory (`~/.cache/er`).
pub fn cache_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("er"));
        }
    }
    dirs::home_dir().map(|h| h.join(".cache").join("er"))
}

/// Build the uninstall plan for the given options (lists expected roots even when
/// missing so dry-run / UI can show the full picture).
pub fn plan(opts: &UninstallOptions) -> Vec<UninstallTarget> {
    let mut targets = Vec::new();

    if opts.remove_config {
        for dir in config_dirs() {
            push_unique(&mut targets, UninstallKind::Config, dir);
        }
    }
    if opts.remove_data {
        push_unique(&mut targets, UninstallKind::Data, storage::storage_root());
        for dir in bundle_data_dirs() {
            push_unique(&mut targets, UninstallKind::Data, dir);
        }
    }
    if opts.remove_cache {
        if let Some(dir) = cache_dir() {
            push_unique(&mut targets, UninstallKind::Cache, dir);
        }
    }
    if opts.remove_binaries {
        for path in discover_binaries() {
            push_unique(&mut targets, UninstallKind::Binary, path);
        }
    }
    if opts.remove_desktop_app {
        for path in discover_desktop_apps() {
            push_unique(&mut targets, UninstallKind::DesktopApp, path);
        }
    }

    targets
}

/// Targets that currently exist on disk.
pub fn existing_targets(opts: &UninstallOptions) -> Vec<UninstallTarget> {
    plan(opts)
        .into_iter()
        .filter(|t| t.exists || t.path.exists())
        .map(|mut t| {
            t.exists = true;
            t
        })
        .collect()
}

fn push_unique(targets: &mut Vec<UninstallTarget>, kind: UninstallKind, path: PathBuf) {
    if targets.iter().any(|t| paths_same(&t.path, &path)) {
        return;
    }
    let exists = path_exists(&path);
    targets.push(UninstallTarget { kind, path, exists });
}

fn path_exists(path: &Path) -> bool {
    // Use symlink_metadata so a dangling symlink still counts as present.
    std::fs::symlink_metadata(path).is_ok()
}

/// Discover installed `er` binaries in common locations (+ current exe when installed).
pub fn discover_binaries() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut candidates = Vec::new();

    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/er"));
        candidates.push(home.join(".cargo/bin/er"));
        #[cfg(windows)]
        {
            candidates.push(home.join(".local/bin/er.exe"));
            candidates.push(home.join(".cargo/bin/er.exe"));
        }
    }
    candidates.push(PathBuf::from("/usr/local/bin/er"));
    candidates.push(PathBuf::from("/opt/homebrew/bin/er"));

    if let Ok(exe) = std::env::current_exe() {
        if looks_like_product_binary(&exe) && is_install_location(&exe) {
            candidates.push(exe);
        }
    }

    for path in candidates {
        if path_exists(&path) && !found.iter().any(|p: &PathBuf| paths_same(p, &path)) {
            // Prefer the path as given for removal; canonicalize only for dedupe key.
            found.push(path);
        }
    }
    found
}

fn looks_like_product_binary(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // Installed product binary is always `er` (see er-tui [[bin]] name).
    name == "er" || name == "er.exe"
}

/// True for typical install dirs — not cargo `target/` build outputs.
fn is_install_location(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if s.contains("/target/") || s.contains("\\target\\") {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let parent = parent.to_string_lossy();
    parent.ends_with("/.local/bin")
        || parent.ends_with("/.cargo/bin")
        || parent.ends_with("/usr/local/bin")
        || parent.ends_with("/opt/homebrew/bin")
        || parent.ends_with("\\.local\\bin")
        || parent.ends_with("\\.cargo\\bin")
}

/// Discover desktop app bundles / install directories.
pub fn discover_desktop_apps() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut candidates = Vec::new();

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from("/Applications/Easy Review.app"));
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("Applications/Easy Review.app"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join(".local/share/applications/easy-review.desktop"));
            candidates.push(home.join(".local/bin/easy-review"));
        }
        candidates.push(PathBuf::from("/usr/local/bin/easy-review"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local) = dirs::data_local_dir() {
            candidates.push(local.join("Easy Review"));
            candidates.push(local.join("Programs").join("Easy Review"));
        }
    }

    // If we're running inside a .app bundle outside cargo target/, include it.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(app) = enclosing_app_bundle(&exe) {
            let app_s = app.to_string_lossy();
            if !app_s.contains("/target/") && !app_s.contains("\\target\\") {
                candidates.push(app);
            }
        }
    }

    for path in candidates {
        if path_exists(&path) && !found.iter().any(|p: &PathBuf| paths_same(p, &path)) {
            found.push(path);
        }
    }
    found
}

fn enclosing_app_bundle(exe: &Path) -> Option<PathBuf> {
    let mut cur = exe.parent()?;
    for _ in 0..8 {
        if cur
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.ends_with(".app"))
        {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
    None
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn paths_same(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Remove the given targets. Paths that are the currently running executable or its
/// enclosing `.app` are returned in [`UninstallReport::deferred`] — callers must
/// [`schedule_deferred_removal`] then exit so the waiter can finish.
pub fn execute(targets: &[UninstallTarget]) -> UninstallReport {
    let mut report = UninstallReport::default();

    let current_exe = std::env::current_exe()
        .ok()
        .map(|p| canonicalize_best_effort(&p));
    let current_app = current_exe
        .as_ref()
        .and_then(|p| enclosing_app_bundle(p).map(|a| canonicalize_best_effort(&a)));

    for target in targets {
        if !path_exists(&target.path) {
            report.missing.push(target.path.clone());
            continue;
        }

        let path = &target.path;
        let is_self_binary = current_exe
            .as_ref()
            .is_some_and(|exe| paths_same(exe, path));
        let is_self_app = current_app
            .as_ref()
            .is_some_and(|app| paths_same(app, path));

        if is_self_binary || is_self_app {
            report.deferred.push(path.clone());
            continue;
        }

        match remove_path(path) {
            Ok(()) => report.removed.push(path.clone()),
            Err(e) => report.failed.push((path.clone(), e.to_string())),
        }
    }

    report
}

/// Remove a file, symlink, or directory without following directory symlinks.
fn remove_path(path: &Path) -> std::io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    if meta.file_type().is_symlink() || meta.file_type().is_file() {
        std::fs::remove_file(path)
    } else if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        // Fallback for unusual file types (fifo/socket): try unlink.
        std::fs::remove_file(path)
    }
}

/// Schedule path removals after this process exits.
///
/// Waits until our PID is gone **or** the process start-time no longer matches
/// the token captured at schedule time (PID reuse), then `rm -rf`. A timeout
/// without either condition skips deletion so an unrelated process that reused
/// the PID is never waited out into a blind delete.
pub fn schedule_deferred_removal(paths: &[PathBuf]) -> std::io::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }

    let pid = std::process::id();

    #[cfg(unix)]
    {
        let start = process_start_token_unix(pid).unwrap_or_default();
        let start_q = shell_single_quote(&start);
        let quoted: Vec<String> = paths
            .iter()
            .map(|p| shell_single_quote(&p.to_string_lossy()))
            .collect();
        let joined = quoted.join(" ");
        // Exit the wait loop when: PID gone, start-time mismatch (reuse), or
        // timeout. Only delete when PID is gone or start-time mismatched —
        // never after a blind timeout while the original-looking PID still runs.
        let script = format!(
            "pid={pid}; start={start_q}; i=0; do_rm=0; \
             while kill -0 \"$pid\" 2>/dev/null; do \
               cur=$(ps -p \"$pid\" -o lstart= 2>/dev/null || true); \
               if [ -n \"$start\" ] && [ \"$cur\" != \"$start\" ]; then do_rm=1; break; fi; \
               i=$((i+1)); [ \"$i\" -gt 120 ] && break; sleep 0.5; \
             done; \
             if [ \"$do_rm\" -eq 1 ] || ! kill -0 \"$pid\" 2>/dev/null; then \
               rm -rf {joined}; \
             fi"
        );
        Command::new("sh")
            .arg("-c")
            .arg(format!("( {script} ) >/dev/null 2>&1 &"))
            .spawn()?;
        Ok(())
    }

    #[cfg(windows)]
    {
        let list: Vec<String> = paths
            .iter()
            .map(|p| format!("\"{}\"", p.to_string_lossy().replace('"', "")))
            .collect();
        let joined = list.join(",");
        let script = format!(
            "$pid={pid}; \
             $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue; \
             $start = if ($proc) {{ $proc.StartTime }} else {{ $null }}; \
             $doRm = $false; \
             for ($i=0; $i -lt 120; $i++) {{ \
               $p = Get-Process -Id $pid -ErrorAction SilentlyContinue; \
               if (-not $p) {{ $doRm = $true; break }}; \
               if ($start -and $p.StartTime -ne $start) {{ $doRm = $true; break }}; \
               Start-Sleep -Milliseconds 500 \
             }}; \
             if ($doRm) {{ \
               @({joined}) | ForEach-Object {{ if (Test-Path $_) {{ Remove-Item -LiteralPath $_ -Recurse -Force }} }} \
             }}"
        );
        Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .spawn()?;
        Ok(())
    }
}

#[cfg(unix)]
fn process_start_token_unix(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn execute_removes_directories() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("easy-review-data");
        fs::create_dir_all(dir.join("repos")).unwrap();
        fs::write(dir.join("repos/marker"), "x").unwrap();

        let targets = vec![UninstallTarget {
            kind: UninstallKind::Data,
            path: dir.clone(),
            exists: true,
        }];
        let report = execute(&targets);
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert!(report.removed.iter().any(|p| p == &dir));
        assert!(!dir.exists());
    }

    #[test]
    fn execute_reports_missing() {
        let path = PathBuf::from("/tmp/er-uninstall-definitely-missing-xyz");
        let targets = vec![UninstallTarget {
            kind: UninstallKind::Cache,
            path: path.clone(),
            exists: false,
        }];
        let report = execute(&targets);
        assert!(report.missing.iter().any(|p| p == &path));
        assert!(report.removed.is_empty());
    }

    #[test]
    fn remove_path_unlinks_symlink_not_target() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("real-dir");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), "safe").unwrap();
        let link = tmp.path().join("link-dir");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link).unwrap();
            remove_path(&link).unwrap();
            assert!(!link.exists());
            assert!(
                target.join("keep.txt").exists(),
                "symlink target must survive"
            );
        }
        #[cfg(not(unix))]
        {
            let _ = (target, link);
        }
    }

    #[test]
    fn shell_quote_escapes_apostrophe() {
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn product_binary_name_is_er_only() {
        assert!(looks_like_product_binary(Path::new("/usr/local/bin/er")));
        assert!(!looks_like_product_binary(Path::new(
            "/usr/local/bin/er-tui"
        )));
        // Name check alone accepts cargo outputs; install_location gates those out.
        assert!(looks_like_product_binary(Path::new("/tmp/target/debug/er")));
        assert!(!is_install_location(Path::new("/tmp/target/debug/er")));
    }

    #[test]
    fn install_location_rejects_cargo_target() {
        assert!(!is_install_location(Path::new(
            "/Users/me/proj/target/tui/debug/er"
        )));
        assert!(is_install_location(Path::new("/Users/me/.local/bin/er")));
    }

    #[test]
    fn full_options_default_all_on() {
        let opts = UninstallOptions::default();
        assert!(opts.remove_config);
        assert!(opts.remove_data);
        assert!(opts.remove_cache);
        assert!(opts.remove_binaries);
        assert!(opts.remove_desktop_app);
    }
}
