use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectsFile {
    #[serde(default)]
    pub projects: Vec<ProjectRecord>,
    #[serde(default)]
    pub active_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub root_path: String,
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default)]
    pub dismissed_prs: Vec<u64>,
    /// PR numbers the user has explicitly chosen to surface in the sidebar
    /// even if they don't naturally pass the author/assignee/reviewer filter.
    #[serde(default)]
    pub tracked_prs: Vec<u64>,
    /// Names of branches the user has chosen to surface in the sidebar.
    /// Empty means "current only" — `snapshot::build_tracked_branches` always
    /// also includes the currently-checked-out branch on top of this list.
    #[serde(default)]
    pub tracked_branches: Vec<String>,
    /// Branches hidden from the sidebar via "Remove from view" (worktree-backed
    /// branches stay dismissed until re-tracked or the user views that branch).
    #[serde(default)]
    pub dismissed_branches: Vec<String>,
    /// PRs the user has opened for review, most recent first (max 50 persisted).
    #[serde(default)]
    pub recent_prs: Vec<RecentPrEntry>,
    /// PRs the user has manually bookmarked (max 50 persisted).
    #[serde(default)]
    pub saved_prs: Vec<SavedPrEntry>,
    /// When true, Desktop auto-runs triage on new/updated open PRs while the app is open.
    #[serde(default)]
    pub auto_triage: bool,
    /// When true (and `auto_triage`), also triage open PRs you authored.
    #[serde(default)]
    pub auto_triage_own_prs: bool,
    /// When to auto-triage: `new-and-push`, `new-only`, or `review-requested`.
    #[serde(default = "default_auto_triage_when")]
    pub auto_triage_when: String,
    /// Skip auto-triage when filtered diff exceeds this size (KB). `0` = no limit.
    #[serde(default)]
    pub auto_triage_max_diff_kb: u32,
    /// Glob patterns excluded from AI review diffs (triage + full review).
    #[serde(default)]
    pub review_ignore_globs: Vec<String>,
}

fn default_auto_triage_when() -> String {
    "new-and-push".to_string()
}

pub const AUTO_TRIAGE_WHEN_OPTIONS: &[&str] = &["new-and-push", "new-only", "review-requested"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentPrEntry {
    pub number: u64,
    pub viewed_at_ms: u64,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPrEntry {
    pub number: u64,
    pub saved_at_ms: u64,
    #[serde(default)]
    pub title: String,
}

const MAX_PR_HISTORY: usize = 50;

fn now_epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn record_recent_pr(project_id: &str, pr_number: u64, title: &str) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    record_recent_pr_on_project(proj, pr_number, title);
    save(&file)
}

fn record_recent_pr_on_project(proj: &mut ProjectRecord, pr_number: u64, title: &str) {
    let now = now_epoch_ms();
    proj.recent_prs.retain(|e| e.number != pr_number);
    proj.recent_prs.insert(
        0,
        RecentPrEntry {
            number: pr_number,
            viewed_at_ms: now,
            title: title.to_string(),
        },
    );
    proj.recent_prs.truncate(MAX_PR_HISTORY);
}

fn normalize_remote_slug(remote: &str) -> Option<String> {
    let trimmed = remote.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .unwrap_or(trimmed);
    let slug = without_scheme
        .trim_end_matches(".git")
        .trim_matches('/')
        .to_ascii_lowercase();
    if slug.split('/').count() == 2 {
        Some(slug)
    } else {
        None
    }
}

fn remote_project_id(remote: &str) -> String {
    format!("remote-{}", sanitize_id(remote))
}

fn ensure_remote_project_in_file(file: &mut ProjectsFile, remote: &str) -> anyhow::Result<String> {
    let remote = normalize_remote_slug(remote)
        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub remote slug: {remote}"))?;
    if let Some(existing) = file.projects.iter().find(|p| {
        p.remote
            .as_deref()
            .and_then(normalize_remote_slug)
            .is_some_and(|existing| existing == remote)
    }) {
        return Ok(existing.id.clone());
    }

    let base_id = remote_project_id(&remote);
    let mut unique_id = base_id.clone();
    let mut n = 2;
    while file.projects.iter().any(|p| p.id == unique_id) {
        unique_id = format!("{}-{}", base_id, n);
        n += 1;
    }

    let record = ProjectRecord {
        id: unique_id.clone(),
        name: remote.clone(),
        root_path: String::new(),
        remote: Some(remote),
        dismissed_prs: Vec::new(),
        tracked_prs: Vec::new(),
        tracked_branches: Vec::new(),
        dismissed_branches: Vec::new(),
        recent_prs: Vec::new(),
        saved_prs: Vec::new(),
        auto_triage: false,
        auto_triage_own_prs: false,
        auto_triage_when: default_auto_triage_when(),
        auto_triage_max_diff_kb: 0,
        review_ignore_globs: Vec::new(),
    };
    file.projects.push(record);
    Ok(unique_id)
}

pub fn ensure_remote_project(remote: &str) -> anyhow::Result<String> {
    let mut file = load();
    let project_id = ensure_remote_project_in_file(&mut file, remote)?;
    save(&file)?;
    Ok(project_id)
}

fn delete_project_in_file(file: &mut ProjectsFile, project_id: &str) -> anyhow::Result<()> {
    let before = file.projects.len();
    file.projects.retain(|p| p.id != project_id);
    if file.projects.len() == before {
        return Err(anyhow::anyhow!("Project not found: {project_id}"));
    }
    if file.active_id.as_deref() == Some(project_id) {
        file.active_id = file.projects.first().map(|p| p.id.clone());
    }
    Ok(())
}

pub fn delete_project(project_id: &str) -> anyhow::Result<()> {
    let mut file = load();
    delete_project_in_file(&mut file, project_id)?;
    save(&file)
}

pub fn save_pr(project_id: &str, pr_number: u64, title: &str) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    let now = now_epoch_ms();
    proj.saved_prs.retain(|e| e.number != pr_number);
    proj.saved_prs.insert(
        0,
        SavedPrEntry {
            number: pr_number,
            saved_at_ms: now,
            title: title.to_string(),
        },
    );
    proj.saved_prs.truncate(MAX_PR_HISTORY);
    save(&file)
}

pub fn unsave_pr(project_id: &str, pr_number: u64) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    let before = proj.saved_prs.len();
    proj.saved_prs.retain(|e| e.number != pr_number);
    if proj.saved_prs.len() != before {
        save(&file)?;
    }
    Ok(())
}

pub fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("er").join("projects.json")
}

use std::sync::Mutex;

static PROJECTS_LOAD_CACHE: Mutex<Option<(std::time::SystemTime, ProjectsFile)>> = Mutex::new(None);

/// Drop the in-process parse cache so the next [`load`] re-reads from disk.
pub fn invalidate_load_cache() {
    if let Ok(mut guard) = PROJECTS_LOAD_CACHE.lock() {
        *guard = None;
    }
}

pub fn load() -> ProjectsFile {
    let path = config_path();
    // Skip JSON parse when mtime hasn't advanced since the last load. The file
    // is hit on every snapshot build; with multiple polls per second under
    // user input, parsing it repeatedly is wasted work.
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if let (Some(mtime), Ok(guard)) = (mtime, PROJECTS_LOAD_CACHE.lock()) {
        if let Some((cached_mtime, cached_file)) = guard.as_ref() {
            if *cached_mtime == mtime {
                return cached_file.clone();
            }
        }
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return ProjectsFile::default();
    };
    let parsed: ProjectsFile = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("projects.json parse failed ({}): {e}", path.display());
            return ProjectsFile::default();
        }
    };
    if let (Some(mtime), Ok(mut guard)) = (mtime, PROJECTS_LOAD_CACHE.lock()) {
        *guard = Some((mtime, parsed.clone()));
    }
    parsed
}

pub fn save(file: &ProjectsFile) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(file)?;
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &path)?;
    invalidate_load_cache();
    Ok(())
}

fn sanitize_id(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = s.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}

fn current_branch(root_path: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(root_path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn query_remote(root_path: &str) -> Option<String> {
    let out = std::process::Command::new("gh")
        .args([
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ])
        .current_dir(root_path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Lightweight tab identity for project registration (testable without git/gh).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabProjectRef {
    pub repo_root: String,
    pub remote: Option<String>,
}

impl TabProjectRef {
    pub fn from_tab(tab: &er_engine::app::TabState) -> Self {
        Self {
            repo_root: tab.repo_root.clone(),
            remote: tab.remote_repo.clone(),
        }
    }
}

/// Register a local repo root in `file` when missing. Returns true if a row was added.
#[cfg(test)]
fn register_local_root_in_file(file: &mut ProjectsFile, root_path: &str) -> bool {
    if root_path.is_empty() {
        return false;
    }
    if file.projects.iter().any(|p| p.root_path == root_path) {
        return false;
    }
    let folder = std::path::Path::new(root_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();
    let id = sanitize_id(&folder);
    let mut unique_id = id.clone();
    let mut n = 2;
    while file.projects.iter().any(|p| p.id == unique_id) {
        unique_id = format!("{}-{}", id, n);
        n += 1;
    }
    file.projects.push(ProjectRecord {
        id: unique_id.clone(),
        name: folder,
        root_path: root_path.to_string(),
        remote: None,
        dismissed_prs: Vec::new(),
        tracked_prs: Vec::new(),
        tracked_branches: Vec::new(),
        dismissed_branches: Vec::new(),
        recent_prs: Vec::new(),
        saved_prs: Vec::new(),
        auto_triage: false,
        auto_triage_own_prs: false,
        auto_triage_when: default_auto_triage_when(),
        auto_triage_max_diff_kb: 0,
        review_ignore_globs: Vec::new(),
    });
    if file.active_id.is_none() {
        file.active_id = Some(unique_id);
    }
    true
}

/// Upsert project rows for tab refs into an in-memory file. Returns true if changed.
#[cfg(test)]
fn sync_project_refs_in_file(file: &mut ProjectsFile, refs: &[TabProjectRef]) -> bool {
    use std::collections::HashSet;
    let mut changed = false;
    let mut seen_roots = HashSet::new();
    let mut seen_remotes = HashSet::new();
    for r in refs {
        if !r.repo_root.is_empty()
            && seen_roots.insert(r.repo_root.clone())
            && register_local_root_in_file(file, &r.repo_root)
        {
            changed = true;
        }
        if let Some(ref remote) = r.remote {
            if seen_remotes.insert(remote.clone())
                && ensure_remote_project_in_file(file, remote).is_ok()
            {
                changed = true;
            }
        }
    }
    changed
}

/// Register every unique local repo and remote slug referenced by open tabs.
pub fn sync_projects_from_tabs(tabs: &[er_engine::app::TabState]) {
    if tabs.is_empty() {
        return;
    }
    use std::collections::HashSet;
    let refs: Vec<TabProjectRef> = tabs.iter().map(TabProjectRef::from_tab).collect();
    let mut seen_roots = HashSet::new();
    for r in &refs {
        if !r.repo_root.is_empty() && seen_roots.insert(r.repo_root.clone()) {
            let _ = auto_register(&r.repo_root);
        }
    }
    let mut seen_remotes = HashSet::new();
    for r in &refs {
        if let Some(ref remote) = r.remote {
            if seen_remotes.insert(remote.clone()) {
                let _ = ensure_remote_project(remote);
            }
        }
    }
}

pub fn auto_register(root_path: &str) -> ProjectRecord {
    let mut file = load();
    let folder = std::path::Path::new(root_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();
    let id = sanitize_id(&folder);
    let remote = query_remote(root_path);

    // Upsert by root_path
    if let Some(existing) = file.projects.iter_mut().find(|p| p.root_path == root_path) {
        if existing.remote.is_none() && remote.is_some() {
            existing.remote = remote.clone();
        }
        let record = existing.clone();
        let _ = save(&file);
        return record;
    }

    // Ensure id uniqueness
    let mut unique_id = id.clone();
    let mut n = 2;
    while file.projects.iter().any(|p| p.id == unique_id) {
        unique_id = format!("{}-{}", id, n);
        n += 1;
    }

    let tracked_branches = current_branch(root_path)
        .map(|b| vec![b])
        .unwrap_or_default();

    let record = ProjectRecord {
        id: unique_id,
        name: folder,
        root_path: root_path.to_string(),
        remote,
        dismissed_prs: Vec::new(),
        tracked_prs: Vec::new(),
        tracked_branches,
        dismissed_branches: Vec::new(),
        recent_prs: Vec::new(),
        saved_prs: Vec::new(),
        auto_triage: false,
        auto_triage_own_prs: false,
        auto_triage_when: default_auto_triage_when(),
        auto_triage_max_diff_kb: 0,
        review_ignore_globs: Vec::new(),
    };
    file.projects.push(record.clone());
    if file.active_id.is_none() {
        file.active_id = Some(record.id.clone());
    }
    let _ = save(&file);
    record
}

pub fn dismiss_pr(project_id: &str, pr_number: u64) {
    let mut file = load();
    if let Some(p) = file.projects.iter_mut().find(|p| p.id == project_id) {
        if !p.dismissed_prs.contains(&pr_number) {
            p.dismissed_prs.push(pr_number);
            let _ = save(&file);
        }
    }
}

pub fn track_pr(project_id: &str, pr_number: u64) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    let mut changed = false;
    if !proj.tracked_prs.contains(&pr_number) {
        proj.tracked_prs.push(pr_number);
        changed = true;
    }
    // Explicit tracking overrides a prior dismiss.
    let before = proj.dismissed_prs.len();
    proj.dismissed_prs.retain(|n| n != &pr_number);
    if proj.dismissed_prs.len() != before {
        changed = true;
    }
    if changed {
        save(&file)?;
    }
    Ok(())
}

pub fn untrack_pr(project_id: &str, pr_number: u64) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    let before = proj.tracked_prs.len();
    proj.tracked_prs.retain(|n| n != &pr_number);
    if proj.tracked_prs.len() != before {
        save(&file)?;
    }
    Ok(())
}

fn add_tracked_branch_on_project(proj: &mut ProjectRecord, name: &str) -> bool {
    let mut changed = false;
    if !proj.tracked_branches.iter().any(|n| n == name) {
        proj.tracked_branches.push(name.to_string());
        changed = true;
    }
    let before = proj.dismissed_branches.len();
    proj.dismissed_branches.retain(|n| n != name);
    if proj.dismissed_branches.len() != before {
        changed = true;
    }
    changed
}

pub fn add_tracked_branch(project_id: &str, name: &str) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    if add_tracked_branch_on_project(proj, name) {
        save(&file)?;
    }
    Ok(())
}

fn remove_tracked_branch_on_project(proj: &mut ProjectRecord, name: &str) -> bool {
    let mut changed = false;
    let before_tracked = proj.tracked_branches.len();
    proj.tracked_branches.retain(|n| n != name);
    if proj.tracked_branches.len() != before_tracked {
        changed = true;
    }
    if !proj.dismissed_branches.iter().any(|n| n == name) {
        proj.dismissed_branches.push(name.to_string());
        changed = true;
    }
    changed
}

pub fn remove_tracked_branch(project_id: &str, name: &str) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    if remove_tracked_branch_on_project(proj, name) {
        save(&file)?;
    }
    Ok(())
}

pub fn set_active(id: &str) {
    let mut file = load();
    if file.projects.iter().any(|p| p.id == id) {
        file.active_id = Some(id.to_string());
        let _ = save(&file);
    }
}

pub fn set_auto_triage(project_id: &str, enabled: bool) -> anyhow::Result<()> {
    patch_project_review_settings(
        project_id,
        ProjectReviewSettingsPatch {
            auto_triage: Some(enabled),
            ..Default::default()
        },
    )
}

pub fn set_auto_triage_own_prs(project_id: &str, enabled: bool) -> anyhow::Result<()> {
    patch_project_review_settings(
        project_id,
        ProjectReviewSettingsPatch {
            auto_triage_own_prs: Some(enabled),
            ..Default::default()
        },
    )
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReviewSettingsPatch {
    pub auto_triage: Option<bool>,
    pub auto_triage_own_prs: Option<bool>,
    pub auto_triage_when: Option<String>,
    pub auto_triage_max_diff_kb: Option<u32>,
    pub review_ignore_glob_add: Option<String>,
    pub review_ignore_glob_remove: Option<usize>,
}

pub fn patch_project_review_settings(
    project_id: &str,
    patch: ProjectReviewSettingsPatch,
) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    let mut changed = false;

    if let Some(enabled) = patch.auto_triage {
        if proj.auto_triage != enabled {
            proj.auto_triage = enabled;
            changed = true;
        }
        if !enabled && proj.auto_triage_own_prs {
            proj.auto_triage_own_prs = false;
            changed = true;
        }
    }
    if let Some(enabled) = patch.auto_triage_own_prs {
        if enabled && !proj.auto_triage {
            anyhow::bail!("Enable auto-triage before including your own PRs");
        }
        if proj.auto_triage_own_prs != enabled {
            proj.auto_triage_own_prs = enabled;
            changed = true;
        }
    }
    if let Some(when) = patch.auto_triage_when {
        if !AUTO_TRIAGE_WHEN_OPTIONS.contains(&when.as_str()) {
            anyhow::bail!("Invalid auto_triage_when: {when}");
        }
        if proj.auto_triage_when != when {
            proj.auto_triage_when = when;
            changed = true;
        }
    }
    if let Some(max_kb) = patch.auto_triage_max_diff_kb {
        if proj.auto_triage_max_diff_kb != max_kb {
            proj.auto_triage_max_diff_kb = max_kb;
            changed = true;
        }
    }
    if let Some(glob) = patch.review_ignore_glob_add {
        let glob = glob.trim().to_string();
        if !glob.is_empty() && !proj.review_ignore_globs.iter().any(|g| g == &glob) {
            proj.review_ignore_globs.push(glob);
            changed = true;
        }
    }
    if let Some(index) = patch.review_ignore_glob_remove {
        if index < proj.review_ignore_globs.len() {
            proj.review_ignore_globs.remove(index);
            changed = true;
        }
    }

    if changed {
        save(&file)?;
    }
    Ok(())
}

/// Resolve a configured project id from inbox target hints (explicit id preferred by caller).
pub fn resolve_project_id_for_inbox(
    repo_root: Option<&str>,
    remote: Option<&str>,
) -> Option<String> {
    let file = load();
    if let Some(root) = repo_root.filter(|r| !r.is_empty()) {
        if let Some(p) = file.projects.iter().find(|p| p.root_path == root) {
            return Some(p.id.clone());
        }
    }
    if let Some(slug) = remote.and_then(normalize_remote_slug) {
        if let Some(p) = file.projects.iter().find(|p| {
            p.remote
                .as_deref()
                .and_then(normalize_remote_slug)
                .is_some_and(|r| r == slug)
        }) {
            return Some(p.id.clone());
        }
    }
    None
}

pub fn review_ignore_globs_for_repo(repo_root: &str, remote: Option<&str>) -> Vec<String> {
    let file = load();
    if !repo_root.is_empty() {
        if let Some(p) = file.projects.iter().find(|p| p.root_path == repo_root) {
            return p.review_ignore_globs.clone();
        }
    }
    if let Some(slug) = remote.and_then(normalize_remote_slug) {
        if let Some(p) = file.projects.iter().find(|p| {
            p.remote
                .as_deref()
                .and_then(normalize_remote_slug)
                .is_some_and(|r| r == slug)
        }) {
            return p.review_ignore_globs.clone();
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_project(id: &str, remote: Option<&str>) -> ProjectRecord {
        ProjectRecord {
            id: id.to_string(),
            name: id.to_string(),
            root_path: format!("/tmp/{id}"),
            remote: remote.map(|r| r.to_string()),
            dismissed_prs: Vec::new(),
            tracked_prs: Vec::new(),
            tracked_branches: Vec::new(),
            dismissed_branches: Vec::new(),
            recent_prs: Vec::new(),
            saved_prs: Vec::new(),
            auto_triage: false,
            auto_triage_own_prs: false,
            auto_triage_when: default_auto_triage_when(),
            auto_triage_max_diff_kb: 0,
            review_ignore_globs: Vec::new(),
        }
    }

    #[test]
    fn ensure_remote_project_creates_one_record_per_normalized_remote() {
        let mut file = ProjectsFile::default();
        let first = ensure_remote_project_in_file(&mut file, "Owner/Repo").unwrap();
        let second =
            ensure_remote_project_in_file(&mut file, "https://github.com/owner/repo.git").unwrap();

        assert_eq!(first, second);
        assert_eq!(file.projects.len(), 1);
        assert_eq!(file.projects[0].root_path, "");
        assert_eq!(file.projects[0].remote.as_deref(), Some("owner/repo"));
        assert_eq!(file.projects[0].name, "owner/repo");
    }

    #[test]
    fn ensure_remote_project_reuses_existing_local_project() {
        let mut file = ProjectsFile {
            projects: vec![local_project("local", Some("Owner/Repo"))],
            active_id: None,
        };

        let project_id = ensure_remote_project_in_file(&mut file, "owner/repo").unwrap();

        assert_eq!(project_id, "local");
        assert_eq!(file.projects.len(), 1);
        assert_eq!(file.projects[0].root_path, "/tmp/local");
    }

    #[test]
    fn record_recent_pr_moves_duplicate_to_top() {
        let mut project = local_project("remote-owner-repo", Some("owner/repo"));

        record_recent_pr_on_project(&mut project, 1, "first");
        record_recent_pr_on_project(&mut project, 2, "second");
        record_recent_pr_on_project(&mut project, 1, "first updated");

        let numbers: Vec<u64> = project
            .recent_prs
            .iter()
            .map(|entry| entry.number)
            .collect();
        assert_eq!(numbers, vec![1, 2]);
        assert_eq!(project.recent_prs[0].title, "first updated");
    }

    #[test]
    fn remove_tracked_branch_dismisses_even_when_not_tracked() {
        let mut project = local_project("bun", Some("oven-sh/bun"));
        project.tracked_branches.clear();

        assert!(remove_tracked_branch_on_project(&mut project, "feature-a"));

        assert!(project.tracked_branches.is_empty());
        assert_eq!(project.dismissed_branches, vec!["feature-a"]);
    }

    #[test]
    fn add_tracked_branch_clears_prior_dismiss() {
        let mut project = local_project("bun", Some("oven-sh/bun"));
        project.dismissed_branches.push("feature-a".to_string());

        assert!(add_tracked_branch_on_project(&mut project, "feature-a"));

        assert!(project.dismissed_branches.iter().all(|n| n != "feature-a"));
        assert!(project.tracked_branches.iter().any(|n| n == "feature-a"));
    }

    #[test]
    fn delete_project_removes_record_and_updates_active_id() {
        let mut file = ProjectsFile {
            projects: vec![
                local_project("first", Some("owner/first")),
                local_project("second", Some("owner/second")),
            ],
            active_id: Some("first".to_string()),
        };

        delete_project_in_file(&mut file, "first").unwrap();

        assert_eq!(file.projects.len(), 1);
        assert_eq!(file.projects[0].id, "second");
        assert_eq!(file.active_id.as_deref(), Some("second"));
    }

    #[test]
    fn sync_project_refs_registers_multiple_roots() {
        let mut file = ProjectsFile::default();
        let refs = vec![
            TabProjectRef {
                repo_root: "/tmp/easy-review".to_string(),
                remote: None,
            },
            TabProjectRef {
                repo_root: "/tmp/discovery".to_string(),
                remote: None,
            },
            TabProjectRef {
                repo_root: "/tmp/inkbooking".to_string(),
                remote: None,
            },
        ];

        assert!(sync_project_refs_in_file(&mut file, &refs));
        assert_eq!(file.projects.len(), 3);
        let roots: Vec<&str> = file.projects.iter().map(|p| p.root_path.as_str()).collect();
        assert!(roots.contains(&"/tmp/easy-review"));
        assert!(roots.contains(&"/tmp/discovery"));
        assert!(roots.contains(&"/tmp/inkbooking"));

        // Idempotent — duplicate refs do not add rows.
        assert!(!sync_project_refs_in_file(&mut file, &refs));
        assert_eq!(file.projects.len(), 3);
    }
}
