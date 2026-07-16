//! Read Easy Review desktop project config (`~/.config/er/projects.json`).

use anyhow::{Context, Result};
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
}

fn projects_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("er").join("projects.json"))
}

pub fn load_projects() -> ProjectsFile {
    let Some(path) = projects_path() else {
        return ProjectsFile::default();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return ProjectsFile::default();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Normalize `owner/repo` or a GitHub URL into `(owner, repo)`.
pub fn parse_repo_slug(remote: &str) -> Result<(String, String)> {
    er_engine::github::parse_remote_url(remote)
        .or_else(|| {
            let trimmed = remote.trim().trim_end_matches('/');
            let parts: Vec<_> = trimmed.split('/').collect();
            if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                Some((
                    parts[0].to_string(),
                    parts[1].trim_end_matches(".git").to_string(),
                ))
            } else {
                None
            }
        })
        .with_context(|| format!("invalid repo slug: {remote}"))
}

/// Resolve `(owner, repo)` from explicit slug, project id, or active project.
pub fn resolve_repo(
    repo: Option<&str>,
    project_id: Option<&str>,
) -> Result<(String, String, Option<String>)> {
    if let Some(slug) = repo {
        let (o, r) = parse_repo_slug(slug)?;
        return Ok((o, r, None));
    }
    let file = load_projects();
    let project = if let Some(id) = project_id {
        file.projects
            .iter()
            .find(|p| p.id == id)
            .with_context(|| format!("project not found: {id}"))?
    } else if let Some(active) = file.active_id.as_deref() {
        file.projects
            .iter()
            .find(|p| p.id == active)
            .context("active project missing from projects.json")?
    } else {
        file.projects
            .first()
            .context("no Easy Review projects configured; pass repo=owner/repo")?
    };
    let remote = project
        .remote
        .as_deref()
        .context("project has no remote; pass repo=owner/repo")?;
    let (o, r) = parse_repo_slug(remote)?;
    Ok((o, r, Some(project.name.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_slug_forms() {
        let (o, r) = parse_repo_slug("acme/widgets").unwrap();
        assert_eq!((o, r), ("acme".into(), "widgets".into()));
        let (o, r) = parse_repo_slug("https://github.com/acme/widgets.git").unwrap();
        assert_eq!((o.as_str(), r.as_str()), ("acme", "widgets"));
    }
}
