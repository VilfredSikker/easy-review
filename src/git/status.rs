use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Validate that a path (after joining with repo_root) stays within the repo root.
/// Returns the resolved path or an error if it escapes.
fn validate_within_repo(repo_root: &str, rel_path: &str) -> Result<PathBuf> {
    let root = std::fs::canonicalize(repo_root)
        .unwrap_or_else(|_| PathBuf::from(repo_root));
    let joined = Path::new(repo_root).join(rel_path);
    let resolved = std::fs::canonicalize(&joined)
        .unwrap_or_else(|_| joined.clone());
    if !resolved.starts_with(&root) {
        anyhow::bail!("Path '{}' escapes repository root", rel_path);
    }
    Ok(resolved)
}

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
    Unmerged,
}

impl FileStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            FileStatus::Added => "+",
            FileStatus::Modified => "~",
            FileStatus::Deleted => "-",
            FileStatus::Renamed(_) => "R",
            FileStatus::Copied(_) => "C",
            FileStatus::Unmerged => "!",
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

    // TODO(risk:medium): if `git rev-parse --abbrev-ref HEAD` fails (e.g., empty repo with
    // no commits), unwrap_or_default() returns "". Then `branch != current` is trivially true
    // for any branch, and the code may select an upstream ref that is also empty (""), which
    // passes the `!branch.is_empty()` guard only by accident. In a totally empty repo the
    // function falls through to returning "main" as a hard-coded guess, which is fine — but
    // the failure to read HEAD is silently swallowed without any log or warning.
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
        if *candidate != current
            && run(&["rev-parse", "--verify", candidate]).is_some() {
                return Ok(candidate.to_string());
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
    // TODO(risk:high): `base` is an unsanitized branch/ref name that comes from user input
    // (--pr flag, config file, or auto-detection). A value like "--output=/tmp/evil" or
    // "-O/tmp/evil" injected as the base branch would be concatenated into merge_base_ref and
    // passed to git diff as a positional argument, not an option. However, if base itself is
    // used as an arg in the args array directly (not as part of merge_base_ref), git would
    // interpret leading dashes as flags. Confirm all callers sanitize the base value and
    // never pass it raw through untrusted channels (e.g., branch names from `gh api` output).
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
    // TODO(risk:medium): `path` comes from DiffFile.path which is parsed from git diff output
    // and not re-validated. The "--" separator protects against path values starting with "-"
    // being interpreted as flags, but a path containing null bytes could cause unexpected
    // behavior depending on OS behavior for Command::arg(). This is low-likelihood but
    // worth auditing if file paths are ever sourced from external input.
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
    // TODO(risk:medium): `path` is an untracked file path from `git ls-files --others`.
    // If the path contains a newline character (unusual but possible on some filesystems),
    // it would break the synthetic diff format and corrupt parse_diff()'s state machine,
    // potentially causing the wrong file to be shown or a panic on subsequent byte-offset slicing.
    diff.push_str(&format!("diff --git a/{path} b/{path}\n"));
    diff.push_str("new file mode 100644\n");
    diff.push_str("--- /dev/null\n");
    diff.push_str(&format!("+++ b/{path}\n"));
    // TODO(risk:minor): if `content` has no trailing newline, .lines() drops the last implicit
    // empty line and `count` underreports by zero lines — correct. However, if content ends with
    // "\n\n", the last empty line IS counted by .lines() in some Rust versions. The hunk header
    // count will then match, but the final diff line will be an empty "+\n" which is valid.
    // Harmless but worth being aware of if the diff is later round-tripped through a strict parser.
    diff.push_str(&format!("@@ -0,0 +1,{count} @@\n"));
    for line in &lines {
        diff.push_str(&format!("+{line}\n"));
    }
    diff
}

// ── Conflicts ──

/// List files with unresolved merge conflicts
pub fn unmerged_files(repo_root: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(repo_root)
        .output()
        .context("Failed to list unmerged files")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().filter(|l| !l.is_empty()).map(String::from).collect())
}

/// Check if a merge is currently in progress (MERGE_HEAD exists)
pub fn is_merge_in_progress(repo_root: &str) -> bool {
    // Use git rev-parse --git-dir to find the correct .git directory (handles worktrees)
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(repo_root)
        .output();
    let git_dir = match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => return false,
    };

    // git_dir may be relative to repo_root
    let git_dir_path = if std::path::Path::new(&git_dir).is_absolute() {
        std::path::PathBuf::from(&git_dir)
    } else {
        std::path::Path::new(repo_root).join(&git_dir)
    };

    git_dir_path.join("MERGE_HEAD").exists()
}

/// Get a combined unified diff representing the full merge changeset.
///
/// Combines:
/// 1. Staged changes (resolved/auto-merged files) via `git diff --cached HEAD`
/// 2. Working-tree changes for each unmerged file via `git diff HEAD -- <file>`
///
/// This ensures the Conflicts view shows the complete picture — not just files
/// that still have conflict markers.
pub fn git_diff_conflicts(repo_root: &str) -> Result<String> {
    let mut combined = String::new();

    // Part 1: staged changes (resolved and auto-merged files)
    let staged_output = Command::new("git")
        .args(["diff", "--cached", "HEAD", "--unified=3", "--no-color", "--no-ext-diff"])
        .current_dir(repo_root)
        .output()
        .context("Failed to run git diff --cached HEAD")?;

    let staged_stderr = String::from_utf8_lossy(&staged_output.stderr);
    if !staged_stderr.is_empty() && !staged_output.status.success() {
        anyhow::bail!("git diff --cached HEAD failed: {}", staged_stderr.trim());
    }
    combined.push_str(&String::from_utf8_lossy(&staged_output.stdout));

    // Part 2: working-tree diff for each unmerged (conflict) file
    let unmerged = unmerged_files(repo_root)?;
    for file in &unmerged {
        let output = Command::new("git")
            .args(["diff", "HEAD", "--unified=3", "--no-color", "--no-ext-diff", "--", file])
            .current_dir(repo_root)
            .output()
            .with_context(|| format!("Failed to run git diff HEAD for conflict file: {}", file))?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() && !output.status.success() {
            anyhow::bail!("git diff HEAD failed for {}: {}", file, stderr.trim());
        }
        combined.push_str(&String::from_utf8_lossy(&output.stdout));
    }

    Ok(combined)
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

/// Commit staged changes with the given message
pub fn git_commit(repo_root: &str, message: &str) -> Result<()> {
    // TODO(risk:minor): `message` is passed as a separate argument to Command::args, so shell
    // injection is not possible. However, a message containing only whitespace or starting with
    // "#" will cause git to reject the commit with a cryptic error. Consider validating that
    // message is non-empty and non-whitespace before invoking git.
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

// ── History (commit log + commit diffs) ──

/// Get commit log for the branch (relative to base), skipping `skip` commits.
pub fn git_log_branch(base: &str, repo_root: &str, limit: usize, skip: usize) -> Result<Vec<CommitInfo>> {
    // TODO(risk:medium): if the first `git log` call fails (non-zero exit), the code silently
    // falls back to `git log` without the range — logging all commits in the repo rather than
    // just branch commits. The failure is swallowed and the caller receives a full repo history
    // with no indication that the range was ignored. This can be very misleading in History mode.
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
    // TODO(risk:medium): `hash` comes from CommitInfo.hash which is parsed from git log output.
    // While git log output is trusted, if a short hash is used and later passed here in an
    // ambiguous state (multiple objects share the prefix), git may print a disambiguation error
    // to stderr and fail. The fallback to diff-tree also takes hash directly. Ensure only full
    // 40-character hashes are stored in CommitInfo.hash (the format string uses %H, so this
    // should be safe, but the short_hash field must never be used here).
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
    let canonical_root = std::fs::canonicalize(repo_root)
        .unwrap_or_else(|_| PathBuf::from(repo_root));
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
                // Validate path stays within repo root (prevents ".." traversal)
                let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if !canonical.starts_with(&canonical_root) {
                    continue;
                }
                let rel_path = path
                    .strip_prefix(repo_root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                let metadata = match std::fs::metadata(&path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                // TODO(risk:minor): metadata.modified() returns Err on platforms that don't
                // support mtime (e.g., some WASM/embedded targets). unwrap_or(UNIX_EPOCH)
                // silently makes all files appear equally old, defeating the recency sort.
                // On supported platforms (macOS/Linux) this is fine in practice.
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
    let src = validate_within_repo(repo_root, rel_path)?;
    let dst = Path::new(repo_root).join(".er-snapshots").join(rel_path);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

/// Read the content of a watched file, returning None if binary
pub fn read_watched_file_content(repo_root: &str, rel_path: &str) -> Result<Option<String>> {
    let full_path = validate_within_repo(repo_root, rel_path)?;
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
    let current_path = validate_within_repo(repo_root, rel_path)?;
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

    #[test]
    fn file_status_symbol_unmerged() {
        assert_eq!(FileStatus::Unmerged.symbol(), "!");
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
