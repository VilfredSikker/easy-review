//! OS handoffs for the TUI: unpacking the downloaded release with the system
//! `tar`.
//!
//! `er::agent_spawn` treats this module as the TUI's one place to launch a
//! program. Every call is a user-visible handoff, so it takes no agent slot.

use std::path::Path;
use std::process::{Command, ExitStatus};

/// Extract a `.tar.gz` archive into `into`.
pub fn extract_tar_gz(archive: &Path, into: &Path) -> std::io::Result<ExitStatus> {
    Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .status()
}
