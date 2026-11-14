//! Off-actor effects: the code that may block.
//!
//! This module is the only place in the crate that calls a port. It runs on an
//! effect thread, never on the actor loop, so its `deps.ports()` call passes
//! the guard.

use crate::actor::WorkspaceDeps;
use crate::handle::WorkspaceError;
use crate::protocol::OpenTarget;
use crate::workspace::WorkspaceTarget;

/// Resolves what to open: parse the target, ask GitHub for the PR state, and
/// return the typed target the actor applies.
pub(crate) fn resolve_target(
    deps: &WorkspaceDeps,
    target: &OpenTarget,
) -> Result<WorkspaceTarget, WorkspaceError> {
    let parts = pr_parts(target)?;
    let remote = if parts.owner.is_empty() || parts.repo.is_empty() {
        String::new()
    } else {
        format!("{}/{}", parts.owner, parts.repo)
    };
    let pr_state = if remote.is_empty() {
        None
    } else {
        Some(
            deps.ports()
                .github
                .pr_state(&parts.owner, &parts.repo, parts.number)
                .map_err(|error| WorkspaceError::Failed(error.to_string()))?,
        )
    };
    Ok(WorkspaceTarget {
        repo_root: target.repo_root().to_string(),
        remote,
        pr_number: Some(parts.number),
        branch: None,
        label: label(&parts),
        pr_state,
    })
}

struct PrParts {
    owner: String,
    repo: String,
    number: u64,
}

fn pr_parts(target: &OpenTarget) -> Result<PrParts, WorkspaceError> {
    match target {
        OpenTarget::PrUrl { url, .. } => {
            let pr = er_github::parse_github_pr_url(url)
                .ok_or_else(|| WorkspaceError::Failed(format!("not a GitHub PR URL: {url}")))?;
            Ok(PrParts {
                owner: pr.owner,
                repo: pr.repo,
                number: pr.number,
            })
        }
        OpenTarget::RemotePr {
            owner,
            repo,
            number,
            ..
        } => Ok(PrParts {
            owner: owner.clone(),
            repo: repo.clone(),
            number: *number,
        }),
        OpenTarget::PrReview {
            remote, pr_number, ..
        } => {
            let (owner, repo) = remote.split_once('/').unwrap_or_default();
            Ok(PrParts {
                owner: owner.to_string(),
                repo: repo.to_string(),
                number: *pr_number,
            })
        }
    }
}

fn label(parts: &PrParts) -> String {
    if parts.owner.is_empty() || parts.repo.is_empty() {
        format!("pr-{}", parts.number)
    } else {
        format!("{}/{}#{}", parts.owner, parts.repo, parts.number)
    }
}
