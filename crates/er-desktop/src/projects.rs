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
}

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

pub fn load() -> ProjectsFile {
    let path = config_path();
    // Skip JSON parse when mtime hasn't advanced since the last load. The file
    // is hit on every snapshot build; with multiple polls per second under
    // user input, parsing it repeatedly is wasted work.
    use std::sync::Mutex;
    static CACHE: Mutex<Option<(std::time::SystemTime, ProjectsFile)>> = Mutex::new(None);
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if let (Some(mtime), Ok(guard)) = (mtime, CACHE.lock()) {
        if let Some((cached_mtime, cached_file)) = guard.as_ref() {
            if *cached_mtime == mtime {
                return cached_file.clone();
            }
        }
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return ProjectsFile::default();
    };
    let parsed: ProjectsFile = serde_json::from_slice(&bytes).unwrap_or_default();
    if let (Some(mtime), Ok(mut guard)) = (mtime, CACHE.lock()) {
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
}
