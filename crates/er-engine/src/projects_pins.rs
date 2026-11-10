//! The projects record: the app's project list and each project's saved PRs.
//!
//! Projects are one app-scoped artifact in the store (`app/projects.json`),
//! written only here — by the desktop, the engine's pin helpers and MCP alike.
//! The payload keeps unknown fields on each project so a record written by a
//! newer build survives a read-modify-write by this one.

use anyhow::{bail, Context, Result};
use er_store::{Artifact, ArtifactKey, Loaded, Mode, UpdateError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PR_HISTORY: usize = 50;

/// The app's project list, as one store artifact.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectsFile {
    #[serde(default)]
    pub projects: Vec<ProjectRecord>,
    #[serde(default)]
    pub active_id: Option<String>,
}

/// One project row. Every field is optional on read so a record written by an
/// older build loads; `extra` keeps fields this build does not know.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    /// Empty means "current only".
    #[serde(default)]
    pub tracked_branches: Vec<String>,
    /// Branches hidden from the sidebar via "Remove from view".
    #[serde(default)]
    pub dismissed_branches: Vec<String>,
    /// PRs the user has opened for review, most recent first (max 50 persisted).
    #[serde(default)]
    pub recent_prs: Vec<RecentPrEntry>,
    /// PRs the user has manually bookmarked (max 50 persisted).
    #[serde(default)]
    pub saved_prs: Vec<SavedPrEntry>,
    #[serde(default)]
    pub auto_triage: bool,
    #[serde(default)]
    pub auto_triage_own_prs: bool,
    #[serde(default = "default_auto_triage_when")]
    pub auto_triage_when: String,
    #[serde(default)]
    pub auto_triage_max_diff_kb: u32,
    #[serde(default)]
    pub review_ignore_globs: Vec<String>,
    /// Fields written by another build, preserved across a read-modify-write.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

fn default_auto_triage_when() -> String {
    "new-and-push".to_string()
}

impl Default for ProjectRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            root_path: String::new(),
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
            extra: serde_json::Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecentPrEntry {
    pub number: u64,
    pub viewed_at_ms: u64,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SavedPrEntry {
    pub number: u64,
    pub saved_at_ms: u64,
    #[serde(default)]
    pub title: String,
}

impl Artifact for ProjectsFile {
    const KIND: &'static str = "projects";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Enveloped;
    const FILE: &'static str = "projects.json";
    type Slot = ();
}

/// One entry in a project's `saved_prs` array (Desktop Saved sidebar).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedPr {
    pub number: u64,
    pub saved_at_ms: u64,
    #[serde(default)]
    pub title: String,
}

fn projects_key() -> Result<ArtifactKey<ProjectsFile>> {
    crate::store_registry::app_key::<ProjectsFile>()
        .map_err(|error| anyhow::anyhow!("projects record key: {error}"))
}

/// Path of the projects record, for callers that watch its mtime.
pub fn projects_artifact_path() -> Option<PathBuf> {
    crate::store_registry::app_path::<ProjectsFile>()
}

/// Last-modified time of the projects record, for the desktop's parse cache.
/// `None` when the record does not exist or its time cannot be read.
pub fn modified() -> Option<SystemTime> {
    crate::store_registry::app_modified::<ProjectsFile>()
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Why the import stopped without writing; both abort silently.
#[derive(Debug)]
enum LegacyImportAborted {
    AlreadyPresent,
    UnreadableRecord,
}

/// Import the pre-1.0 project list exactly once (owner decision D15).
///
/// Runs only when the record is absent, and checks again inside the scope lock,
/// so two processes starting together import at most once. An unparseable or
/// unreadable old file logs one warning and imports nothing. Returns whether a
/// record was written; the marker file it leaves is for the first-launch
/// notice only — the record's existence is what makes "once" hold.
pub(crate) fn import_legacy_projects_once() -> bool {
    let Ok(key) = projects_key() else {
        return false;
    };
    let store = crate::store_registry::store();
    // Once the record exists, the old file is never read again: deleting every
    // project writes an empty record, so nothing comes back.
    if store.exists(&key) {
        return false;
    }
    let Some(legacy) = crate::legacy_projects::read_legacy_projects() else {
        return false;
    };
    let result = store.update_durable(&key, er_store::DEFAULT_LOCK_TIMEOUT, |current| {
        if current.is_some() {
            return Err(LegacyImportAborted::AlreadyPresent);
        }
        // An unreadable record also reads as `None`; its file exists, so it is
        // not absent and keeps the refusal `update` gives it.
        if store.exists(&key) {
            return Err(LegacyImportAborted::UnreadableRecord);
        }
        Ok(legacy.clone())
    });
    match result {
        Ok(_) => {
            crate::legacy_projects::mark_imported();
            true
        }
        Err(UpdateError::Apply(_)) => false,
        Err(UpdateError::Store(error)) => {
            log::warn!("cannot import the old project list into the store: {error}");
            false
        }
    }
}

/// Everything in the projects record, or an empty one when it is absent or
/// unreadable — the same answer a fresh install gets.
pub fn load() -> ProjectsFile {
    import_legacy_projects_once();
    let Ok(key) = projects_key() else {
        return ProjectsFile::default();
    };
    match crate::store_registry::store().read(&key) {
        Ok(Loaded::Found(stored)) => stored.value,
        Ok(Loaded::Absent | Loaded::Unreadable(_)) | Err(_) => ProjectsFile::default(),
    }
}

/// The one writer. `op` returns whether it changed anything; the record is
/// written either way so a read-modify-write stays a single locked step.
pub fn update(op: impl FnOnce(&mut ProjectsFile) -> Result<bool>) -> Result<()> {
    let key = projects_key()?;
    let store = crate::store_registry::store();
    let mut imported = false;
    let result = store.update_durable(&key, er_store::DEFAULT_LOCK_TIMEOUT, |current| {
        // `update` cannot tell absent from unreadable, but the file can.
        if current.is_none() && store.exists(&key) {
            return Err(anyhow::anyhow!("the projects record is unreadable"));
        }
        let mut file = match current {
            Some(file) => file,
            // First write on a fresh install: seed from the old project list
            // before the caller's edit, so a pin arriving before any load does
            // not skip the one-time import.
            None => match crate::legacy_projects::read_legacy_projects() {
                Some(legacy) => {
                    imported = true;
                    legacy
                }
                None => ProjectsFile::default(),
            },
        };
        op(&mut file)?;
        Ok(file)
    });
    let outcome = match result {
        Ok(_) => Ok(()),
        Err(UpdateError::Apply(error)) => Err(error),
        Err(UpdateError::Store(error)) => Err(error.into()),
    };
    if outcome.is_ok() && imported {
        crate::legacy_projects::mark_imported();
    }
    outcome
}

/// Normalize a GitHub remote reference to a lowercase `owner/repo` slug.
///
/// Input the parser cannot read falls back to the old trimmed, lowercased
/// text, so a caller keyed on garbage keeps its previous key.
pub fn normalize_remote_slug(remote: &str) -> String {
    crate::github::RepoSlug::parse(remote)
        .map(|slug| slug.key())
        .unwrap_or_else(|| remote.trim().to_ascii_lowercase())
}

fn valid_remote_slug(remote: &str) -> Option<String> {
    crate::github::RepoSlug::parse(remote).map(|slug| slug.key())
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

/// Review-ignore globs configured for a repository in the projects record.
///
/// Repository roots use exact string matching to preserve the desktop's existing
/// semantics. Remote matching is the fallback for projects without a matching root.
pub fn review_ignore_globs_for_repo(repo_root: &str, remote: Option<&str>) -> Vec<String> {
    let file = load();
    let by_root = (!repo_root.is_empty()).then(|| {
        file.projects
            .iter()
            .find(|project| !project.root_path.is_empty() && project.root_path == repo_root)
    });
    let project = by_root.flatten().or_else(|| {
        let remote = remote.and_then(valid_remote_slug)?;
        file.projects.iter().find(|project| {
            project
                .remote
                .as_deref()
                .and_then(valid_remote_slug)
                .as_deref()
                == Some(remote.as_str())
        })
    });
    project
        .map(|project| project.review_ignore_globs.clone())
        .unwrap_or_default()
}

fn find_project_by_id<'a>(file: &'a ProjectsFile, project_id: &str) -> Option<&'a ProjectRecord> {
    file.projects.iter().find(|p| p.id == project_id)
}

fn project_by_id(project_id: &str) -> Result<(String, String)> {
    let file = load();
    let project = find_project_by_id(&file, project_id)
        .with_context(|| format!("Project not found: {project_id}"))?;
    Ok((project.id.clone(), project.name.clone()))
}

fn find_by_remote(file: &ProjectsFile, remote: &str) -> Result<Option<(String, String)>> {
    for project in &file.projects {
        let existing = project.remote.as_deref().and_then(valid_remote_slug);
        if existing.as_deref() == Some(remote) {
            if project.id.is_empty() {
                bail!("project for {remote} has empty id");
            }
            let name = if project.name.is_empty() {
                remote.to_string()
            } else {
                project.name.clone()
            };
            return Ok(Some((project.id.clone(), name)));
        }
    }
    Ok(None)
}

/// Find an existing project for `owner/repo` without creating one.
pub fn find_project_for_remote(owner: &str, repo: &str) -> Result<Option<(String, String)>> {
    let remote = valid_remote_slug(&format!("{owner}/{repo}"))
        .with_context(|| format!("invalid GitHub remote slug: {owner}/{repo}"))?;
    find_by_remote(&load(), &remote)
}

fn unique_remote_project_id(file: &ProjectsFile, remote: &str) -> String {
    let base_id = format!("remote-{}", sanitize_id(remote));
    let mut unique_id = base_id.clone();
    let mut n = 2;
    while file.projects.iter().any(|p| p.id == unique_id) {
        unique_id = format!("{base_id}-{n}");
        n += 1;
    }
    unique_id
}

/// Find or create a Desktop project for `owner/repo`. Returns `(project_id, project_name)`.
pub fn ensure_project_for_remote(owner: &str, repo: &str) -> Result<(String, String)> {
    let remote = valid_remote_slug(&format!("{owner}/{repo}"))
        .with_context(|| format!("invalid GitHub remote slug: {owner}/{repo}"))?;
    let mut found: Option<(String, String)> = None;
    update(|file| {
        if let Some(existing) = find_by_remote(file, &remote)? {
            found = Some(existing);
            return Ok(true);
        }
        let id = unique_remote_project_id(file, &remote);
        file.projects.push(ProjectRecord {
            id: id.clone(),
            name: remote.clone(),
            root_path: String::new(),
            remote: Some(remote.clone()),
            ..Default::default()
        });
        found = Some((id, remote.clone()));
        Ok(true)
    })?;
    found.context("project was not created")
}

/// Resolve a project id for pinning: explicit id, else match by remote, else create.
pub fn resolve_project_for_pin(
    project_id: Option<&str>,
    owner: &str,
    repo: &str,
) -> Result<(String, String)> {
    match project_id {
        Some(id) => project_by_id(id),
        None => ensure_project_for_remote(owner, repo),
    }
}

/// Resolve a project for list tools: explicit id or remote match — never creates.
pub fn resolve_project_for_list(
    project_id: Option<&str>,
    owner: &str,
    repo: &str,
) -> Result<Option<(String, String)>> {
    if let Some(id) = project_id {
        return project_by_id(id).map(Some);
    }
    find_project_for_remote(owner, repo)
}

/// Pin a PR into the project's `saved_prs` (Desktop Saved). Upserts, newest first, max 50.
pub fn pin_pr(project_id: &str, number: u64, title: &str) -> Result<PinnedPr> {
    let entry = PinnedPr {
        number,
        saved_at_ms: now_epoch_ms(),
        title: title.to_string(),
    };
    let mut written = false;
    update(|file| {
        let project = file
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .with_context(|| format!("Project not found: {project_id}"))?;
        project.saved_prs.retain(|e| e.number != number);
        project.saved_prs.insert(
            0,
            SavedPrEntry {
                number: entry.number,
                saved_at_ms: entry.saved_at_ms,
                title: entry.title.clone(),
            },
        );
        project.saved_prs.truncate(MAX_PR_HISTORY);
        written = true;
        Ok(true)
    })?;
    if !written {
        bail!("Project not found: {project_id}");
    }
    Ok(entry)
}

/// Remove a PR from the project's `saved_prs`. Returns whether an entry was removed.
pub fn unpin_pr(project_id: &str, number: u64) -> Result<bool> {
    let mut removed = false;
    update(|file| {
        let project = file
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .with_context(|| format!("Project not found: {project_id}"))?;
        let before = project.saved_prs.len();
        project.saved_prs.retain(|e| e.number != number);
        removed = project.saved_prs.len() != before;
        Ok(removed)
    })?;
    Ok(removed)
}

fn saved_prs(project_id: &str) -> Result<Vec<PinnedPr>> {
    let file = load();
    let project = find_project_by_id(&file, project_id)
        .with_context(|| format!("Project not found: {project_id}"))?;
    Ok(project
        .saved_prs
        .iter()
        .map(|entry| PinnedPr {
            number: entry.number,
            saved_at_ms: entry.saved_at_ms,
            title: entry.title.clone(),
        })
        .collect())
}

/// List pinned PRs for a project (most recently pinned first).
pub fn list_pinned(project_id: &str) -> Result<Vec<PinnedPr>> {
    saved_prs(project_id)
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
    use serde_json::json;

    fn with_temp_store<F: FnOnce()>(f: F) {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), f);
    }

    fn write_projects(file: &ProjectsFile) {
        let key = projects_key().unwrap();
        crate::store_registry::store()
            .write_durable(&key, file)
            .unwrap();
    }

    fn project(id: &str, root_path: &str, remote: Option<&str>, globs: &[&str]) -> ProjectRecord {
        ProjectRecord {
            id: id.to_string(),
            name: id.to_string(),
            root_path: root_path.to_string(),
            remote: remote.map(str::to_string),
            review_ignore_globs: globs.iter().map(|glob| glob.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn pin_unpin_list_round_trip() {
        with_temp_store(|| {
            let (id, name) = ensure_project_for_remote("Acme", "Widgets").unwrap();
            assert_eq!(name, "acme/widgets");
            assert!(id.starts_with("remote-"));

            let pinned = pin_pr(&id, 42, "Fix thing").unwrap();
            assert_eq!(pinned.number, 42);
            assert_eq!(pinned.title, "Fix thing");

            let list = list_pinned(&id).unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].number, 42);

            // Upsert moves to front and updates title.
            pin_pr(&id, 7, "Other").unwrap();
            pin_pr(&id, 42, "Fix thing v2").unwrap();
            let list = list_pinned(&id).unwrap();
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].number, 42);
            assert_eq!(list[0].title, "Fix thing v2");
            assert_eq!(list[1].number, 7);

            assert!(unpin_pr(&id, 42).unwrap());
            assert!(!unpin_pr(&id, 42).unwrap());
            let list = list_pinned(&id).unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].number, 7);
        });
    }

    #[test]
    fn preserves_unknown_project_fields() {
        with_temp_store(|| {
            let mut extra = serde_json::Map::new();
            extra.insert("custom_future_field".into(), json!({"nested": true}));
            let mut record = project("p1", "/tmp/widgets", Some("acme/widgets"), &[]);
            record.name = "Widgets".into();
            record.tracked_prs = vec![99];
            record.auto_triage = true;
            record.auto_triage_when = "new-only".into();
            record.extra = extra;
            write_projects(&ProjectsFile {
                projects: vec![record],
                active_id: Some("p1".into()),
            });

            pin_pr("p1", 5, "Pinned").unwrap();

            let loaded = load();
            let p = &loaded.projects[0];
            assert_eq!(p.tracked_prs, vec![99]);
            assert!(p.auto_triage);
            assert_eq!(p.auto_triage_when, "new-only");
            assert_eq!(p.extra["custom_future_field"], json!({"nested": true}));
            assert_eq!(p.root_path, "/tmp/widgets");
            assert_eq!(p.saved_prs.len(), 1);
            assert_eq!(p.saved_prs[0].number, 5);
            assert_eq!(loaded.active_id.as_deref(), Some("p1"));
        });
    }

    #[test]
    fn ensure_reuses_existing_remote_project() {
        with_temp_store(|| {
            let (id1, _) = ensure_project_for_remote("acme", "widgets").unwrap();
            let (id2, _) = ensure_project_for_remote("ACME", "Widgets").unwrap();
            assert_eq!(id1, id2);
            assert_eq!(load().projects.len(), 1);
        });
    }

    #[test]
    fn resolve_explicit_project_id() {
        with_temp_store(|| {
            let mut record = project("local-1", "/x", Some("acme/widgets"), &[]);
            record.name = "Local".into();
            write_projects(&ProjectsFile {
                projects: vec![record],
                active_id: None,
            });
            let (id, name) = resolve_project_for_pin(Some("local-1"), "acme", "widgets").unwrap();
            assert_eq!(id, "local-1");
            assert_eq!(name, "Local");
        });
    }

    #[test]
    fn update_refuses_an_unreadable_record_without_replacing_it() {
        with_temp_store(|| {
            let path = projects_artifact_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"{").unwrap();

            let error = update(|_| Ok(true)).unwrap_err();

            assert!(error.to_string().contains("unreadable"), "{error}");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                b"{",
                "the unreadable record must not be replaced"
            );
        });
    }

    #[test]
    fn projects_load_of_missing_record_is_empty_not_error() {
        with_temp_store(|| {
            let file = load();
            assert!(file.projects.is_empty());
            assert_eq!(file.active_id, None);
        });
    }

    #[test]
    fn review_ignore_globs_root_match_wins_even_when_empty() {
        with_temp_store(|| {
            write_projects(&ProjectsFile {
                projects: vec![
                    project("a", "/other", Some("acme/widgets"), &["remote/**"]),
                    project("b", "/repo", Some("other/repo"), &[]),
                    project("c", "/repo", None, &["later/**"]),
                ],
                active_id: None,
            });

            assert!(review_ignore_globs_for_repo("/repo", Some("acme/widgets")).is_empty());
            assert_eq!(
                review_ignore_globs_for_repo("/unmatched", Some("acme/widgets")),
                vec!["remote/**"]
            );
        });
    }

    #[test]
    fn review_ignore_globs_remote_accepts_https_ssh_and_slug_forms() {
        with_temp_store(|| {
            write_projects(&ProjectsFile {
                projects: vec![project(
                    "a",
                    "",
                    Some("ACME/Widgets.git"),
                    &["generated/**"],
                )],
                active_id: None,
            });

            for remote in [
                "https://github.com/acme/widgets.git",
                "git@github.com:acme/widgets.git",
                "acme/widgets",
            ] {
                assert_eq!(
                    review_ignore_globs_for_repo("/repo", Some(remote)),
                    vec!["generated/**"]
                );
            }
            assert!(
                review_ignore_globs_for_repo("/repo", Some("github.com/acme/widgets")).is_empty()
            );
            assert!(review_ignore_globs_for_repo("/repo", None).is_empty());
        });
    }

    #[test]
    fn review_ignore_globs_defaults_missing_field() {
        with_temp_store(|| {
            write_projects(&ProjectsFile {
                projects: vec![
                    project("a", "/mixed", None, &["one/**", "two.txt"]),
                    project("b", "/missing", None, &[]),
                ],
                active_id: None,
            });

            assert_eq!(
                review_ignore_globs_for_repo("/mixed", None),
                vec!["one/**", "two.txt"]
            );
            assert!(review_ignore_globs_for_repo("/missing", None).is_empty());
        });
    }

    #[test]
    fn review_ignore_globs_returns_empty_for_missing_unreadable_or_malformed_store() {
        with_temp_store(|| {
            assert!(review_ignore_globs_for_repo("/repo", None).is_empty());

            let path = projects_artifact_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"{").unwrap();
            assert!(review_ignore_globs_for_repo("/repo", None).is_empty());

            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            assert!(review_ignore_globs_for_repo("/repo", None).is_empty());
        });
    }

    #[test]
    fn review_ignore_globs_empty_root_does_not_match_empty_repo_root() {
        with_temp_store(|| {
            write_projects(&ProjectsFile {
                projects: vec![project("a", "", None, &["ignored/**"])],
                active_id: None,
            });

            assert!(review_ignore_globs_for_repo("", None).is_empty());
        });
    }

    // ── the one-time pre-1.0 import (D15) ──

    fn with_legacy_env<F: FnOnce(&std::path::Path, &std::path::Path)>(f: F) {
        let store = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let store_path = store.path().to_path_buf();
        let config_path = config.path().to_path_buf();
        crate::storage::with_test_storage_root(&store_path, || {
            crate::test_support::with_var(
                "ER_LEGACY_CONFIG_DIR",
                config_path.to_string_lossy().into_owned(),
                || f(&store_path, &config_path),
            );
        });
    }

    fn write_legacy(config: &std::path::Path, text: &str) -> std::path::PathBuf {
        let dir = config.join("er");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("projects.json");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn legacy_json() -> String {
        serde_json::json!({
            "active_id": "p1",
            "projects": [
                {
                    "id": "p1",
                    "name": "One",
                    "root_path": "/repos/one",
                    "remote": "acme/one",
                    "tracked_branches": ["main"],
                    "tracked_prs": [7],
                    "saved_prs": [{"number": 42, "saved_at_ms": 111, "title": "Fix"}]
                },
                {"id": "p2", "name": "Two", "root_path": "/repos/two"}
            ]
        })
        .to_string()
    }

    #[test]
    fn an_absent_record_imports_the_old_project_list() {
        with_legacy_env(|_store, config| {
            write_legacy(config, &legacy_json());

            let file = load();

            assert_eq!(file.active_id.as_deref(), Some("p1"));
            assert_eq!(file.projects.len(), 2);
            let one = &file.projects[0];
            assert_eq!(one.id, "p1");
            assert_eq!(one.remote.as_deref(), Some("acme/one"));
            assert_eq!(one.tracked_branches, ["main"]);
            assert_eq!(one.tracked_prs, [7]);
            assert_eq!(one.saved_prs[0].number, 42);
            assert_eq!(one.saved_prs[0].saved_at_ms, 111);
            assert_eq!(one.saved_prs[0].title, "Fix");
            assert!(projects_artifact_path().unwrap().exists());
        });
    }

    #[test]
    fn an_existing_record_never_reads_the_old_file() {
        with_legacy_env(|_store, config| {
            write_projects(&ProjectsFile {
                active_id: Some("keep".into()),
                projects: vec![project("keep", "/repos/keep", None, &[])],
            });
            // A valid old list with different projects: importing it would
            // replace the record, so the store showing "keep" proves it was
            // not read.
            write_legacy(config, &legacy_json());

            let file = load();

            assert_eq!(file.active_id.as_deref(), Some("keep"));
            assert_eq!(file.projects.len(), 1);
            assert_eq!(file.projects[0].id, "keep");
        });
    }

    #[test]
    fn deleting_every_project_does_not_reimport() {
        with_legacy_env(|_store, config| {
            write_legacy(config, &legacy_json());
            assert_eq!(load().projects.len(), 2);

            update(|file| {
                file.projects.clear();
                file.active_id = None;
                Ok(true)
            })
            .unwrap();

            assert!(load().projects.is_empty());
        });
    }

    #[test]
    fn an_unparseable_old_file_imports_nothing_and_keeps_the_old_bytes() {
        with_legacy_env(|_store, config| {
            let path = write_legacy(config, "{ not json");
            let before = std::fs::read(&path).unwrap();

            assert!(load().projects.is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(!projects_artifact_path().unwrap().exists());
        });
    }

    #[test]
    fn an_unreadable_record_does_not_import() {
        with_legacy_env(|_store, config| {
            let record = projects_artifact_path().unwrap();
            std::fs::create_dir_all(record.parent().unwrap()).unwrap();
            std::fs::write(&record, b"not a projects envelope").unwrap();
            write_legacy(config, &legacy_json());

            assert!(load().projects.is_empty());
            assert_eq!(
                std::fs::read(&record).unwrap(),
                b"not a projects envelope",
                "the unreadable record must keep its bytes"
            );
        });
    }

    #[test]
    fn the_old_file_is_never_modified() {
        with_legacy_env(|_store, config| {
            let path = write_legacy(config, &legacy_json());
            let before = std::fs::read(&path).unwrap();
            let before_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

            assert_eq!(load().projects.len(), 2);

            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(
                std::fs::metadata(&path).unwrap().modified().unwrap(),
                before_mtime
            );
        });
    }

    #[test]
    fn a_pin_before_load_still_imports_the_old_list() {
        with_legacy_env(|_store, config| {
            write_legacy(config, &legacy_json());

            // The MCP pin path reaches `update` before anything calls `load`.
            let (id, _) = ensure_project_for_remote("Acme", "New").unwrap();

            let file = load();
            let ids: Vec<&str> = file.projects.iter().map(|p| p.id.as_str()).collect();
            assert_eq!(id, "remote-acme-new");
            assert!(ids.contains(&"remote-acme-new"), "{ids:?}");
            assert!(ids.contains(&"p1"), "legacy projects lost: {ids:?}");
            assert!(ids.contains(&"p2"), "legacy projects lost: {ids:?}");
        });
    }
}
