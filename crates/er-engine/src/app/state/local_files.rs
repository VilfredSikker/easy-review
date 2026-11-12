//! Files the artifact store does not own: launch markers and debug transcripts.
//!
//! These carry no envelope and no version, so they are not kinds. The module is
//! deliberately tiny: a registered sidecar read or written here would skip the
//! store's rules, so the test below fails if a name here resolves to one.
//! Sidecars stay in the module that owns their kind.

use std::path::Path;

/// The marker a first-launch notice leaves behind, so it shows once.
pub(super) const LEGACY_ROOT_MARKER: &str = ".legacy-root-noticed";
/// The retired `[commands].summary` key was announced. Separate from the
/// legacy-root marker, so a machine that already saw that notice still hears
/// about the key.
pub(super) const RETIRED_SUMMARY_MARKER: &str = ".retired-summary-noticed";
/// The retired per-project `auto_triage` flag was announced. Separate from the
/// other two markers for the same reason.
pub(super) const RETIRED_AUTO_TRIAGE_MARKER: &str = ".retired-auto-triage-noticed";

/// The opt-in full agent transcript.
pub(super) const DEBUG_LOG_FILE: &str = "debug-agent.log";

/// Create the bucket before a command writes into it.
pub(super) fn ensure_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Write a launch marker. A failed write means the notice shows again next
/// launch, so the error is dropped on purpose.
pub(super) fn mark_notice_once(dir: &Path, marker: &str) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(marker), b"");
}

/// Write a full agent transcript when `ER_DEBUG` asked for one.
pub(super) fn write_debug_log(path: &Path, text: &str) {
    let _ = std::fs::write(path, text);
}

#[cfg(test)]
mod tests {
    /// No quoted name in the production part may resolve to a registered kind.
    /// The registry is the source of truth, so a new kind needs no edit here.
    #[test]
    fn the_helper_never_names_a_registered_kind() {
        let production = include_str!("local_files.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default();
        let registry = crate::store_registry::registry();
        for literal in production.split('"').skip(1).step_by(2) {
            if literal.is_empty() {
                continue;
            }
            assert!(
                registry.kind_for(std::path::Path::new(literal)).is_none(),
                "local_files.rs names the registered kind for {literal}"
            );
        }
    }
}
