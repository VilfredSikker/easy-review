//! OS handoffs that are not agent runs: the clipboard, the editor and the
//! uninstall helper's detached shell.
//!
//! `er::agent_spawn` treats this module as the engine's one place to launch a
//! program other than an agent. Every call is a user-visible handoff, so it
//! takes no agent slot.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Context;

/// Copy `text` to the system clipboard.
pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("pbcopy", vec![])
    } else if cfg!(target_os = "windows") {
        ("clip", vec![])
    } else {
        // Linux — try xclip, fall back to xsel
        if Command::new("which")
            .arg("xclip")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            ("xclip", vec!["-selection", "clipboard"])
        } else {
            ("xsel", vec!["--clipboard", "--input"])
        }
    };

    let mut child = Command::new(cmd)
        .args(&args)
        .stdin(Stdio::piped())
        .spawn()
        .context("Failed to open clipboard command")?;

    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(text.as_bytes())?;
    }

    child.wait().context("Clipboard command failed")?;
    Ok(())
}

/// Open `file_path` at `line` in the user's editor.
pub fn open_editor(repo_root: &str, file_path: &Path, line: usize) -> anyhow::Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "code".to_string());
    let mut cmd = Command::new(&editor);
    if editor.contains("code") || editor.contains("cursor") {
        cmd.arg(repo_root)
            .arg("-g")
            .arg(format!("{}:{}", file_path.display(), line));
    } else if editor.contains("zed") {
        cmd.arg(repo_root)
            .arg(format!("{}:{}", file_path.display(), line));
    } else {
        cmd.arg(format!("+{line}")).arg(file_path);
    }

    cmd.spawn().context("Failed to open editor")?;
    Ok(())
}

/// Start a detached shell script that outlives this process.
#[cfg(unix)]
pub fn spawn_deferred_removal_sh(script: &str) -> std::io::Result<()> {
    Command::new("sh")
        .arg("-c")
        .arg(format!("( {script} ) >/dev/null 2>&1 &"))
        .spawn()?;
    Ok(())
}

/// Start a detached PowerShell script that outlives this process.
#[cfg(windows)]
pub fn spawn_deferred_removal_powershell(script: &str) -> std::io::Result<()> {
    Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
        .spawn()?;
    Ok(())
}

/// The process start time used to tell a reused PID apart, on unix.
#[cfg(unix)]
pub fn process_start_token(pid: u32) -> Option<String> {
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
