//! Bounded subprocess execution.
//!
//! `Command::output()` has no upper bound. A black-holed TCP connect, an
//! unresponsive GitHub, or an ssh passphrase prompt read from `/dev/tty` leaves
//! the caller blocked until the OS or a human gives up — and several of these
//! callers run with the desktop App lock held, where one stuck call freezes
//! every command in the app rather than degrading a single row.
//!
//! [`run_with_timeout`] puts a ceiling on that. It follows the shape already
//! used by [`crate::model_discovery::run_models_command`], and adds the
//! environment guards that stop a tool from asking a question in the first
//! place.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// One `gh` round trip.
///
/// A warm `gh repo view` measures 0.46–0.67s on a normal connection, so ten
/// seconds is far past any healthy response while still short enough that a
/// stalled call surfaces as an error instead of a frozen window.
pub const GH_TIMEOUT: Duration = Duration::from_secs(10);

/// A `git fetch`.
///
/// Larger than the `gh` budget because a large ref legitimately takes longer to
/// transfer, and it matches the 30s read timeout the desktop already sets on its
/// own HTTP client.
pub const GIT_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Run a command to completion, killing it once `timeout` elapses.
///
/// On timeout this returns [`std::io::ErrorKind::TimedOut`], so callers can tell
/// a slow call apart from a failed one — and the absorbing branches above them
/// (an `Option` return, a best-effort match arm) become reachable, which they
/// are not while every call blocks forever.
///
/// `stdin` is null and the prompt guards below are set, because a prompt is the
/// one failure this cannot bound: a tool that reads `/dev/tty` directly is not
/// stopped by closing stdin. The three variables are the documented switches for
/// their respective tools and are ignored by the other one, so a single helper
/// can set all of them.
///
/// `GIT_SSH_COMMAND` is deliberately *not* set. Forcing `BatchMode` there would
/// override a user's own `core.sshCommand` or `GIT_SSH`, which can break a
/// working ssh setup for people who need a specific identity or proxy. The kill
/// timeout still unblocks the caller; a prompt in an ssh child may linger.
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    cmd.env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;

    // Drain both pipes on their own threads, starting now.
    //
    // Draining after the child exits does not work: a command whose output
    // exceeds the pipe buffer blocks in `write` with no reader to take it, so
    // it never exits and `try_wait` never reports one. That turns a healthy
    // command into a timeout — and the larger the output, the surer the
    // failure, so `gh pr diff` on a big PR always lost this race.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || drain(stdout_pipe));
    let stderr_reader = std::thread::spawn(move || drain(stderr_pipe));

    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Deliberately not joined: a grandchild that outlived the
                    // kill can still hold the write end open, and blocking here
                    // would give back the unbounded wait this function exists
                    // to remove. The readers end with the pipe, on their own.
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("command timed out after {}s", timeout.as_secs()),
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };

    Ok(Output {
        status,
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

fn drain<R: Read>(pipe: Option<R>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut buf);
    }
    buf
}

/// `.output()`, but bounded.
///
/// Exists so a call site can gain a ceiling by changing one token instead of
/// restructuring its builder chain: `Command::new("gh").args(..).current_dir(..)`
/// evaluates to `&mut Command`, so `.output_timed(GH_TIMEOUT)` resolves here the
/// same way `.output()` resolves to std.
pub trait CommandTimeoutExt {
    fn output_timed(&mut self, timeout: Duration) -> std::io::Result<Output>;
}

impl CommandTimeoutExt for Command {
    fn output_timed(&mut self, timeout: Duration) -> std::io::Result<Output> {
        run_with_timeout(self, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_output_of_a_successful_command() {
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        let out = run_with_timeout(&mut cmd, GH_TIMEOUT).expect("ran");
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
    }

    #[test]
    fn kills_a_command_that_outlives_its_timeout() {
        // The bug this exists for: a hung call must become an error, not an
        // unbounded wait. `sleep` stands in for a stalled network round trip.
        let start = Instant::now();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let err = run_with_timeout(&mut cmd, Duration::from_millis(150))
            .expect_err("a 30s sleep must not outlive a 150ms budget");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        // Bounded by the timeout, not by the command's own runtime.
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the call must return promptly, took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn reports_a_nonzero_exit_rather_than_erroring() {
        // A failing command is a result, not a transport error — callers read
        // `status` to decide whether to fall back.
        let mut cmd = Command::new("false");
        let out = run_with_timeout(&mut cmd, GH_TIMEOUT).expect("ran");
        assert!(!out.status.success());
    }

    #[test]
    fn the_extension_trait_bounds_a_builder_chain() {
        // The shape every migrated call site uses: a chain that evaluates to
        // `&mut Command`, then `.output_timed(..)` in place of `.output()`.
        let start = Instant::now();
        let err = Command::new("sleep")
            .arg("30")
            .output_timed(Duration::from_millis(150))
            .expect_err("must not outlive the budget");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn returns_output_larger_than_the_pipe_buffer() {
        // A child that writes past the OS pipe buffer (~64 KB on macOS) blocks
        // in `write` until someone drains the read end. Nothing here reads
        // until `try_wait` reports an exit, so the child never exits and a
        // perfectly healthy command is reported as a timeout.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "head -c 200000 /dev/zero"]);
        let out = run_with_timeout(&mut cmd, GH_TIMEOUT)
            .expect("a 200 KB result is not a stalled command");
        assert_eq!(out.stdout.len(), 200_000);
    }

    #[test]
    fn sets_the_prompt_guards() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo $GH_PROMPT_DISABLED$GIT_TERMINAL_PROMPT"]);
        let out = run_with_timeout(&mut cmd, GH_TIMEOUT).expect("ran");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "10");
    }
}
