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
}

pub fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("er").join("projects.json")
}

pub fn load() -> ProjectsFile {
    let path = config_path();
    let Ok(bytes) = std::fs::read(&path) else {
        return ProjectsFile::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
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
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let trimmed = s.trim_matches('-').to_string();
    if trimmed.is_empty() { "project".to_string() } else { trimmed }
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
    if s.is_empty() { None } else { Some(s) }
}

fn query_remote(root_path: &str) -> Option<String> {
    let out = std::process::Command::new("gh")
        .args(["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])
        .current_dir(root_path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
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

pub fn add_tracked_branch(project_id: &str, name: &str) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    if !proj.tracked_branches.iter().any(|n| n == name) {
        proj.tracked_branches.push(name.to_string());
        save(&file)?;
    }
    Ok(())
}

pub fn remove_tracked_branch(project_id: &str, name: &str) -> anyhow::Result<()> {
    let mut file = load();
    let proj = file
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    let before = proj.tracked_branches.len();
    proj.tracked_branches.retain(|n| n != name);
    if proj.tracked_branches.len() != before {
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
