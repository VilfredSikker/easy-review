//! Value-preserving pin/unpin of PRs into Desktop's `saved_prs`
//! (`~/.config/er/projects.json`).
//!
//! Mutations go through `serde_json::Value` so unknown project fields
//! (tracked_prs, auto_triage, …) are never dropped — critical for MCP, which
//! must not round-trip a partial `ProjectRecord`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PR_HISTORY: usize = 50;

/// One entry in a project's `saved_prs` array (Desktop Saved sidebar).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedPr {
    pub number: u64,
    pub saved_at_ms: u64,
    #[serde(default)]
    pub title: String,
}

/// Path to `projects.json`. Overridable via `ER_PROJECTS_JSON` for tests.
pub fn projects_path() -> PathBuf {
    if let Ok(override_path) = std::env::var("ER_PROJECTS_JSON") {
        if !override_path.is_empty() {
            return PathBuf::from(override_path);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("er")
        .join("projects.json")
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Normalize a GitHub remote reference to a lowercase `owner/repo` slug.
pub fn normalize_remote_slug(remote: &str) -> String {
    let trimmed = remote.trim();
    let without_scheme = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("git@github.com:"))
        .unwrap_or(trimmed);
    without_scheme
        .trim_end_matches(".git")
        .trim_matches('/')
        .to_ascii_lowercase()
}

fn valid_remote_slug(remote: &str) -> Option<String> {
    let slug = normalize_remote_slug(remote);
    if !slug.is_empty() && slug.split('/').count() == 2 {
        Some(slug)
    } else {
        None
    }
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

fn load_value(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({ "projects": [], "active_id": null }));
    }
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(json!({ "projects": [], "active_id": null }));
    }
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

fn save_value(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value).context("serialize projects.json")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} → {}", tmp.display(), path.display()))?;
    Ok(())
}

fn projects_array_mut(root: &mut Value) -> Result<&mut Vec<Value>> {
    if root.get("projects").is_none() {
        root.as_object_mut()
            .context("projects.json root must be an object")?
            .insert("projects".into(), Value::Array(Vec::new()));
    }
    root.get_mut("projects")
        .and_then(|v| v.as_array_mut())
        .context("projects.json projects must be an array")
}

fn find_project_mut<'a>(projects: &'a mut [Value], project_id: &str) -> Result<&'a mut Value> {
    projects
        .iter_mut()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(project_id))
        .with_context(|| format!("Project not found: {project_id}"))
}

fn parse_saved_prs(project: &Value) -> Vec<PinnedPr> {
    project
        .get("saved_prs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| serde_json::from_value::<PinnedPr>(e.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn write_saved_prs(project: &mut Value, entries: &[PinnedPr]) -> Result<()> {
    let obj = project
        .as_object_mut()
        .context("project entry must be an object")?;
    obj.insert(
        "saved_prs".into(),
        serde_json::to_value(entries).context("serialize saved_prs")?,
    );
    Ok(())
}

/// Find an existing project for `owner/repo` without creating one.
pub fn find_project_for_remote(owner: &str, repo: &str) -> Result<Option<(String, String)>> {
    find_project_for_remote_at(&projects_path(), owner, repo)
}

fn find_project_for_remote_at(
    path: &Path,
    owner: &str,
    repo: &str,
) -> Result<Option<(String, String)>> {
    let remote = valid_remote_slug(&format!("{owner}/{repo}"))
        .with_context(|| format!("invalid GitHub remote slug: {owner}/{repo}"))?;
    let root = load_value(path)?;
    let Some(projects) = root.get("projects").and_then(|v| v.as_array()) else {
        return Ok(None);
    };
    for p in projects {
        let existing = p
            .get("remote")
            .and_then(|v| v.as_str())
            .and_then(valid_remote_slug);
        if existing.as_deref() == Some(remote.as_str()) {
            let id = p
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = p
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&remote)
                .to_string();
            if id.is_empty() {
                bail!("project for {remote} has empty id");
            }
            return Ok(Some((id, name)));
        }
    }
    Ok(None)
}

/// Find or create a Desktop project for `owner/repo`. Returns `(project_id, project_name)`.
pub fn ensure_project_for_remote(owner: &str, repo: &str) -> Result<(String, String)> {
    ensure_project_for_remote_at(&projects_path(), owner, repo)
}

fn ensure_project_for_remote_at(path: &Path, owner: &str, repo: &str) -> Result<(String, String)> {
    if let Some(found) = find_project_for_remote_at(path, owner, repo)? {
        return Ok(found);
    }

    let remote = valid_remote_slug(&format!("{owner}/{repo}"))
        .with_context(|| format!("invalid GitHub remote slug: {owner}/{repo}"))?;

    let mut root = load_value(path)?;
    let projects = projects_array_mut(&mut root)?;

    let base_id = format!("remote-{}", sanitize_id(&remote));
    let mut unique_id = base_id.clone();
    let mut n = 2;
    while projects
        .iter()
        .any(|p| p.get("id").and_then(|v| v.as_str()) == Some(unique_id.as_str()))
    {
        unique_id = format!("{base_id}-{n}");
        n += 1;
    }

    let record = json!({
        "id": unique_id,
        "name": remote,
        "root_path": "",
        "remote": remote,
        "dismissed_prs": [],
        "tracked_prs": [],
        "tracked_branches": [],
        "dismissed_branches": [],
        "recent_prs": [],
        "saved_prs": [],
        "auto_triage": false,
        "auto_triage_own_prs": false,
        "auto_triage_when": "new-and-push",
        "auto_triage_max_diff_kb": 0,
        "review_ignore_globs": [],
    });
    let id = unique_id.clone();
    let name = remote.clone();
    projects.push(record);
    save_value(path, &root)?;
    Ok((id, name))
}

/// Resolve a project id for pinning: explicit id, else match by remote, else create.
pub fn resolve_project_for_pin(
    project_id: Option<&str>,
    owner: &str,
    repo: &str,
) -> Result<(String, String)> {
    resolve_project_for_pin_at(&projects_path(), project_id, owner, repo, true)
}

/// Resolve a project for list tools: explicit id or remote match — never creates.
pub fn resolve_project_for_list(
    project_id: Option<&str>,
    owner: &str,
    repo: &str,
) -> Result<Option<(String, String)>> {
    let path = projects_path();
    if let Some(id) = project_id {
        let root = load_value(&path)?;
        let projects = root
            .get("projects")
            .and_then(|v| v.as_array())
            .context("projects.json projects must be an array")?;
        let p = projects
            .iter()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
            .with_context(|| format!("project not found: {id}"))?;
        let name = p
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(id)
            .to_string();
        return Ok(Some((id.to_string(), name)));
    }
    find_project_for_remote_at(&path, owner, repo)
}

fn resolve_project_for_pin_at(
    path: &Path,
    project_id: Option<&str>,
    owner: &str,
    repo: &str,
    create: bool,
) -> Result<(String, String)> {
    if let Some(id) = project_id {
        let root = load_value(path)?;
        let projects = root
            .get("projects")
            .and_then(|v| v.as_array())
            .context("projects.json projects must be an array")?;
        let p = projects
            .iter()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
            .with_context(|| format!("project not found: {id}"))?;
        let name = p
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(id)
            .to_string();
        return Ok((id.to_string(), name));
    }
    if create {
        ensure_project_for_remote_at(path, owner, repo)
    } else {
        find_project_for_remote_at(path, owner, repo)?
            .with_context(|| format!("no Easy Review project for {owner}/{repo}"))
    }
}

/// Pin a PR into the project's `saved_prs` (Desktop Saved). Upserts, newest first, max 50.
pub fn pin_pr(project_id: &str, number: u64, title: &str) -> Result<PinnedPr> {
    pin_pr_at(&projects_path(), project_id, number, title)
}

fn pin_pr_at(path: &Path, project_id: &str, number: u64, title: &str) -> Result<PinnedPr> {
    let mut root = load_value(path)?;
    let projects = projects_array_mut(&mut root)?;
    let project = find_project_mut(projects, project_id)?;
    let mut entries = parse_saved_prs(project);
    entries.retain(|e| e.number != number);
    let entry = PinnedPr {
        number,
        saved_at_ms: now_epoch_ms(),
        title: title.to_string(),
    };
    entries.insert(0, entry.clone());
    entries.truncate(MAX_PR_HISTORY);
    write_saved_prs(project, &entries)?;
    save_value(path, &root)?;
    Ok(entry)
}

/// Remove a PR from the project's `saved_prs`. Returns whether an entry was removed.
pub fn unpin_pr(project_id: &str, number: u64) -> Result<bool> {
    unpin_pr_at(&projects_path(), project_id, number)
}

fn unpin_pr_at(path: &Path, project_id: &str, number: u64) -> Result<bool> {
    let mut root = load_value(path)?;
    let projects = projects_array_mut(&mut root)?;
    let project = find_project_mut(projects, project_id)?;
    let mut entries = parse_saved_prs(project);
    let before = entries.len();
    entries.retain(|e| e.number != number);
    if entries.len() == before {
        return Ok(false);
    }
    write_saved_prs(project, &entries)?;
    save_value(path, &root)?;
    Ok(true)
}

/// List pinned PRs for a project (most recently pinned first).
pub fn list_pinned(project_id: &str) -> Result<Vec<PinnedPr>> {
    list_pinned_at(&projects_path(), project_id)
}

fn list_pinned_at(path: &Path, project_id: &str) -> Result<Vec<PinnedPr>> {
    let root = load_value(path)?;
    let projects = root
        .get("projects")
        .and_then(|v| v.as_array())
        .context("projects.json projects must be an array")?;
    let project = projects
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(project_id))
        .with_context(|| format!("Project not found: {project_id}"))?;
    Ok(parse_saved_prs(project))
}

/// Set of pinned PR numbers for a project (empty if project missing).
pub fn pinned_numbers(project_id: &str) -> std::collections::HashSet<u64> {
    list_pinned(project_id)
        .map(|v| v.into_iter().map(|e| e.number).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize env-var tests so parallel cargo test doesn't race on ER_PROJECTS_JSON.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_projects<F, R>(f: F) -> R
    where
        F: FnOnce(&Path) -> R,
    {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        std::env::set_var("ER_PROJECTS_JSON", &path);
        let result = f(&path);
        std::env::remove_var("ER_PROJECTS_JSON");
        result
    }

    #[test]
    fn pin_unpin_list_round_trip() {
        with_temp_projects(|path| {
            let (id, name) = ensure_project_for_remote_at(path, "Acme", "Widgets").unwrap();
            assert_eq!(name, "acme/widgets");
            assert!(id.starts_with("remote-"));

            let pinned = pin_pr_at(path, &id, 42, "Fix thing").unwrap();
            assert_eq!(pinned.number, 42);
            assert_eq!(pinned.title, "Fix thing");

            let list = list_pinned_at(path, &id).unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].number, 42);

            // Upsert moves to front and updates title.
            pin_pr_at(path, &id, 7, "Other").unwrap();
            pin_pr_at(path, &id, 42, "Fix thing v2").unwrap();
            let list = list_pinned_at(path, &id).unwrap();
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].number, 42);
            assert_eq!(list[0].title, "Fix thing v2");
            assert_eq!(list[1].number, 7);

            assert!(unpin_pr_at(path, &id, 42).unwrap());
            assert!(!unpin_pr_at(path, &id, 42).unwrap());
            let list = list_pinned_at(path, &id).unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].number, 7);
        });
    }

    #[test]
    fn preserves_unknown_project_fields() {
        with_temp_projects(|path| {
            let initial = json!({
                "active_id": "p1",
                "projects": [{
                    "id": "p1",
                    "name": "Widgets",
                    "root_path": "/tmp/widgets",
                    "remote": "acme/widgets",
                    "tracked_prs": [99],
                    "auto_triage": true,
                    "auto_triage_when": "new-only",
                    "custom_future_field": {"nested": true},
                    "saved_prs": []
                }]
            });
            save_value(path, &initial).unwrap();

            pin_pr_at(path, "p1", 5, "Pinned").unwrap();

            let root = load_value(path).unwrap();
            let p = &root["projects"][0];
            assert_eq!(p["tracked_prs"], json!([99]));
            assert_eq!(p["auto_triage"], json!(true));
            assert_eq!(p["auto_triage_when"], json!("new-only"));
            assert_eq!(p["custom_future_field"], json!({"nested": true}));
            assert_eq!(p["root_path"], json!("/tmp/widgets"));
            let saved = parse_saved_prs(p);
            assert_eq!(saved.len(), 1);
            assert_eq!(saved[0].number, 5);
        });
    }

    #[test]
    fn ensure_reuses_existing_remote_project() {
        with_temp_projects(|path| {
            let (id1, _) = ensure_project_for_remote_at(path, "acme", "widgets").unwrap();
            let (id2, _) = ensure_project_for_remote_at(path, "ACME", "Widgets").unwrap();
            assert_eq!(id1, id2);
            let root = load_value(path).unwrap();
            assert_eq!(root["projects"].as_array().unwrap().len(), 1);
        });
    }

    #[test]
    fn resolve_explicit_project_id() {
        with_temp_projects(|path| {
            let initial = json!({
                "projects": [{
                    "id": "local-1",
                    "name": "Local",
                    "root_path": "/x",
                    "remote": "acme/widgets",
                    "saved_prs": []
                }]
            });
            save_value(path, &initial).unwrap();
            let (id, name) =
                resolve_project_for_pin_at(path, Some("local-1"), "acme", "widgets", true).unwrap();
            assert_eq!(id, "local-1");
            assert_eq!(name, "Local");
        });
    }
}
