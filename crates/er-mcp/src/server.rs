//! MCP tool surface for Easy Review PR triage.

use er_engine::git::ProdDiffStats;
use er_engine::github::{gh_pr_list_queue, gh_pr_prod_diff_stats};
use er_engine::review_queue::{
    filter_by_status, rank_low_hanging, rank_priority, score_pr, QueuePr, ReviewStatus,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::projects::{self, resolve_repo};

#[derive(Clone)]
pub struct ErMcp {
    /// Kept so callers can compose routers; `#[tool_handler]` uses `Self::tool_router()`.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RepoArgs {
    /// GitHub `owner/repo` (or URL). Defaults to the active Easy Review project remote.
    #[serde(default)]
    pub repo: Option<String>,
    /// Easy Review project id from `~/.config/er/projects.json`.
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LimitRepoArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Max PRs to return (default 5, max 50).
    #[serde(default)]
    pub limit: Option<u32>,
    /// When true, enrich top candidates with production-only line counts (slower: fetches diffs).
    #[serde(default)]
    pub production_lines: Option<bool>,
    /// Include draft PRs (default false for low-hanging fruit).
    #[serde(default)]
    pub include_drafts: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiffStatsArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// PR number.
    pub number: u64,
    /// Include per-file breakdown (default false).
    #[serde(default)]
    pub include_files: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StatusFilterArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// One of: ready_to_review, draft, outdated, blocked_conflicts, waiting_on_author, approved, merge_ready, inactive
    pub status: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

fn clamp_limit(limit: Option<u32>, default: u32) -> usize {
    limit.unwrap_or(default).clamp(1, 50) as usize
}

fn text_json(value: &impl Serialize) -> Result<CallToolResult, McpError> {
    let body = serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(body)]))
}

fn tool_err(msg: impl Into<String>) -> McpError {
    McpError::invalid_params(msg.into(), None)
}

async fn load_queue(
    repo: Option<&str>,
    project_id: Option<&str>,
) -> Result<(String, String, Option<String>, Vec<QueuePr>), McpError> {
    let (owner, name, project_name) =
        resolve_repo(repo, project_id).map_err(|e| tool_err(e.to_string()))?;

    let prs = tokio::task::spawn_blocking({
        let owner = owner.clone();
        let name = name.clone();
        move || gh_pr_list_queue(&owner, &name, "open", 100)
    })
    .await
    .map_err(|e| McpError::internal_error(e.to_string(), None))?
    .map_err(|e| tool_err(e.to_string()))?;

    Ok((owner, name, project_name, prs))
}

async fn enrich_production_lines(owner: &str, repo: &str, prs: &mut [QueuePr], max_enrich: usize) {
    for pr in prs.iter_mut().take(max_enrich) {
        let owner = owner.to_string();
        let repo = repo.to_string();
        let number = pr.number;
        let stats =
            tokio::task::spawn_blocking(move || gh_pr_prod_diff_stats(&owner, &repo, number))
                .await
                .ok()
                .and_then(|r| r.ok());
        if let Some(stats) = stats {
            pr.production_lines = Some(stats.production.lines_changed() as u64);
        }
    }
}

#[tool_router]
impl ErMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List Easy Review projects from ~/.config/er/projects.json (name, id, remote)."
    )]
    async fn list_projects(&self) -> Result<CallToolResult, McpError> {
        let file = projects::load_projects();
        text_json(&json!({
            "active_id": file.active_id,
            "projects": file.projects.iter().map(|p| json!({
                "id": p.id,
                "name": p.name,
                "root_path": p.root_path,
                "remote": p.remote,
            })).collect::<Vec<_>>(),
        }))
    }

    #[tool(
        description = "List open PRs for a repo (or the active Easy Review project) with size, review decision, and merge state."
    )]
    async fn list_prs(
        &self,
        Parameters(args): Parameters<RepoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (owner, name, project_name, prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;
        let ranked: Vec<_> = prs.iter().map(score_pr).collect();
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "count": ranked.len(),
            "prs": ranked,
        }))
    }

    #[tool(
        description = "Top priority PRs to review next. Scores by review request, readiness, size, labels, and blocked/outdated penalties. Ask: 'give me top 5 priority PRs to review'."
    )]
    async fn priority_prs(
        &self,
        Parameters(args): Parameters<LimitRepoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = clamp_limit(args.limit, 5);
        let (owner, name, project_name, mut prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;

        if args.production_lines.unwrap_or(false) {
            let window = (limit * 3).min(prs.len());
            enrich_production_lines(&owner, &name, &mut prs[..window], window).await;
        }

        let ranked = rank_priority(&prs, limit);
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "limit": limit,
            "production_lines_enriched": args.production_lines.unwrap_or(false),
            "prs": ranked,
        }))
    }

    #[tool(
        description = "Smallest / low-hanging-fruit open PRs (by changed lines). Optionally enrich with production-only line counts (excludes test, storybook, generated, docs)."
    )]
    async fn low_hanging_fruit(
        &self,
        Parameters(args): Parameters<LimitRepoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = clamp_limit(args.limit, 5);
        let include_drafts = args.include_drafts.unwrap_or(false);
        let (owner, name, project_name, mut prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;

        if args.production_lines.unwrap_or(true) {
            let window = prs.len().min(25);
            enrich_production_lines(&owner, &name, &mut prs[..window], window).await;
        }

        let ranked = rank_low_hanging(&prs, limit, include_drafts);
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "limit": limit,
            "sorted_by": if args.production_lines.unwrap_or(true) {
                "production_lines"
            } else {
                "total_github_lines"
            },
            "prs": ranked,
        }))
    }

    #[tool(
        description = "Diff line stats for one PR, split into production vs test / storybook / generated / docs. Use when you want production-code churn only."
    )]
    async fn pr_diff_stats(
        &self,
        Parameters(args): Parameters<DiffStatsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (owner, name, project_name) =
            resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
                .map_err(|e| tool_err(e.to_string()))?;
        let number = args.number;
        let include_files = args.include_files.unwrap_or(false);

        let stats: ProdDiffStats = tokio::task::spawn_blocking({
            let owner = owner.clone();
            let name = name.clone();
            move || gh_pr_prod_diff_stats(&owner, &name, number)
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;

        let stats = if include_files {
            stats
        } else {
            stats.summary_only()
        };

        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "number": number,
            "stats": stats,
            "production_lines": stats.production.lines_changed(),
            "total_lines": stats.total.lines_changed(),
        }))
    }

    #[tool(
        description = "List open PRs filtered by review status: ready_to_review, draft, outdated (needs rebase), blocked_conflicts, waiting_on_author, approved, merge_ready."
    )]
    async fn prs_by_status(
        &self,
        Parameters(args): Parameters<StatusFilterArgs>,
    ) -> Result<CallToolResult, McpError> {
        let status = parse_status(&args.status)?;
        let limit = clamp_limit(args.limit, 20);
        let (owner, name, project_name, prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;
        let mut ranked = filter_by_status(&prs, status);
        ranked.truncate(limit);
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "status": status.as_str(),
            "count": ranked.len(),
            "prs": ranked,
        }))
    }

    #[tool(
        description = "Ideas for additional Easy Review MCP tools (outdated, already-fixed, blocked, CI, review debt, etc.)."
    )]
    async fn tool_ideas(&self) -> Result<CallToolResult, McpError> {
        text_json(&json!({
            "shipped": [
                "list_projects",
                "list_prs",
                "priority_prs",
                "low_hanging_fruit",
                "pr_diff_stats",
                "prs_by_status",
            ],
            "ideas": TOOL_IDEAS,
        }))
    }
}

fn parse_status(s: &str) -> Result<ReviewStatus, McpError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "ready_to_review" | "ready" => Ok(ReviewStatus::ReadyToReview),
        "draft" => Ok(ReviewStatus::Draft),
        "outdated" | "behind" => Ok(ReviewStatus::Outdated),
        "blocked_conflicts" | "conflicts" | "blocked" => Ok(ReviewStatus::BlockedConflicts),
        "waiting_on_author" | "changes_requested" => Ok(ReviewStatus::WaitingOnAuthor),
        "approved" => Ok(ReviewStatus::Approved),
        "merge_ready" | "ready_to_merge" => Ok(ReviewStatus::MergeReady),
        "inactive" | "closed" | "merged" => Ok(ReviewStatus::Inactive),
        other => Err(tool_err(format!(
            "unknown status '{other}'; expected ready_to_review|draft|outdated|blocked_conflicts|waiting_on_author|approved|merge_ready"
        ))),
    }
}

const TOOL_IDEAS: &[&str] = &[
    "prs_outdated — PRs whose head is behind base (rebase needed); already available via prs_by_status status=outdated",
    "prs_blocked — conflicts, failing required checks, or missing approvals; combine merge state + CI rollup",
    "prs_waiting_on_author — CHANGES_REQUESTED or unanswered reviewer questions",
    "prs_already_addressed — all review threads resolved / outdated after new commits (\"already fixed\")",
    "prs_stale — open PRs with no push/comment activity for N days",
    "prs_failing_ci — open PRs with failing required status checks",
    "my_review_debt — PRs where I was requested and have not reviewed yet",
    "compare_prod_size — rank a batch of PR numbers by production-only lines",
    "open_in_easy_review — deep-link / instruct desktop to open owner/repo#N",
    "summarize_triage — read managed triage.json / review.json sidecars for a PR if present",
    "diff_hotspots — files in a PR with highest production churn",
    "cross_repo_queue — priority_prs across all configured Easy Review projects",
];

#[tool_handler]
impl ServerHandler for ErMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("er-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Easy Review MCP helps prioritize PR review. \
             Use priority_prs for 'top N to review', low_hanging_fruit for smallest/production-light PRs, \
             pr_diff_stats for production vs test/storybook/generated line counts, \
             and prs_by_status for outdated/blocked/waiting_on_author. \
             Pass repo=owner/repo or rely on the active Easy Review project remote. \
             Call tool_ideas for more suggested tools.",
            )
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }
}
