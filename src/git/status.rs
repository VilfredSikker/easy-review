use super::diff::{DiffHunk, LineType};
use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

/// Metadata for a single commit (used in History mode)
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub short_hash: String,
    pub subject: String,
    pub author: String,
    #[allow(dead_code)]
    pub date: String,
    pub relative_date: String,
    #[allow(dead_code)]
    pub file_count: usize,
    #[allow(dead_code)]
    pub adds: usize,
    #[allow(dead_code)]
    pub dels: usize,
    pub is_merge: bool,
}

/// A git worktree entry
#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
}

/// File change status in git
#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed(String), // old path
    #[allow(dead_code)]
    Copied(String),
}

impl FileStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            FileStatus::Added => "+",
            FileStatus::Modified => "~",
            FileStatus::Deleted => "-",
            FileStatus::Renamed(_) => "R",
            FileStatus::Copied(_) => "C",
        }
    }
}

// ── Repo Info ──

/// Get the repository root directory
pub fn get_repo_root() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to run git")?;

    if !output.status.success() {
        anyhow::bail!("Not in a git repository");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the repository root directory for a specific path
pub fn get_repo_root_in(dir: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .context(format!("Failed to run git in '{}'", dir))?;

    if !output.status.success() {
        anyhow::bail!("Not a git repository: {}", dir);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the current branch name
#[allow(dead_code)]
pub fn get_current_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to get current branch")?;

    if !output.status.success() {
        anyhow::bail!("Failed to determine current branch");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get current branch for a specific repo root
pub fn get_current_branch_in(repo_root: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_root)
        .output()
        .context("Failed to get current branch")?;

    if !output.status.success() {
        anyhow::bail!("Failed to determine current branch");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Auto-detect the base branch by checking upstream tracking, then falling
/// back to common names (main, master, develop).
#[allow(dead_code)]
pub fn detect_base_branch() -> Result<String> {
    detect_base_branch_impl(None)
}

/// Auto-detect base branch for a specific repo root
pub fn detect_base_branch_in(repo_root: &str) -> Result<String> {
    detect_base_branch_impl(Some(repo_root))
}

fn detect_base_branch_impl(repo_root: Option<&str>) -> Result<String> {
    // Helper: run a git command and return trimmed stdout on success
    let run = |args: &[&str]| -> Option<String> {
        let mut cmd = Command::new("git");
        cmd.args(args);
        if let Some(root) = repo_root {
            cmd.current_dir(root);
        }
        let out = cmd.output().ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    };

    let current = run(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();

    // Try upstream tracking branch
    if let Some(upstream) = run(&["rev-parse", "--abbrev-ref", "@{upstream}"]) {
        // Strip remote name prefix (first component) to get the branch name.
        // e.g. "origin/stack/foo" → "stack/foo", "origin/main" → "main"
        let branch = upstream.find('/').map(|i| &upstream[i + 1..]).unwrap_or(&upstream);
        if branch != current && !branch.is_empty() {
            // Verify the short name is a valid revision
            if run(&["rev-parse", "--verify", branch]).is_some() {
                return Ok(branch.to_string());
            }
            // Fall back to the full upstream ref (e.g. origin/main)
            if run(&["rev-parse", "--verify", &upstream]).is_some() {
                return Ok(upstream);
            }
        }
    }

    // Common local branch names
    for candidate in &["main", "master", "develop", "dev"] {
        if *candidate != current {
            if run(&["rev-parse", "--verify", candidate]).is_some() {
                return Ok(candidate.to_string());
            }
        }
    }

    // Remote-tracking branches as last resort
    for candidate in &["origin/main", "origin/master", "origin/develop"] {
        if run(&["rev-parse", "--verify", candidate]).is_some() {
            return Ok(candidate.to_string());
        }
    }

    Ok("main".to_string())
}

// ── Diff ──

/// Get the raw diff output from git for a given mode
pub fn git_diff_raw(mode: &str, base: &str, repo_root: &str) -> Result<String> {
    let merge_base_ref = format!("{}...HEAD", base);
    let args: Vec<&str> = match mode {
        "branch" => vec!["diff", &merge_base_ref, "--unified=3", "--no-color", "--no-ext-diff"],
        "unstaged" => vec!["diff", "--unified=3", "--no-color", "--no-ext-diff"],
        "staged" => vec!["diff", "--staged", "--unified=3", "--no-color", "--no-ext-diff"],
        _ => anyhow::bail!("Unknown diff mode: {}", mode),
    };

    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_root)
        .output()
        .context("Failed to run git diff")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if std::env::var("ER_DEBUG").is_ok() {
        let debug = format!(
            "cmd: git {}\ncwd: {}\nexit: {:?}\nstdout_len: {}\nstderr: {}\nstdout_first_200: {}\n",
            args.join(" "),
            repo_root,
            output.status.code(),
            stdout.len(),
            stderr,
            &stdout[..stdout.len().min(200)],
        );
        let _ = std::fs::write("/tmp/er_debug.log", debug);
    }

    if !stderr.is_empty() && !output.status.success() {
        anyhow::bail!("git diff failed: {}", stderr.trim());
    }

    // For unstaged mode, append synthetic diffs for untracked files
    if mode == "unstaged" {
        let untracked = untracked_files(repo_root)?;
        if !untracked.is_empty() {
            let mut combined = stdout;
            for path in untracked {
                if let Ok(content) = std::fs::read_to_string(
                    std::path::Path::new(repo_root).join(&path),
                ) {
                    combined.push_str(&synthetic_new_file_diff(&path, &content));
                }
            }
            return Ok(combined);
        }
    }

    Ok(stdout)
}

/// Get the raw diff output for a single file
pub fn git_diff_raw_file(mode: &str, base: &str, repo_root: &str, path: &str) -> Result<String> {
    let merge_base_ref = format!("{}...HEAD", base);
    let mut args: Vec<&str> = match mode {
        "branch" => vec!["diff", &merge_base_ref, "--unified=3", "--no-color", "--no-ext-diff", "--"],
        "unstaged" => vec!["diff", "--unified=3", "--no-color", "--no-ext-diff", "--"],
        "staged" => vec!["diff", "--staged", "--unified=3", "--no-color", "--no-ext-diff", "--"],
        _ => anyhow::bail!("Unknown diff mode: {}", mode),
    };
    args.push(path);

    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_root)
        .output()
        .context("Failed to run git diff for single file")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !stderr.is_empty() && !output.status.success() {
        anyhow::bail!("git diff failed for {}: {}", path, stderr.trim());
    }

    Ok(stdout)
}

/// List untracked files (excluding gitignored)
fn untracked_files(repo_root: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(repo_root)
        .output()
        .context("Failed to list untracked files")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().filter(|l| !l.is_empty()).map(String::from).collect())
}

/// Build a unified diff for a new untracked file
fn synthetic_new_file_diff(path: &str, content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let count = lines.len();
    let mut diff = String::new();
    diff.push_str(&format!("diff --git a/{path} b/{path}\n"));
    diff.push_str("new file mode 100644\n");
    diff.push_str("--- /dev/null\n");
    diff.push_str(&format!("+++ b/{path}\n"));
    diff.push_str(&format!("@@ -0,0 +1,{count} @@\n"));
    for line in &lines {
        diff.push_str(&format!("+{line}\n"));
    }
    diff
}

// ── Worktrees ──

/// List all git worktrees for the repo
pub fn list_worktrees(repo_root: &str) -> Result<Vec<Worktree>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .context("Failed to list worktrees")?;

    if !output.status.success() {
        anyhow::bail!("git worktree list failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();
    let mut current_path = String::new();
    let mut current_branch = String::new();

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            // Save previous entry
            if !current_path.is_empty() {
                worktrees.push(Worktree {
                    path: current_path.clone(),
                    branch: if current_branch.is_empty() {
                        "(detached)".to_string()
                    } else {
                        current_branch.clone()
                    },
                });
            }
            current_path = path.to_string();
            current_branch.clear();
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = branch.to_string();
        } else if line == "detached" {
            current_branch = "(detached)".to_string();
        }
    }

    // Don't forget the last entry
    if !current_path.is_empty() {
        worktrees.push(Worktree {
            path: current_path,
            branch: if current_branch.is_empty() {
                "(detached)".to_string()
            } else {
                current_branch
            },
        });
    }

    Ok(worktrees)
}

/// Check if a path is a git repository
#[allow(dead_code)]
pub fn is_git_repo(path: &str) -> bool {
    std::path::Path::new(path).join(".git").exists()
}

// ── Staging ──

/// Stage a single file
pub fn git_stage_file(repo_root: &str, file_path: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["add", "--", file_path])
        .current_dir(repo_root)
        .output()
        .context("Failed to stage file")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git add failed: {}", stderr.trim());
    }
    Ok(())
}

/// Unstage a single file
pub fn git_unstage_file(repo_root: &str, file_path: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["reset", "HEAD", "--", file_path])
        .current_dir(repo_root)
        .output()
        .context("Failed to unstage file")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset failed: {}", stderr.trim());
    }
    Ok(())
}

/// Stage all files
#[allow(dead_code)]
pub fn git_stage_all(repo_root: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo_root)
        .output()
        .context("Failed to stage all")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git add -A failed: {}", stderr.trim());
    }
    Ok(())
}

/// Stage a specific hunk by reconstructing a patch and piping to `git apply --cached`
pub fn git_stage_hunk(repo_root: &str, file_path: &str, hunk: &DiffHunk) -> Result<()> {
    let patch = reconstruct_hunk_patch(file_path, hunk);

    let mut child = Command::new("git")
        .args(["apply", "--cached", "--unidiff-zero"])
        .current_dir(repo_root)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn git apply")?;

    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(patch.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stage hunk: {}", stderr.trim());
    }

    Ok(())
}

/// Commit staged changes with the given message
pub fn git_commit(repo_root: &str, message: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo_root)
        .output()
        .context("Failed to run git commit")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git commit failed: {}", stderr.trim());
    }
    Ok(())
}

/// Quote a git path for use in patch headers.
/// Paths containing whitespace or double-quotes must be wrapped in double-quotes
/// so that `git apply` parses them correctly.
fn quote_git_path(path: &str) -> String {
    if path.contains(|c: char| c.is_whitespace() || c == '"') {
        format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        path.to_string()
    }
}

/// Reconstruct a minimal unified diff patch from a single DiffHunk
fn reconstruct_hunk_patch(file_path: &str, hunk: &DiffHunk) -> String {
    let quoted = quote_git_path(file_path);
    let mut patch = String::new();
    patch.push_str(&format!("diff --git a/{} b/{}\n", quoted, quoted));
    patch.push_str(&format!("--- a/{}\n", quoted));
    patch.push_str(&format!("+++ b/{}\n", quoted));
    patch.push_str(&hunk.header);
    patch.push('\n');

    for line in &hunk.lines {
        let prefix = match line.line_type {
            LineType::Add => "+",
            LineType::Delete => "-",
            LineType::Context => " ",
        };
        patch.push_str(prefix);
        patch.push_str(&line.content);
        patch.push('\n');
    }

    patch
}

// ── History (commit log + commit diffs) ──

/// Get commit log for the branch (relative to base), skipping `skip` commits.
pub fn git_log_branch(base: &str, repo_root: &str, limit: usize, skip: usize) -> Result<Vec<CommitInfo>> {
    let range = format!("{}..HEAD", base);
    let format_str = "--format=%H\x1e%h\x1e%s\x1e%an\x1e%aI\x1e%ar\x1e%P";
    let limit_str = format!("--max-count={}", limit);
    let skip_str = format!("--skip={}", skip);

    let output = Command::new("git")
        .args(["log", &range, &limit_str, &skip_str, format_str, "--shortstat"])
        .current_dir(repo_root)
        .output()
        .context("Failed to run git log")?;

    if !output.status.success() {
        // Might be on a detached HEAD or base doesn't exist — try without range
        let output = Command::new("git")
            .args(["log", &limit_str, &skip_str, format_str, "--shortstat"])
            .current_dir(repo_root)
            .output()
            .context("Failed to run git log")?;

        return parse_git_log(&String::from_utf8_lossy(&output.stdout));
    }

    parse_git_log(&String::from_utf8_lossy(&output.stdout))
}

/// Parse the output of `git log --format=... --shortstat`
///
/// The format string uses `\x1e` (ASCII record separator) as the field delimiter,
/// so each commit's metadata appears on one line regardless of subject content.
/// `--shortstat` output follows on the next line(s), still newline-separated.
fn parse_git_log(output: &str) -> Result<Vec<CommitInfo>> {
    let mut commits = Vec::new();
    let mut lines = output.lines().peekable();

    while let Some(line) = lines.next() {
        // Format lines are identified by the \x1e delimiter we injected
        if !line.contains('\x1e') {
            continue;
        }

        let parts: Vec<&str> = line.split('\x1e').collect();
        if parts.len() < 7 {
            continue;
        }

        let hash = parts[0].to_string();
        let short_hash = parts[1].to_string();
        let subject = parts[2].to_string();
        let author = parts[3].to_string();
        let date = parts[4].to_string();
        let relative_date = parts[5].to_string();
        let parents = parts[6];
        let is_merge = parents.split_whitespace().count() > 1;

        // Parse the optional --shortstat line that follows (absent for empty commits)
        let (file_count, adds, dels) = if let Some(next) = lines.peek() {
            if !next.contains('\x1e') && !next.trim().is_empty() {
                let stat_line = lines.next().unwrap();
                parse_shortstat(stat_line)
            } else {
                (0, 0, 0)
            }
        } else {
            (0, 0, 0)
        };

        commits.push(CommitInfo {
            hash,
            short_hash,
            subject,
            author,
            date,
            relative_date,
            file_count,
            adds,
            dels,
            is_merge,
        });
    }

    Ok(commits)
}

/// Parse a --shortstat line like " 3 files changed, 45 insertions(+), 12 deletions(-)"
fn parse_shortstat(line: &str) -> (usize, usize, usize) {
    let mut file_count = 0;
    let mut adds = 0;
    let mut dels = 0;

    let parts: Vec<&str> = line.split(',').collect();
    for part in parts {
        let trimmed = part.trim();
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.len() >= 2 {
            if let Ok(n) = tokens[0].parse::<usize>() {
                if trimmed.contains("file") {
                    file_count = n;
                } else if trimmed.contains("insertion") {
                    adds = n;
                } else if trimmed.contains("deletion") {
                    dels = n;
                }
            }
        }
    }

    (file_count, adds, dels)
}

/// Get the diff for a single commit, handling merge commits and root commits
pub fn git_diff_commit(hash: &str, repo_root: &str) -> Result<String> {
    // Try diff against first parent
    let output = Command::new("git")
        .args([
            "diff",
            &format!("{}^..{}", hash, hash),
            "--unified=3",
            "--no-color",
            "--no-ext-diff",
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to run git diff for commit")?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    // Fallback: might be the initial commit (no parent) — use diff-tree --root
    let output = Command::new("git")
        .args([
            "diff-tree",
            "-p",
            "--root",
            "--unified=3",
            "--no-color",
            "--no-ext-diff",
            hash,
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to run git diff-tree for root commit")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ── Watched Files ──

/// A git-ignored file opted into visibility via .er-config.toml
#[derive(Debug, Clone)]
pub struct WatchedFile {
    pub path: String,
    pub modified: SystemTime,
    pub size: u64,
}

/// Discover watched files matching glob patterns relative to repo root
pub fn discover_watched_files(repo_root: &str, patterns: &[String]) -> Result<Vec<WatchedFile>> {
    let mut files = Vec::new();
    for pattern in patterns {
        let full_pattern = format!("{}/{}", repo_root, pattern);
        let entries = glob::glob(&full_pattern)
            .with_context(|| format!("Invalid glob pattern: {}", pattern))?;
        for entry in entries {
            let path = match entry {
                Ok(p) => p,
                Err(_) => continue,
            };
            if path.is_file() {
                let rel_path = path
                    .strip_prefix(repo_root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                let metadata = match std::fs::metadata(&path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let size = metadata.len();
                files.push(WatchedFile {
                    path: rel_path,
                    modified,
                    size,
                });
            }
        }
    }
    // Sort by modification time (most recent first)
    files.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(files)
}

/// Check if a path is gitignored
pub fn verify_gitignored(repo_root: &str, path: &str) -> bool {
    let output = Command::new("git")
        .args(["check-ignore", "-q", path])
        .current_dir(repo_root)
        .output();
    matches!(output, Ok(o) if o.status.success())
}

/// Save a snapshot of a watched file for later diffing
pub fn save_snapshot(repo_root: &str, rel_path: &str) -> Result<()> {
    let src = Path::new(repo_root).join(rel_path);
    let dst = Path::new(repo_root).join(".er-snapshots").join(rel_path);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

/// Read the content of a watched file, returning None if binary
pub fn read_watched_file_content(repo_root: &str, rel_path: &str) -> Result<Option<String>> {
    let full_path = Path::new(repo_root).join(rel_path);
    let bytes = std::fs::read(&full_path)
        .with_context(|| format!("Failed to read watched file: {}", rel_path))?;

    // Check for binary content (null bytes in first 8KB)
    let check_len = bytes.len().min(8192);
    if bytes[..check_len].contains(&0) {
        return Ok(None);
    }

    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| anyhow::anyhow!("Watched file is not valid UTF-8: {}", rel_path))
}

/// Diff a watched file against its snapshot using git diff --no-index
pub fn diff_watched_file_snapshot(repo_root: &str, rel_path: &str) -> Result<Option<String>> {
    let current_path = Path::new(repo_root).join(rel_path);
    let snapshot_path = Path::new(repo_root).join(".er-snapshots").join(rel_path);

    if !snapshot_path.exists() {
        // First time seeing this file — save snapshot, signal "new file"
        save_snapshot(repo_root, rel_path)?;
        return Ok(None);
    }

    let output = Command::new("git")
        .args([
            "diff", "--no-index", "--unified=3", "--no-color", "--no-ext-diff",
        ])
        .arg(&snapshot_path)
        .arg(&current_path)
        .current_dir(repo_root)
        .output()
        .context("Failed to run git diff --no-index")?;

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    if raw.is_empty() {
        return Ok(Some(String::new())); // No changes since snapshot
    }

    Ok(Some(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::diff::{DiffHunk, DiffLine, LineType};

    // ── FileStatus::symbol ──

    #[test]
    fn file_status_symbol_added() {
        assert_eq!(FileStatus::Added.symbol(), "+");
    }

    #[test]
    fn file_status_symbol_modified() {
        assert_eq!(FileStatus::Modified.symbol(), "~");
    }

    #[test]
    fn file_status_symbol_deleted() {
        assert_eq!(FileStatus::Deleted.symbol(), "-");
    }

    #[test]
    fn file_status_symbol_renamed() {
        assert_eq!(FileStatus::Renamed("old.rs".to_string()).symbol(), "R");
    }

    #[test]
    fn file_status_symbol_copied() {
        assert_eq!(FileStatus::Copied("old.rs".to_string()).symbol(), "C");
    }

    // ── reconstruct_hunk_patch ──

    fn make_hunk(header: &str, lines: Vec<DiffLine>) -> DiffHunk {
        DiffHunk {
            header: header.to_string(),
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 4,
            lines,
        }
    }

    #[test]
    fn reconstruct_hunk_patch_simple_add() {
        let hunk = make_hunk(
            "@@ -1,1 +1,2 @@ fn main()",
            vec![
                DiffLine {
                    line_type: LineType::Context,
                    content: "fn main() {".to_string(),
                    old_num: Some(1),
                    new_num: Some(1),
                },
                DiffLine {
                    line_type: LineType::Add,
                    content: "    println!(\"hello\");".to_string(),
                    old_num: None,
                    new_num: Some(2),
                },
            ],
        );

        let patch = reconstruct_hunk_patch("main.rs", &hunk);

        assert_eq!(
            patch,
            "diff --git a/main.rs b/main.rs\n\
             --- a/main.rs\n\
             +++ b/main.rs\n\
             @@ -1,1 +1,2 @@ fn main()\n\
             \x20fn main() {\n\
             +    println!(\"hello\");\n"
        );
    }

    #[test]
    fn reconstruct_hunk_patch_mixed_add_delete_context() {
        let hunk = make_hunk(
            "@@ -1,3 +1,3 @@",
            vec![
                DiffLine {
                    line_type: LineType::Context,
                    content: "let x = 1;".to_string(),
                    old_num: Some(1),
                    new_num: Some(1),
                },
                DiffLine {
                    line_type: LineType::Delete,
                    content: "let y = 2;".to_string(),
                    old_num: Some(2),
                    new_num: None,
                },
                DiffLine {
                    line_type: LineType::Add,
                    content: "let y = 42;".to_string(),
                    old_num: None,
                    new_num: Some(2),
                },
                DiffLine {
                    line_type: LineType::Context,
                    content: "let z = 3;".to_string(),
                    old_num: Some(3),
                    new_num: Some(3),
                },
            ],
        );

        let patch = reconstruct_hunk_patch("lib.rs", &hunk);

        let lines: Vec<&str> = patch.lines().collect();
        assert_eq!(lines[4], " let x = 1;");
        assert_eq!(lines[5], "-let y = 2;");
        assert_eq!(lines[6], "+let y = 42;");
        assert_eq!(lines[7], " let z = 3;");
    }

    #[test]
    fn reconstruct_hunk_patch_only_deletions() {
        let hunk = make_hunk(
            "@@ -1,2 +1,0 @@",
            vec![
                DiffLine {
                    line_type: LineType::Delete,
                    content: "fn old() {}".to_string(),
                    old_num: Some(1),
                    new_num: None,
                },
                DiffLine {
                    line_type: LineType::Delete,
                    content: "fn also_old() {}".to_string(),
                    old_num: Some(2),
                    new_num: None,
                },
            ],
        );

        let patch = reconstruct_hunk_patch("old.rs", &hunk);

        let lines: Vec<&str> = patch.lines().collect();
        assert_eq!(lines[4], "-fn old() {}");
        assert_eq!(lines[5], "-fn also_old() {}");
        // Only content lines (after the 4-line header) should be checked — none should be additions
        assert!(lines[4..].iter().all(|l| !l.starts_with('+')));
    }

    #[test]
    fn reconstruct_hunk_patch_file_path_with_directory() {
        let hunk = make_hunk(
            "@@ -1,1 +1,1 @@",
            vec![DiffLine {
                line_type: LineType::Add,
                content: "pub fn foo() {}".to_string(),
                old_num: None,
                new_num: Some(1),
            }],
        );

        let patch = reconstruct_hunk_patch("src/lib/foo.rs", &hunk);

        assert!(patch.contains("diff --git a/src/lib/foo.rs b/src/lib/foo.rs\n"));
        assert!(patch.contains("--- a/src/lib/foo.rs\n"));
        assert!(patch.contains("+++ b/src/lib/foo.rs\n"));
    }

    #[test]
    fn reconstruct_hunk_patch_path_with_spaces() {
        let hunk = make_hunk(
            "@@ -1,1 +1,1 @@",
            vec![DiffLine {
                line_type: LineType::Add,
                content: "fn hello() {}".to_string(),
                old_num: None,
                new_num: Some(1),
            }],
        );

        let patch = reconstruct_hunk_patch("src/my file.rs", &hunk);

        // Paths with spaces must be quoted so git apply parses them correctly.
        assert!(patch.contains("diff --git a/\"src/my file.rs\" b/\"src/my file.rs\"\n"));
        assert!(patch.contains("--- a/\"src/my file.rs\"\n"));
        assert!(patch.contains("+++ b/\"src/my file.rs\"\n"));
    }

    // ── quote_git_path ──

    #[test]
    fn quote_git_path_plain_path_unchanged() {
        assert_eq!(quote_git_path("src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn quote_git_path_space_gets_quoted() {
        assert_eq!(quote_git_path("my file.rs"), "\"my file.rs\"");
    }

    #[test]
    fn quote_git_path_double_quote_in_name() {
        assert_eq!(quote_git_path("say \"hi\".rs"), "\"say \\\"hi\\\".rs\"");
    }

    // ── upstream branch name extraction (detect_base_branch) ──

    #[test]
    fn upstream_strip_simple_branch() {
        // "origin/main" → strip remote prefix → "main"
        let upstream = "origin/main";
        let branch = upstream.find('/').map(|i| &upstream[i + 1..]).unwrap_or(upstream);
        assert_eq!(branch, "main");
    }

    #[test]
    fn upstream_strip_slashed_branch() {
        // "origin/stack/foo-bar" → strip remote prefix → "stack/foo-bar"
        // This must match current branch "stack/foo-bar" to skip upstream detection
        let upstream = "origin/stack/foo-bar";
        let branch = upstream.find('/').map(|i| &upstream[i + 1..]).unwrap_or(upstream);
        assert_eq!(branch, "stack/foo-bar");
    }

    #[test]
    fn upstream_strip_deeply_nested_branch() {
        let upstream = "origin/user/feature/sub-task";
        let branch = upstream.find('/').map(|i| &upstream[i + 1..]).unwrap_or(upstream);
        assert_eq!(branch, "user/feature/sub-task");
    }

    // ── parse_shortstat ──

    #[test]
    fn parse_shortstat_typical_output() {
        let (files, adds, dels) =
            parse_shortstat(" 3 files changed, 45 insertions(+), 12 deletions(-)");
        assert_eq!(files, 3);
        assert_eq!(adds, 45);
        assert_eq!(dels, 12);
    }

    #[test]
    fn parse_shortstat_only_insertions() {
        let (files, adds, dels) =
            parse_shortstat(" 1 file changed, 10 insertions(+)");
        assert_eq!(files, 1);
        assert_eq!(adds, 10);
        assert_eq!(dels, 0);
    }

    #[test]
    fn parse_shortstat_only_deletions() {
        let (files, adds, dels) =
            parse_shortstat(" 2 files changed, 5 deletions(-)");
        assert_eq!(files, 2);
        assert_eq!(adds, 0);
        assert_eq!(dels, 5);
    }

    #[test]
    fn parse_shortstat_empty_returns_zeros() {
        let (files, adds, dels) = parse_shortstat("");
        assert_eq!(files, 0);
        assert_eq!(adds, 0);
        assert_eq!(dels, 0);
    }

    // ── parse_git_log ──

    #[test]
    fn parse_git_log_single_commit() {
        let output = "abc1234def5678901234567890abcdef12345678\x1eabc1234\x1eFix token expiry bug\x1eWill\x1e2026-02-25T10:00:00+01:00\x1e2 hours ago\x1eparenthash1234567890abcdef12345678901234\n 3 files changed, 45 insertions(+), 12 deletions(-)\n";
        let commits = parse_git_log(output).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].short_hash, "abc1234");
        assert_eq!(commits[0].subject, "Fix token expiry bug");
        assert_eq!(commits[0].author, "Will");
        assert_eq!(commits[0].relative_date, "2 hours ago");
        assert_eq!(commits[0].file_count, 3);
        assert_eq!(commits[0].adds, 45);
        assert_eq!(commits[0].dels, 12);
        assert!(!commits[0].is_merge);
    }

    #[test]
    fn parse_git_log_merge_commit_has_two_parents() {
        let output = "abc1234def5678901234567890abcdef12345678\x1eabc1234\x1eMerge branch 'feature'\x1eWill\x1e2026-02-25T10:00:00+01:00\x1e1 day ago\x1eparent1hash234567890abcdef1234567890123 parent2hash234567890abcdef1234567890123\n 5 files changed, 100 insertions(+), 50 deletions(-)\n";
        let commits = parse_git_log(output).unwrap();
        assert_eq!(commits.len(), 1);
        assert!(commits[0].is_merge);
    }

    #[test]
    fn parse_git_log_empty_input() {
        let commits = parse_git_log("").unwrap();
        assert!(commits.is_empty());
    }

    #[test]
    fn parse_git_log_multiple_commits() {
        let output = concat!(
            "aaaa1234567890abcdef1234567890abcdef1234\x1eaaaa123\x1eFirst commit\x1eAlice\x1e2026-02-25T10:00:00+01:00\x1e1 hour ago\x1eparenthash1234567890abcdef12345678901234\n",
            " 1 file changed, 10 insertions(+)\n",
            "\n",
            "bbbb1234567890abcdef1234567890abcdef1234\x1ebbbb123\x1eSecond commit\x1eBob\x1e2026-02-24T10:00:00+01:00\x1e1 day ago\x1eparenthash2234567890abcdef12345678901234\n",
            " 2 files changed, 20 insertions(+), 5 deletions(-)\n",
        );
        let commits = parse_git_log(output).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].short_hash, "aaaa123");
        assert_eq!(commits[1].short_hash, "bbbb123");
    }

    #[test]
    fn parse_git_log_subject_with_special_chars() {
        // With \x1e delimiters, subjects containing characters that would previously
        // confuse a line-count parser (colons, parentheses, numbers) are handled safely.
        // git's %s outputs only the first line of the subject, so embedded newlines
        // cannot occur in real output.
        let output = "abc123def5678901234567890abcdef12345678\x1eabc123\x1efix(auth): handle 401 errors\x1eAuthor Name\x1e2025-01-01T00:00:00Z\x1e1 hour ago\x1edef456\n 1 file changed, 5 insertions(+)\n";
        let commits = parse_git_log(output).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "fix(auth): handle 401 errors");
        assert_eq!(commits[0].author, "Author Name");
        assert_eq!(commits[0].adds, 5);
    }
}
