/// Desktop-managed storage for review revisions.
///
/// Layout under `<app_data>/easy-review/repos/<repo_slug>/branches/<branch_slug>/`:
///
/// ```text
/// active.json          { "revision_id": "...", "active_agent": "claude" }
/// revisions/
///   <revision_id>/     e.g. 2026-05-15T103045Z-ba2249621
///     meta.json        { base_branch, head_branch, diff_hash, commit_hash, created_at, scope }
///     agents/
///       claude/        review.json, order.json, checklist.json, questions.json,
///                      github-comments.json, summary.md, diff-tmp, diff-annotated,
///                      snapshots/, debug-*.log
///       codex/         FUTURE
///       consensus/     FUTURE
///     session.json
///     reviewed
/// ```
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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

/// Directory holding all revisions for a branch.
pub fn revisions_dir(repo_slug: &str, branch_slug: &str) -> PathBuf {
    branch_dir(repo_slug, branch_slug).join("revisions")
}

/// Directory for a specific revision's agent output.
pub fn agent_dir(repo_slug: &str, branch_slug: &str, revision_id: &str, agent: &str) -> PathBuf {
    revisions_dir(repo_slug, branch_slug)
        .join(revision_id)
        .join("agents")
        .join(agent)
}

/// Directory root for a specific revision (session.json + reviewed live here).
pub fn revision_root(repo_slug: &str, branch_slug: &str, revision_id: &str) -> PathBuf {
    revisions_dir(repo_slug, branch_slug).join(revision_id)
}

// ── active.json ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePointer {
    pub revision_id: String,
    pub active_agent: String,
}

fn active_path(repo_slug: &str, branch_slug: &str) -> PathBuf {
    branch_dir(repo_slug, branch_slug).join("active.json")
}

/// Read which revision is currently active for this branch. Returns `None` if
/// no revision has been created yet (fresh repo or first desktop launch).
pub fn active_revision(repo_slug: &str, branch_slug: &str) -> Option<ActivePointer> {
    let path = active_path(repo_slug, branch_slug);
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Atomically write `active.json` for a branch.
pub fn set_active(
    repo_slug: &str,
    branch_slug: &str,
    revision_id: &str,
    agent: &str,
) -> Result<()> {
    let dir = branch_dir(repo_slug, branch_slug);
    std::fs::create_dir_all(&dir)?;
    let pointer = ActivePointer {
        revision_id: revision_id.to_string(),
        active_agent: agent.to_string(),
    };
    let json = serde_json::to_vec_pretty(&pointer)?;
    atomic_write(&active_path(repo_slug, branch_slug), &json)
}

// ── meta.json ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionMeta {
    pub revision_id: String,
    pub base_branch: String,
    pub head_branch: String,
    pub diff_hash: String,
    pub commit_hash: String,
    pub created_at: String,
    pub scope: String,
}

fn meta_path(repo_slug: &str, branch_slug: &str, revision_id: &str) -> PathBuf {
    revision_root(repo_slug, branch_slug, revision_id).join("meta.json")
}

fn write_meta(repo_slug: &str, branch_slug: &str, meta: &RevisionMeta) -> Result<()> {
    let json = serde_json::to_vec_pretty(meta)?;
    atomic_write(&meta_path(repo_slug, branch_slug, &meta.revision_id), &json)
}

// ── revision creation ─────────────────────────────────────────────────────────

/// Generate a unique revision ID from the current UTC time and a diff-hash prefix.
pub fn new_revision_id(diff_hash: &str) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Format as compact ISO-ish timestamp: 20260515T103045Z
    let ts = format_unix_ts(secs);
    let hash_prefix = &diff_hash[..diff_hash.len().min(8)];
    format!("{ts}-{hash_prefix}")
}

/// Create a new revision directory tree, write `meta.json`, set it as active,
/// and return the revision ID.
pub fn create_revision(
    repo_slug: &str,
    branch_slug: &str,
    meta_fields: RevisionMeta,
) -> Result<String> {
    let rev_id = meta_fields.revision_id.clone();
    let agent_path = agent_dir(
        repo_slug,
        branch_slug,
        &rev_id,
        &meta_fields.scope.clone().replace("claude-", "claude"),
    );
    // Create the agent dir (creates all parents including agents/ and revision/).
    std::fs::create_dir_all(&agent_path)
        .with_context(|| format!("creating agent dir {}", agent_path.display()))?;
    // Write meta.
    write_meta(repo_slug, branch_slug, &meta_fields).context("writing meta.json")?;
    // Point active.json at this revision.
    let agent_name = &meta_fields.scope;
    let agent_name = if agent_name == "branch" || agent_name == "unstaged" || agent_name == "staged"
    {
        "claude"
    } else {
        agent_name.as_str()
    };
    set_active(repo_slug, branch_slug, &rev_id, agent_name)?;
    Ok(rev_id)
}

/// Bootstrap: copy an existing `<repo>/.er/` directory into a `bootstrap-<ts>` revision
/// so first-launch users don't lose existing review data.
pub fn bootstrap_from_repo_er(repo_root: &str, repo_slug: &str, branch_slug: &str) -> Result<()> {
    bootstrap_from_er_dir(&std::path::Path::new(repo_root).join(".er"), repo_slug, branch_slug)
}

/// Bootstrap from a legacy LocalPr cache path (`~/.cache/er/local/<repo>/pr-<n>/.er/`).
///
/// Called when a LocalPr tab's managed storage is empty but the old cache directory has
/// review files. This avoids losing previously-generated reviews when first loading a PR
/// that was reviewed before managed storage was introduced.
pub fn bootstrap_from_legacy_er_path(
    legacy_er_path: &str,
    repo_slug: &str,
    branch_slug: &str,
) -> Result<()> {
    bootstrap_from_er_dir(std::path::Path::new(legacy_er_path), repo_slug, branch_slug)
}

fn bootstrap_from_er_dir(
    er_dir: &std::path::Path,
    repo_slug: &str,
    branch_slug: &str,
) -> Result<()> {
    if !er_dir.is_dir() {
        return Ok(());
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rev_id = format!("bootstrap-{}", format_unix_ts(secs));
    let dest = agent_dir(repo_slug, branch_slug, &rev_id, "claude");
    std::fs::create_dir_all(&dest)?;

    // Copy each file/dir (shallow: immediate children only — snapshots/ subdir recursed separately).
    for entry in std::fs::read_dir(er_dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let dst = dest.join(entry.file_name());
        if ft.is_file() {
            let _ = std::fs::copy(entry.path(), dst);
        } else if ft.is_dir() {
            let _ = copy_dir_all(&entry.path(), &dst);
        }
    }

    let meta = RevisionMeta {
        revision_id: rev_id.clone(),
        base_branch: String::new(),
        head_branch: String::new(),
        diff_hash: String::new(),
        commit_hash: String::new(),
        created_at: iso_now(),
        scope: "bootstrap".to_string(),
    };
    write_meta(repo_slug, branch_slug, &meta)?;
    set_active(repo_slug, branch_slug, &rev_id, "claude")?;
    Ok(())
}

// ── revision listing ──────────────────────────────────────────────────────────

/// List all revisions for a branch, newest first.
pub fn list_revisions(repo_slug: &str, branch_slug: &str) -> Vec<RevisionMeta> {
    let dir = revisions_dir(repo_slug, branch_slug);
    let mut metas: Vec<RevisionMeta> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let meta_file = entry.path().join("meta.json");
            if let Ok(bytes) = std::fs::read(&meta_file) {
                if let Ok(m) = serde_json::from_slice::<RevisionMeta>(&bytes) {
                    metas.push(m);
                }
            }
        }
    }
    metas.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    metas
}

// ── ErRoot construction ───────────────────────────────────────────────────────

/// Resolve an `ErRoot::Managed` for the given tab, creating the managed
/// directory structure and bootstrapping from `<repo>/.er/` on first launch.
///
/// Falls back to `ErRoot::RepoLocal` if the storage root cannot be determined.
pub fn resolve_managed_root(repo_root: &str, branch: &str) -> er_engine::ErRoot {
    let repo_slug = slug_repo(repo_root);
    let branch_slug = slug_branch(branch);

    // Bootstrap from <repo>/.er/ on first launch (no active.json yet).
    if active_revision(&repo_slug, &branch_slug).is_none() {
        let _ = bootstrap_from_repo_er(repo_root, &repo_slug, &branch_slug);
    }

    // If we still have no active revision (clean repo), create an empty one.
    let active = match active_revision(&repo_slug, &branch_slug) {
        Some(a) => a,
        None => {
            let rev_id = format!(
                "empty-{}",
                format_unix_ts(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                )
            );
            let meta = RevisionMeta {
                revision_id: rev_id.clone(),
                base_branch: String::new(),
                head_branch: branch.to_string(),
                diff_hash: String::new(),
                commit_hash: String::new(),
                created_at: iso_now(),
                scope: "empty".to_string(),
            };
            if create_revision(&repo_slug, &branch_slug, meta).is_err() {
                return er_engine::ErRoot::RepoLocal(repo_root.to_string());
            }
            ActivePointer {
                revision_id: rev_id,
                active_agent: "claude".to_string(),
            }
        }
    };

    let agent = agent_dir(
        &repo_slug,
        &branch_slug,
        &active.revision_id,
        &active.active_agent,
    );
    let session = revision_root(&repo_slug, &branch_slug, &active.revision_id);

    // Ensure the agent dir exists (may be newly created).
    let _ = std::fs::create_dir_all(&agent);

    er_engine::ErRoot::Managed {
        agent_dir: agent.to_string_lossy().into_owned(),
        session_dir: session.to_string_lossy().into_owned(),
    }
}

// ── utilities ─────────────────────────────────────────────────────────────────

/// Atomic file write via tmp + rename.
fn atomic_write(path: &std::path::Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ft.is_file() {
            std::fs::copy(entry.path(), dst_path)?;
        } else if ft.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

/// Format a Unix timestamp as a compact ISO-style string: `20260515T103045Z`.
fn format_unix_ts(secs: u64) -> String {
    // Manual ISO 8601 without chrono. Good enough for directory names.
    let s = secs;
    let mins = s / 60;
    let sec = s % 60;
    let hours = mins / 60;
    let min = mins % 60;
    let days_total = hours / 24;
    let hour = hours % 24;

    // Gregorian calendar computation (handles years 1970–2100 accurately).
    let mut y = 1970u64;
    let mut d = days_total;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 1u64;
    for md in &month_days {
        if d < *md {
            break;
        }
        d -= md;
        m += 1;
    }
    let day = d + 1;
    format!("{y:04}{m:02}{day:02}T{hour:02}{min:02}{sec:02}Z")
}

/// Current time as a compact ISO string (same format as `format_unix_ts`).
pub(crate) fn iso_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_ts(secs)
}
