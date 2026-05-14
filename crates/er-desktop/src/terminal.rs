//! PTY session management for the in-app terminal drawer.
//!
//! Wraps `portable-pty` so the frontend (xterm.js) can drive a real shell.
//! Each ER tab gets its own `PtySession`, keyed by a frontend-supplied
//! `session_id` (typically `tab-<idx>`). When the session is dropped (either
//! because the user closed the terminal or because the app exited), the child
//! process is killed via the `Drop` impl below.

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtySession {
    /// Spawn the user's `$SHELL` (falling back to `/bin/zsh`) in `cwd`.
    /// Returns the session plus a reader the caller drains on a background
    /// thread.
    pub fn spawn(cwd: &str) -> anyhow::Result<(Self, Box<dyn Read + Send>)> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut cmd = CommandBuilder::new(shell);
        if !cwd.is_empty() {
            cmd.cwd(cwd);
        }
        // Sane defaults for a TTY-aware shell.
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd)?;
        // Drop the slave half — the child holds its own fd. Keeping it open in
        // the parent would prevent EOF detection when the shell exits.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        Ok((
            Self {
                master: pair.master,
                writer,
                child,
            },
            reader,
        ))
    }

    pub fn write(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Best-effort: kill the child. If it's already exited, the error is
        // benign.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
