use super::diff::{DiffHunk, LineType};
use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

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
        if let Some(branch) = upstream.split('/').last() {
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
    let args: Vec<&str> = match mode {
        "branch" => vec!["diff", base, "--unified=3", "--no-color", "--no-ext-diff"],
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

    Ok(stdout)
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

/// Reconstruct a minimal unified diff patch from a single DiffHunk
fn reconstruct_hunk_patch(file_path: &str, hunk: &DiffHunk) -> String {
    let mut patch = String::new();
    patch.push_str(&format!("diff --git a/{} b/{}\n", file_path, file_path));
    patch.push_str(&format!("--- a/{}\n", file_path));
    patch.push_str(&format!("+++ b/{}\n", file_path));
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
}
