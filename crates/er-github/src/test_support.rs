//! Constructors shared by unit tests.

use std::process::{ExitStatus, Output};

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(code as u32)
}

/// The `Output` a `gh` process would produce with this exit code and streams.
pub fn output(exit: i32, stdout: impl Into<Vec<u8>>, stderr: impl Into<Vec<u8>>) -> Output {
    Output {
        status: exit_status(exit),
        stdout: stdout.into(),
        stderr: stderr.into(),
    }
}
