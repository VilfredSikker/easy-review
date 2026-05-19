/// Desktop-managed storage for review artifacts.
///
/// Flat branch-level layout under
/// `<app_data>/easy-review/repos/<repo_slug>/branches/<branch_slug>/`:
///
/// ```text
/// review.json
/// order.json
/// checklist.json
/// summary.md
/// questions.json
/// github-comments.json
/// reviewed
/// diff-tmp, diff-annotated
/// debug-*.log
/// ```
///
/// Re-running review or validate overwrites these files in place. There is
/// no revision history — questions and comments persist across runs because
/// they live in the same dir.
use std::path::PathBuf;

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Root of all desktop-managed review storage.
pub fn storage_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local")
                .join("share")
        })
        .join("easy-review")
}

/// Sanitize a string for use as a directory name component.
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Derive a stable repo slug. Prefer the basename of the git remote origin URL;
/// fall back to the basename of `repo_root`.
pub fn slug_repo(repo_root: &str) -> String {
    // Try git remote origin
    if let Ok(out) = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
    {
        if out.status.success() {
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // Strip trailing `.git` and take the last path/name component.
            let url = url.trim_end_matches(".git").to_string();
            let name = url
                .rsplit('/')
                .next()
                .or_else(|| url.rsplit(':').next())
                .unwrap_or(&url)
                .to_string();
            if !name.is_empty() {
                return slugify(&name);
            }
        }
    }
    // Fallback: directory name
    std::path::Path::new(repo_root)
        .file_name()
        .and_then(|s| s.to_str())
        .map(slugify)
        .unwrap_or_else(|| "repo".to_string())
}

/// Sanitize a branch name for use as a directory component.
pub fn slug_branch(branch: &str) -> String {
    slugify(&branch.replace('/', "-"))
}

/// Directory for a specific branch under the managed storage root.
pub fn branch_dir(repo_slug: &str, branch_slug: &str) -> PathBuf {
    storage_root()
        .join("repos")
        .join(repo_slug)
        .join("branches")
        .join(branch_slug)
}

// ── ErRoot construction ───────────────────────────────────────────────────────

/// Resolve managed storage from already-slugged components. Used by
/// `apply_managed_root` which derives slugs differently for local vs remote
/// tabs. Falls back to `RepoLocal` only if the dir can't be created.
pub fn resolve_managed_root_from_slugs(repo_slug: &str, branch_slug: &str) -> er_engine::ErRoot {
    let branch_path = branch_dir(repo_slug, branch_slug);
    if std::fs::create_dir_all(&branch_path).is_err() {
        return er_engine::ErRoot::RepoLocal(String::new());
    }
    let path_str = branch_path.to_string_lossy().into_owned();
    er_engine::ErRoot::Managed {
        agent_dir: path_str.clone(),
        session_dir: path_str,
    }
}
