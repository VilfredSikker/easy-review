//! Desktop OS handoffs: the editor, the default browser and the system file
//! manager.
//!
//! `er::agent_spawn` treats this module as the desktop's one place to launch a
//! program. Every call is a user-visible handoff, so it takes no agent slot.

use std::path::Path;
use std::process::Command;

/// Open `file_path` at `line_num` in the user's editor.
pub fn open_editor_at(repo_root: &str, file_path: &Path, line_num: usize) -> anyhow::Result<()> {
    use anyhow::Context;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "code".to_string());
    let mut cmd = Command::new(&editor);
    if editor.contains("code") || editor.contains("cursor") {
        cmd.arg(repo_root)
            .arg("-g")
            .arg(format!("{}:{}", file_path.display(), line_num));
    } else if editor.contains("zed") {
        cmd.arg(repo_root)
            .arg(format!("{}:{}", file_path.display(), line_num));
    } else {
        cmd.arg(format!("+{line_num}")).arg(file_path);
    }
    cmd.spawn().context("Failed to open editor")?;
    Ok(())
}

/// Open `file_path` at `line_num` in VS Code.
pub fn open_vscode_at(repo_root: &str, file_path: &Path, line_num: usize) -> anyhow::Result<()> {
    use anyhow::Context;
    Command::new("code")
        .arg(repo_root)
        .arg("-g")
        .arg(format!("{}:{}", file_path.display(), line_num))
        .spawn()
        .context("Failed to open VS Code (is `code` on PATH?)")?;
    Ok(())
}

/// Open an http(s) URL in the system default browser (shared by the Tauri
/// command and the nav policy).
pub fn open_external_url(url: &str) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "linux") {
        Command::new("xdg-open").arg(url).spawn()
    } else {
        Command::new("cmd").args(["/c", "start", url]).spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

/// Reveal `target` in the system file manager.
pub fn reveal_folder(target: &Path) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(target).spawn()
    } else if cfg!(target_os = "linux") {
        Command::new("xdg-open").arg(target).spawn()
    } else {
        Command::new("explorer").arg(target).spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

/// Reveal `target` in the system file manager, selecting it where supported.
pub fn reveal_path(target: &Path) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg("-R").arg(target).spawn()
    } else if cfg!(target_os = "linux") {
        let parent = target.parent().unwrap_or(target);
        Command::new("xdg-open").arg(parent).spawn()
    } else {
        let arg = format!("/select,{}", target.display());
        Command::new("explorer").arg(arg).spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}
