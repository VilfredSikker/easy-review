use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Check if the repo has a `.git/gitbutler/` directory (= GitButler-managed)
pub fn is_gitbutler_repo(repo_root: &str) -> bool {
    Path::new(repo_root).join(".git/gitbutler").is_dir()
}

/// Find the GitButler CLI binary. Checks PATH for `but` and `gitbutler-tauri`,
/// then falls back to the macOS app bundle location.
pub fn find_gitbutler_binary() -> Option<PathBuf> {
    // Check PATH for `but`
    if let Ok(output) = Command::new("which").arg("but").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }

    // Check PATH for `gitbutler-tauri`
    if let Ok(output) = Command::new("which").arg("gitbutler-tauri").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }

    // macOS app bundle fallback
    let app_bundle =
        PathBuf::from("/Applications/GitButler.app/Contents/MacOS/gitbutler-tauri");
    if app_bundle.exists() {
        return Some(app_bundle);
    }

    None
}

// ---------------------------------------------------------------------------
// Serde structs — `but status --json -f`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GbStatus {
    pub unassigned_changes: Vec<GbChange>,
    pub stacks: Vec<GbStack>,
    pub merge_base: GbCommit,
    pub upstream_state: Option<GbUpstream>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GbStack {
    pub cli_id: String,
    pub assigned_changes: Vec<GbChange>,
    pub branches: Vec<GbBranch>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GbBranch {
    pub cli_id: String,
    pub name: String,
    pub commits: Vec<GbCommit>,
    pub upstream_commits: Vec<GbCommit>,
    pub branch_status: String,
    pub review_id: Option<String>,
    pub ci: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GbCommit {
    pub cli_id: String,
    pub commit_id: Option<String>,
    pub created_at: String,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
    pub conflicted: Option<bool>,
    pub review_id: Option<String>,
    pub changes: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GbUpstream {
    pub behind: u64,
    pub latest_commit: GbCommit,
    pub last_fetched: String,
}

/// Placeholder for change entries — schema not yet observed with real data.
pub type GbChange = serde_json::Value;

// ---------------------------------------------------------------------------
// Serde structs — `but config --json`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct GbConfig {
    pub target_branch: String,
    // Other fields exist but we only need target_branch
}

// ---------------------------------------------------------------------------
// Context file — written to .er/gb-context.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct GbContext {
    pub enabled: bool,
    pub binary: String,
    pub selected_stack_id: String,
    pub selected_branch: String,
    pub stacks: Vec<GbContextStack>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GbContextStack {
    pub cli_id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// CLI functions
// ---------------------------------------------------------------------------

/// Run `but status --json -f` and deserialize.
pub fn gitbutler_status(binary: &Path, repo_root: &str) -> Result<GbStatus> {
    let output = Command::new(binary)
        .args(["status", "--json", "-f"])
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .context("Failed to execute GitButler CLI")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("GitButler status failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).context("Failed to parse GitButler status JSON")
}

/// Run `but diff <target>` (without --json) and return raw unified diff text.
/// If target is empty, shows all uncommitted changes.
pub fn gitbutler_diff_raw(binary: &Path, repo_root: &str, target: &str) -> Result<String> {
    let mut cmd = Command::new(binary);
    cmd.arg("diff")
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_INDEX_FILE");

    if !target.is_empty() {
        cmd.arg(target);
    }

    let output = cmd.output().context("Failed to execute GitButler diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("GitButler diff failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run `but config --json` and extract target_branch.
#[allow(dead_code)]
pub fn gitbutler_target_branch(binary: &Path, repo_root: &str) -> Result<String> {
    let output = Command::new(binary)
        .args(["config", "--json"])
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .context("Failed to execute GitButler config")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("GitButler config failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let config: GbConfig =
        serde_json::from_str(&stdout).context("Failed to parse GitButler config JSON")?;
    Ok(config.target_branch)
}

// ---------------------------------------------------------------------------
// Context file functions
// ---------------------------------------------------------------------------

/// Write .er/gb-context.json with current GitButler state.
/// Uses atomic write (tmp file + rename).
pub fn write_gb_context(er_dir: &str, ctx: &GbContext) -> Result<()> {
    let dir = Path::new(er_dir);
    std::fs::create_dir_all(dir).context("Failed to create .er/ directory")?;

    let path = dir.join("gb-context.json");
    let tmp_path = dir.join("gb-context.json.tmp");
    let json = serde_json::to_string_pretty(ctx).context("Failed to serialize GbContext")?;
    std::fs::write(&tmp_path, json).context("Failed to write gb-context.json.tmp")?;
    std::fs::rename(&tmp_path, &path).context("Failed to rename gb-context.json.tmp")?;
    Ok(())
}

/// Compute per-stack .er/ directory path: .er/stacks/<sanitized_branch_name>/
/// Sanitizes branch name for filesystem safety.
pub fn gb_er_dir(er_dir: &str, branch_name: &str) -> String {
    let sanitized: String = branch_name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ' ' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            _ => c,
        })
        .collect();

    format!("{}/stacks/{}", er_dir, sanitized)
}
