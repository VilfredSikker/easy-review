//! Read the Easy Review projects record from the app store.

use anyhow::{Context, Result};
use er_engine::pr_resolve::{self, ProjectHint, ResolvePrRefInput, ResolvedPrRef};
use er_engine::projects_pins::{ProjectRecord, ProjectsFile};

/// The `projects_list` tool description. The macro needs an expression it can
/// read, so the description lives here and the test below pins its wording.
pub const PROJECTS_LIST_DESCRIPTION: &str = "List Easy Review projects from the app store.";

pub fn load_projects() -> ProjectsFile {
    er_engine::projects_pins::load()
}

pub fn project_hints(file: &ProjectsFile) -> Vec<ProjectHint> {
    file.projects
        .iter()
        .map(|p| ProjectHint {
            id: p.id.clone(),
            name: p.name.clone(),
            root_path: p.root_path.clone(),
            remote: p.remote.clone(),
        })
        .collect()
}

/// Normalize `owner/repo` or a GitHub URL into `(owner, repo)`.
pub fn parse_repo_slug(remote: &str) -> Result<(String, String)> {
    er_engine::github::RepoSlug::parse(remote)
        .map(|slug| (slug.owner, slug.repo))
        .with_context(|| format!("invalid repo slug: {remote}"))
}

/// Resolved PR target for MCP tools.
pub type ResolvedPr = ResolvedPrRef;

#[derive(Debug, Clone, Default)]
pub struct PrTargetInput<'a> {
    pub ref_str: Option<&'a str>,
    pub pr_url: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub number: Option<u64>,
}

pub fn resolve_pr_target(input: &PrTargetInput<'_>) -> Result<ResolvedPr> {
    let file = load_projects();
    let hints = project_hints(&file);
    pr_resolve::resolve_pr_ref(
        &*er_engine::github::default_handle(),
        &ResolvePrRefInput {
            ref_str: input.ref_str,
            pr_url: input.pr_url,
            repo: input.repo,
            project_id: input.project_id,
            number: input.number,
            projects: &hints,
            active_project_id: file.active_id.as_deref(),
        },
    )
}

/// Resolve `(owner, repo)` from explicit slug, project id, or active project.
pub fn resolve_repo(
    repo: Option<&str>,
    project_id: Option<&str>,
) -> Result<(String, String, Option<String>)> {
    let file = load_projects();
    if let Some(slug) = repo {
        let (o, r) = parse_repo_slug(slug)?;
        return Ok((o, r, None));
    }
    let project: &ProjectRecord = if let Some(id) = project_id {
        file.projects
            .iter()
            .find(|p| p.id == id)
            .with_context(|| format!("project not found: {id}"))?
    } else if let Some(active) = file.active_id.as_deref() {
        file.projects
            .iter()
            .find(|p| p.id == active)
            .context("active project missing from projects")?
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

    #[test]
    fn resolve_pr_from_url() {
        let dir = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(dir.path(), || {
            let pr = resolve_pr_target(&PrTargetInput {
                pr_url: Some("https://github.com/acme/widgets/pull/42/files"),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(pr.owner, "acme");
            assert_eq!(pr.repo, "widgets");
            assert_eq!(pr.number, 42);
            assert!(pr.bucket_path.contains("prs/pr-42"));
        });
    }

    #[test]
    fn resolve_pr_from_repo_and_number() {
        let dir = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(dir.path(), || {
            let pr = resolve_pr_target(&PrTargetInput {
                repo: Some("acme/widgets"),
                number: Some(7),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(pr.number, 7);
            assert!(pr.bucket_path.contains("prs/pr-7"));
        });
    }

    #[test]
    fn mcp_projects_list_description_names_no_old_path() {
        let description = PROJECTS_LIST_DESCRIPTION;
        assert!(!description.contains(".config/er"), "{description}");
        assert!(
            description.to_lowercase().contains("store"),
            "{description}"
        );
    }
}
