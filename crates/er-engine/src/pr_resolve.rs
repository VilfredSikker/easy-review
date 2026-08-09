//! Resolve a user-facing PR `ref` (URL, worktree path, owner/repo, branch, number)
//! into owner/repo/number for MCP and skills.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::git::get_repo_root_at;
use crate::github::{
    get_pr_info, gh_open_pr_number_for_head, owner_repo_storage_slug, parse_github_pr_url,
    parse_remote_url,
};
use crate::storage;

/// Easy Review project entry used when resolving `owner/repo` or active project.
#[derive(Debug, Clone)]
pub struct ProjectHint {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub remote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedPrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub pr_url: String,
    pub bucket_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    pub resolved_via: String,
}

#[derive(Debug, Clone)]
pub struct ResolvePrRefInput<'a> {
    /// Catch-all user ref (URL, path, owner/repo, branch, number).
    pub ref_str: Option<&'a str>,
    pub pr_url: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub number: Option<u64>,
    pub projects: &'a [ProjectHint],
    /// Easy Review `projects.json` `active_id` (preferred over first project).
    pub active_project_id: Option<&'a str>,
}

/// Resolve a PR target from explicit fields and/or a single `ref` string.
pub fn resolve_pr_ref(input: &ResolvePrRefInput<'_>) -> Result<ResolvedPrRef> {
    if let Some(url) = input.pr_url.map(str::trim).filter(|s| !s.is_empty()) {
        return resolve_from_pr_url(url, None, "pr_url");
    }

    if let Some(raw) = input.ref_str.map(str::trim).filter(|s| !s.is_empty()) {
        return resolve_ref_string(raw, input);
    }

    let number = input
        .number
        .context("pass ref, pr_url, or number (with repo or Easy Review project)")?;
    let (owner, repo, project_name) = resolve_repo_context(
        input.repo,
        input.project_id,
        input.projects,
        input.active_project_id,
    )?;
    finish(&owner, &repo, number, project_name, None, "repo_and_number")
}

fn resolve_ref_string(raw: &str, input: &ResolvePrRefInput<'_>) -> Result<ResolvedPrRef> {
    if parse_github_pr_url(raw).is_some() {
        return resolve_from_pr_url(raw, None, "ref_pr_url");
    }

    if let Some(path) = expand_path(raw) {
        if path.is_dir() || path.is_file() {
            if let Some(path_str) = path.to_str() {
                if let Ok(repo_root) = get_repo_root_at(path_str) {
                    return resolve_from_worktree(&repo_root, None, "ref_worktree");
                }
            }
        }
    }

    if let Some((owner, repo, number)) = parse_repo_number(raw) {
        return finish(&owner, &repo, number, None, None, "ref_repo_number");
    }

    if let Some((owner, repo)) = parse_repo_slug_pair(raw) {
        if let Some(project) = find_project_for_slug(input.projects, &owner, &repo) {
            let repo_root = project.root_path.clone();
            let project_name = Some(project.name.clone());
            return resolve_from_worktree(&repo_root, project_name, "ref_project_slug");
        }
        // `feature/auth` is a branch, not owner/repo — try branch lookup before failing.
        if let Ok((gh_owner, gh_repo, project_name, _repo_root)) = resolve_repo_with_root(
            input.repo,
            input.project_id,
            input.projects,
            input.active_project_id,
        ) {
            if let Some(number) = gh_open_pr_number_for_head(&gh_owner, &gh_repo, raw)? {
                return finish(
                    &gh_owner,
                    &gh_repo,
                    number,
                    project_name,
                    Some(raw.to_string()),
                    "ref_branch_head",
                );
            }
        }
        bail!(
            "ref '{raw}' looks like owner/repo but no Easy Review project matches — \
             use owner/repo#N, a worktree path, or configure the project in Desktop"
        );
    }

    if raw.chars().all(|c| c.is_ascii_digit()) {
        let number: u64 = raw.parse().context("invalid PR number")?;
        let (owner, repo, project_name) = resolve_repo_context(
            input.repo,
            input.project_id,
            input.projects,
            input.active_project_id,
        )?;
        return finish(&owner, &repo, number, project_name, None, "ref_number");
    }

    // Branch name — repo from explicit repo, active project, or cwd.
    let (owner, repo, project_name, repo_root) = resolve_repo_with_root(
        input.repo,
        input.project_id,
        input.projects,
        input.active_project_id,
    )?;
    if let Some(number) = gh_open_pr_number_for_head(&owner, &repo, raw)? {
        return finish(
            &owner,
            &repo,
            number,
            project_name,
            Some(raw.to_string()),
            "ref_branch_head",
        );
    }
    if let Ok((o, r, n)) = get_pr_info(&repo_root) {
        return finish(
            &o,
            &r,
            n,
            project_name,
            Some(raw.to_string()),
            "ref_branch_worktree",
        );
    }

    bail!(
        "no open PR found for branch '{raw}' in {owner}/{repo} — pass a PR URL, owner/repo#N, or a worktree path"
    )
}

fn resolve_from_pr_url(
    url: &str,
    project_name: Option<String>,
    via: &str,
) -> Result<ResolvedPrRef> {
    let pr = parse_github_pr_url(url).with_context(|| format!("invalid GitHub PR URL: {url}"))?;
    finish(&pr.owner, &pr.repo, pr.number, project_name, None, via)
}

fn resolve_from_worktree(
    repo_root: &str,
    project_name: Option<String>,
    via: &str,
) -> Result<ResolvedPrRef> {
    let (owner, repo, number) = get_pr_info(repo_root).with_context(|| {
        format!("no open PR for current branch in {repo_root} — checkout the PR branch or pass a PR URL")
    })?;
    finish(&owner, &repo, number, project_name, None, via)
}

fn finish(
    owner: &str,
    repo: &str,
    number: u64,
    project_name: Option<String>,
    branch: Option<String>,
    resolved_via: &str,
) -> Result<ResolvedPrRef> {
    let pr_url = format!("https://github.com/{owner}/{repo}/pull/{number}");
    let bucket_path = storage::pr_bucket_dir(&owner_repo_storage_slug(owner, repo), number)
        .to_string_lossy()
        .into_owned();
    Ok(ResolvedPrRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
        pr_url,
        bucket_path,
        branch,
        project_name,
        resolved_via: resolved_via.to_string(),
    })
}

fn resolve_repo_context(
    repo: Option<&str>,
    project_id: Option<&str>,
    projects: &[ProjectHint],
    active_project_id: Option<&str>,
) -> Result<(String, String, Option<String>)> {
    if let Some(slug) = repo.map(str::trim).filter(|s| !s.is_empty()) {
        let (o, r) = parse_remote_url(slug)
            .or_else(|| parse_repo_slug_pair(slug))
            .with_context(|| format!("invalid repo slug: {slug}"))?;
        return Ok((o, r, None));
    }
    let project = select_project(project_id, active_project_id, projects)?;
    let remote = project
        .remote
        .as_deref()
        .context("project has no remote; pass repo=owner/repo")?;
    let (o, r) =
        parse_remote_url(remote).with_context(|| format!("invalid project remote: {remote}"))?;
    Ok((o, r, Some(project.name.clone())))
}

fn resolve_repo_with_root(
    repo: Option<&str>,
    project_id: Option<&str>,
    projects: &[ProjectHint],
    active_project_id: Option<&str>,
) -> Result<(String, String, Option<String>, String)> {
    if let Some(slug) = repo.map(str::trim).filter(|s| !s.is_empty()) {
        let (o, r) = parse_remote_url(slug)
            .or_else(|| parse_repo_slug_pair(slug))
            .with_context(|| format!("invalid repo slug: {slug}"))?;
        if let Some(project) = find_project_for_slug(projects, &o, &r) {
            return Ok((o, r, Some(project.name.clone()), project.root_path.clone()));
        }
        bail!("no Easy Review project for {o}/{r} — pass a worktree path in ref");
    }
    let project = select_project(project_id, active_project_id, projects)?;
    let remote = project
        .remote
        .as_deref()
        .context("project has no remote; pass repo=owner/repo")?;
    let (o, r) =
        parse_remote_url(remote).with_context(|| format!("invalid project remote: {remote}"))?;
    Ok((o, r, Some(project.name.clone()), project.root_path.clone()))
}

fn select_project<'a>(
    project_id: Option<&str>,
    active_project_id: Option<&str>,
    projects: &'a [ProjectHint],
) -> Result<&'a ProjectHint> {
    if let Some(id) = project_id {
        return projects
            .iter()
            .find(|p| p.id == id)
            .with_context(|| format!("project not found: {id}"));
    }
    if let Some(active) = active_project_id {
        return projects
            .iter()
            .find(|p| p.id == active)
            .with_context(|| format!("active project missing from projects.json: {active}"));
    }
    if let Some(first) = projects.first() {
        return Ok(first);
    }
    bail!("no Easy Review projects configured; pass repo=owner/repo or ref with a PR URL")
}

fn find_project_for_slug<'a>(
    projects: &'a [ProjectHint],
    owner: &str,
    repo: &str,
) -> Option<&'a ProjectHint> {
    projects.iter().find(|p| {
        p.remote.as_deref().is_some_and(|remote| {
            parse_remote_url(remote)
                .is_some_and(|(o, r)| o.eq_ignore_ascii_case(owner) && r.eq_ignore_ascii_case(repo))
        })
    })
}

fn parse_repo_slug_pair(s: &str) -> Option<(String, String)> {
    let trimmed = s.trim().trim_end_matches('/');
    let (owner, repo) = trimmed.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || owner.contains('\\') {
        return None;
    }
    // Avoid treating filesystem paths as slugs.
    if trimmed.starts_with('/') || trimmed.starts_with('~') || trimmed.starts_with('.') {
        return None;
    }
    Some((owner.to_string(), repo.trim_end_matches(".git").to_string()))
}

fn parse_repo_number(s: &str) -> Option<(String, String, u64)> {
    let trimmed = s.trim();
    let (left, num_str) = trimmed
        .split_once('#')
        .or_else(|| trimmed.split_once('!'))?;
    let number = num_str.parse::<u64>().ok()?;
    if number == 0 {
        return None;
    }
    let (owner, repo) = parse_repo_slug_pair(left)?;
    Some((owner, repo, number))
}

fn expand_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "~" || trimmed.starts_with("~/") {
        let home = dirs::home_dir()?;
        let rest = trimmed.strip_prefix("~/").unwrap_or("");
        return Some(home.join(rest));
    }
    if trimmed.starts_with('/') || trimmed.starts_with("./") || trimmed.starts_with("../") {
        return Some(PathBuf::from(trimmed));
    }
    // Heuristic: absolute-looking macOS/Linux project paths without leading ./
    if trimmed.contains('/') && !trimmed.contains('#') {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_projects() -> Vec<ProjectHint> {
        Vec::new()
    }

    #[test]
    fn resolve_pr_url_via_ref() {
        let input = ResolvePrRefInput {
            ref_str: Some("https://github.com/acme/widgets/pull/42/files"),
            pr_url: None,
            repo: None,
            project_id: None,
            number: None,
            projects: &empty_projects(),
            active_project_id: None,
        };
        let pr = resolve_pr_ref(&input).unwrap();
        assert_eq!(pr.owner, "acme");
        assert_eq!(pr.repo, "widgets");
        assert_eq!(pr.number, 42);
        assert_eq!(pr.resolved_via, "ref_pr_url");
    }

    #[test]
    fn resolve_repo_number_hash() {
        let input = ResolvePrRefInput {
            ref_str: Some("acme/widgets#7"),
            pr_url: None,
            repo: None,
            project_id: None,
            number: None,
            projects: &empty_projects(),
            active_project_id: None,
        };
        let pr = resolve_pr_ref(&input).unwrap();
        assert_eq!(pr.number, 7);
        assert_eq!(pr.resolved_via, "ref_repo_number");
    }

    #[test]
    fn select_project_prefers_active_id() {
        let projects = vec![
            ProjectHint {
                id: "first".into(),
                name: "First".into(),
                root_path: "/a".into(),
                remote: Some("https://github.com/acme/one.git".into()),
            },
            ProjectHint {
                id: "active".into(),
                name: "Active".into(),
                root_path: "/b".into(),
                remote: Some("https://github.com/acme/two.git".into()),
            },
        ];
        let picked = select_project(None, Some("active"), &projects).unwrap();
        assert_eq!(picked.id, "active");
    }

    #[test]
    fn select_project_errors_on_stale_active_id() {
        let projects = vec![ProjectHint {
            id: "first".into(),
            name: "First".into(),
            root_path: "/a".into(),
            remote: Some("https://github.com/acme/one.git".into()),
        }];
        let err = select_project(None, Some("missing"), &projects).unwrap_err();
        assert!(err.to_string().contains("active project missing"));
    }

    #[test]
    fn parse_slug_rejects_path_like() {
        assert!(parse_repo_slug_pair("/Users/me/proj").is_none());
    }
}
