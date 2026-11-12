//! Files the artifact store does not own: launch markers and debug transcripts.
//!
//! These carry no envelope and no version, so they are not kinds. The module is
//! deliberately tiny: a registered sidecar read or written here would skip the
//! store's rules, so the test below fails if the production part names one.
//! Sidecars stay in the module that owns their kind.

use std::path::Path;

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
    /// A kind's file must never be read or written through this module.
    #[test]
    fn the_helper_never_names_a_registered_kind() {
        let production = include_str!("local_files.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default();
        for file in [
            "review.json",
            "triage.json",
            "arbiter.json",
            "questions.json",
            "notes.json",
            "summary.md",
        ] {
            assert!(
                !production.contains(&format!("\"{file}\"")),
                "local_files.rs names the registered file {file}"
            );
        }
    }
}
