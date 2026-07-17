//! MCP tool surface for Easy Review PR triage.

use std::time::{SystemTime, UNIX_EPOCH};

use er_engine::git::ProdDiffStats;
use er_engine::github::{
    gh_pr_checks_state_remote, gh_pr_list_queue, gh_pr_prod_diff_stats,
    gh_pr_thread_addressing_remote,
};
use er_engine::headless_jobs::{
    cancel_job, job_status, list_jobs, start_job, HeadlessJobKind, HeadlessJobRequest,
};
use er_engine::review_queue::{
    filter_blocked, filter_by_status, filter_failing_ci, filter_review_debt, filter_stale,
    open_in_easy_review, rank_low_hanging, rank_priority, score_pr, QueuePr, RankedPr,
    ReviewStatus,
};
use er_engine::sidecar_summary::summarize_pr_sidecars;
use er_engine::sidecar_upload::{
    prepare_review_kit, upload_pr_artifacts, UploadArtifactsRequest,
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
    /// When true, enrich candidates with production-only line counts (slower: fetches diffs).
    #[serde(default)]
    pub production_lines: Option<bool>,
    /// Include draft PRs (default false for low-hanging fruit).
    #[serde(default)]
    pub include_drafts: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StaleArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Days without GitHub activity (default 14).
    #[serde(default)]
    pub days: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrNumberArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub number: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HotspotsArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub number: u64,
    /// Max production files to return (default 10).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompareProdArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// PR numbers to compare.
    pub numbers: Vec<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunReviewArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// PR number to review.
    pub number: u64,
    /// Optional AI provider id from `~/.config/er/config.toml`.
    #[serde(default)]
    pub provider_id: Option<String>,
    /// Optional model id.
    #[serde(default)]
    pub model_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrepareReviewArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub number: u64,
    /// Artifact kinds to prepare prompts for. Default: triage, review, tour.
    /// Allowed: triage, review, tour.
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UploadArtifactsArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub number: u64,
    /// One of: triage, review, tour.
    pub kind: String,
    /// Map of relative filename → file contents (e.g. `{"tour.json": "{...}"}`).
    pub files: std::collections::BTreeMap<String, String>,
    /// Re-fetch PR diff before validating (default false — reuse prepare_review's diff-tmp).
    #[serde(default)]
    pub refresh_diff: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobIdArgs {
    /// Job id returned by run_triage / run_review / run_tour.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CrossRepoArgs {
    /// Max PRs to return across all projects (default 10).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Enrich with production-only lines (slower).
    #[serde(default)]
    pub production_lines: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScanLimitArgs {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Max open PRs to scan for expensive enrichments (default 20, max 40).
    #[serde(default)]
    pub scan_limit: Option<u32>,
}

fn clamp_limit(limit: Option<u32>, default: u32) -> usize {
    limit.unwrap_or(default).clamp(1, 50) as usize
}

fn now_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

async fn enrich_ci(owner: &str, repo: &str, prs: &mut [QueuePr], max_enrich: usize) {
    for pr in prs.iter_mut().take(max_enrich) {
        let owner = owner.to_string();
        let repo = repo.to_string();
        let number = pr.number;
        let state = tokio::task::spawn_blocking(move || {
            gh_pr_checks_state_remote(&owner, &repo, number).unwrap_or("unknown")
        })
        .await
        .ok();
        if let Some(state) = state {
            pr.checks_state = Some(state.to_string());
        }
    }
}

#[derive(Serialize)]
struct TaggedRankedPr<'a> {
    project: Option<&'a str>,
    repo: String,
    #[serde(flatten)]
    ranked: RankedPr,
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
        description = "Top priority PRs to review next. Scores by review request, readiness, size, labels, and blocked/outdated penalties."
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
        description = "Smallest / low-hanging-fruit open PRs. Defaults to production-only line enrichment (excludes test, storybook, generated, docs)."
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
        description = "Diff line stats for one PR, split into production vs test / storybook / generated / docs."
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
        description = "List open PRs filtered by review status: ready_to_review, draft, outdated, blocked_conflicts, waiting_on_author, approved, merge_ready."
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
        description = "PRs blocked by merge conflicts, mergeStateStatus=BLOCKED, or failing CI (fetches checks). Use for 'what is blocked?'."
    )]
    async fn prs_blocked(
        &self,
        Parameters(args): Parameters<ScanLimitArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = clamp_limit(args.limit, 20);
        let scan = clamp_limit(args.scan_limit, 20).min(40);
        let (owner, name, project_name, mut prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;
        let n = prs.len().min(scan);
        enrich_ci(&owner, &name, &mut prs[..n], scan).await;
        let ranked = filter_blocked(&prs, limit);
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "count": ranked.len(),
            "prs": ranked,
        }))
    }

    #[tool(
        description = "Open PRs with failing CI checks. Fetches `gh pr checks` for a scan window."
    )]
    async fn prs_failing_ci(
        &self,
        Parameters(args): Parameters<ScanLimitArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = clamp_limit(args.limit, 20);
        let scan = clamp_limit(args.scan_limit, 20).min(40);
        let (owner, name, project_name, mut prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;
        let n = prs.len().min(scan);
        enrich_ci(&owner, &name, &mut prs[..n], scan).await;
        let ranked = filter_failing_ci(&prs, limit);
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "count": ranked.len(),
            "prs": ranked,
        }))
    }

    #[tool(
        description = "PRs where review was requested of you and you have not approved / requested changes yet (your review debt)."
    )]
    async fn my_review_debt(
        &self,
        Parameters(args): Parameters<LimitRepoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = clamp_limit(args.limit, 20);
        let (owner, name, project_name, prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;
        let ranked = filter_review_debt(&prs, limit);
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "count": ranked.len(),
            "prs": ranked,
        }))
    }

    #[tool(
        description = "Stale open PRs with no GitHub activity for N days (default 14). Uses PR updatedAt."
    )]
    async fn prs_stale(
        &self,
        Parameters(args): Parameters<StaleArgs>,
    ) -> Result<CallToolResult, McpError> {
        let days = args.days.unwrap_or(14).clamp(1, 365) as u64;
        let limit = clamp_limit(args.limit, 20);
        let (owner, name, project_name, prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;
        let ranked = filter_stale(&prs, days, now_epoch_secs(), limit);
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "days": days,
            "count": ranked.len(),
            "prs": ranked,
        }))
    }

    #[tool(
        description = "PRs where all review threads are resolved or outdated (feedback already addressed / fixed). Scans open PRs via GraphQL."
    )]
    async fn prs_already_addressed(
        &self,
        Parameters(args): Parameters<ScanLimitArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = clamp_limit(args.limit, 20);
        let scan = clamp_limit(args.scan_limit, 15).min(30);
        let (owner, name, project_name, prs) =
            load_queue(args.repo.as_deref(), args.project_id.as_deref()).await?;

        let mut out = Vec::new();
        for pr in prs
            .iter()
            .filter(|p| p.state.eq_ignore_ascii_case("OPEN"))
            .take(scan)
        {
            let owner_c = owner.clone();
            let name_c = name.clone();
            let number = pr.number;
            let summary = tokio::task::spawn_blocking(move || {
                gh_pr_thread_addressing_remote(&owner_c, &name_c, number).unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            if summary.all_addressed {
                let mut ranked = score_pr(pr);
                ranked.reasons.push(format!(
                    "threads addressed: {} resolved, {} outdated, {} open",
                    summary.resolved, summary.outdated, summary.open
                ));
                out.push(json!({
                    "ranked": ranked,
                    "threads": summary,
                }));
            }
            if out.len() >= limit {
                break;
            }
        }

        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "scanned": scan.min(prs.len()),
            "count": out.len(),
            "prs": out,
        }))
    }

    #[tool(
        description = "Priority PRs across ALL configured Easy Review projects (cross-repo review queue)."
    )]
    async fn cross_repo_queue(
        &self,
        Parameters(args): Parameters<CrossRepoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = clamp_limit(args.limit, 10);
        let file = projects::load_projects();
        let mut tagged: Vec<TaggedRankedPr<'_>> = Vec::new();
        // Collect owned strings first so we can borrow project names.
        let mut batches: Vec<(String, String, String, Vec<QueuePr>)> = Vec::new();

        for project in &file.projects {
            let Some(remote) = project.remote.as_deref() else {
                continue;
            };
            let Ok((owner, name)) = projects::parse_repo_slug(remote) else {
                continue;
            };
            let prs = tokio::task::spawn_blocking({
                let owner = owner.clone();
                let name = name.clone();
                move || gh_pr_list_queue(&owner, &name, "open", 50)
            })
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
            batches.push((project.name.clone(), owner, name, prs));
        }

        if args.production_lines.unwrap_or(false) {
            for (_, owner, name, prs) in &mut batches {
                let window = prs.len().min(10);
                enrich_production_lines(owner, name, &mut prs[..window], window).await;
            }
        }

        for (project_name, owner, name, prs) in &batches {
            for pr in prs {
                tagged.push(TaggedRankedPr {
                    project: Some(project_name.as_str()),
                    repo: format!("{owner}/{name}"),
                    ranked: score_pr(pr),
                });
            }
        }
        tagged.sort_by(|a, b| {
            b.ranked
                .priority_score
                .cmp(&a.ranked.priority_score)
                .then_with(|| a.ranked.total_lines.cmp(&b.ranked.total_lines))
        });
        tagged.truncate(limit);

        text_json(&json!({
            "projects_scanned": batches.len(),
            "limit": limit,
            "prs": tagged,
        }))
    }

    #[tool(
        name = "open_in_easy_review",
        description = "How to open a PR in Easy Review (GitHub URL + desktop/TUI instructions). No OS deep-link yet."
    )]
    async fn open_in_easy_review_tool(
        &self,
        Parameters(args): Parameters<PrNumberArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (owner, name, project_name) =
            resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
                .map_err(|e| tool_err(e.to_string()))?;
        let hint = open_in_easy_review(&owner, &name, args.number);
        text_json(&json!({
            "project": project_name,
            "open": hint,
        }))
    }

    #[tool(
        description = "Summarize managed Easy Review triage.json / review.json / tour.json sidecars for a PR (local app data), if present."
    )]
    async fn summarize_triage(
        &self,
        Parameters(args): Parameters<PrNumberArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (owner, name, project_name) =
            resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
                .map_err(|e| tool_err(e.to_string()))?;
        let number = args.number;
        let summary =
            tokio::task::spawn_blocking(move || summarize_pr_sidecars(&owner, &name, number))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        text_json(&json!({
            "project": project_name,
            "summary": summary,
        }))
    }

    #[tool(
        description = "Top production-code files by churn in a PR (excludes test/storybook/generated/docs)."
    )]
    async fn diff_hotspots(
        &self,
        Parameters(args): Parameters<HotspotsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (owner, name, project_name) =
            resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
                .map_err(|e| tool_err(e.to_string()))?;
        let number = args.number;
        let limit = clamp_limit(args.limit, 10);
        let stats: ProdDiffStats = tokio::task::spawn_blocking({
            let owner = owner.clone();
            let name = name.clone();
            move || gh_pr_prod_diff_stats(&owner, &name, number)
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;
        let hotspots = stats.production_hotspots(limit);
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "number": number,
            "production_lines": stats.production.lines_changed(),
            "hotspots": hotspots,
        }))
    }

    #[tool(
        description = "Compare production-only changed lines for a list of PR numbers and rank smallest → largest."
    )]
    async fn compare_prod_size(
        &self,
        Parameters(args): Parameters<CompareProdArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.numbers.is_empty() {
            return Err(tool_err("numbers must be a non-empty array of PR numbers"));
        }
        if args.numbers.len() > 25 {
            return Err(tool_err("compare at most 25 PRs at a time"));
        }
        let (owner, name, project_name) =
            resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
                .map_err(|e| tool_err(e.to_string()))?;

        let mut rows = Vec::new();
        for number in args.numbers {
            let owner_c = owner.clone();
            let name_c = name.clone();
            let stats = tokio::task::spawn_blocking(move || {
                gh_pr_prod_diff_stats(&owner_c, &name_c, number)
            })
            .await
            .ok()
            .and_then(|r| r.ok());
            let Some(stats) = stats else {
                rows.push(json!({
                    "number": number,
                    "error": "failed to fetch diff",
                }));
                continue;
            };
            rows.push(json!({
                "number": number,
                "production_lines": stats.production.lines_changed(),
                "total_lines": stats.total.lines_changed(),
                "production_files": stats.production.files,
                "test_lines": stats.test.lines_changed(),
                "generated_lines": stats.generated.lines_changed(),
            }));
        }
        rows.sort_by(|a, b| {
            let la = a
                .get("production_lines")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::MAX);
            let lb = b
                .get("production_lines")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::MAX);
            la.cmp(&lb)
        });

        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project": project_name,
            "prs": rows,
        }))
    }

    #[tool(
        description = "PREFERRED: prepare a PR review kit (writes shared diff-tmp, returns diff_hash + prompts). You (the MCP client agent) do the review yourself, then call upload_artifacts. Does NOT spawn agent CLIs or use the agent slot pool."
    )]
    async fn prepare_review(
        &self,
        Parameters(args): Parameters<PrepareReviewArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (owner, name, project_name) =
            resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
                .map_err(|e| tool_err(e.to_string()))?;
        let number = args.number;
        let kinds = parse_kinds(args.kinds).map_err(tool_err)?;

        let kit = tokio::task::spawn_blocking(move || {
            prepare_review_kit(&owner, &name, number, &kinds, &[])
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;

        text_json(&json!({
            "project": project_name,
            "kit": kit,
        }))
    }

    #[tool(
        description = "PREFERRED: upload triage/review/tour sidecar files you produced into shared Easy Review storage. Validates JSON shape + diff_hash against prepare_review's diff-tmp. No agent spawn / no slot pool."
    )]
    async fn upload_artifacts(
        &self,
        Parameters(args): Parameters<UploadArtifactsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (owner, name, project_name) =
            resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
                .map_err(|e| tool_err(e.to_string()))?;
        let kind = parse_kind(&args.kind).map_err(tool_err)?;
        let number = args.number;
        let files = args.files;
        let refresh_diff = args.refresh_diff.unwrap_or(false);

        let result = tokio::task::spawn_blocking(move || {
            upload_pr_artifacts(UploadArtifactsRequest {
                owner,
                repo: name,
                pr: number,
                kind,
                files,
                refresh_diff,
            })
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;

        text_json(&json!({
            "project": project_name,
            "uploaded": result,
            "note": "Sidecars are in shared managed storage — open the PR in Desktop/TUI or call summarize_triage.",
        }))
    }

    #[tool(
        description = "OPTIONAL/legacy: spawn a local agent CLI for triage (uses agent slot pool — prefer prepare_review + upload_artifacts)."
    )]
    async fn run_triage(
        &self,
        Parameters(args): Parameters<RunReviewArgs>,
    ) -> Result<CallToolResult, McpError> {
        spawn_review_job(HeadlessJobKind::Triage, args).await
    }

    #[tool(
        description = "OPTIONAL/legacy: spawn a local agent CLI for general review (uses agent slot pool — prefer prepare_review + upload_artifacts)."
    )]
    async fn run_review(
        &self,
        Parameters(args): Parameters<RunReviewArgs>,
    ) -> Result<CallToolResult, McpError> {
        spawn_review_job(HeadlessJobKind::Review, args).await
    }

    #[tool(
        description = "OPTIONAL/legacy: spawn a local agent CLI for guided tour (uses agent slot pool — prefer prepare_review + upload_artifacts)."
    )]
    async fn run_tour(
        &self,
        Parameters(args): Parameters<RunReviewArgs>,
    ) -> Result<CallToolResult, McpError> {
        spawn_review_job(HeadlessJobKind::Tour, args).await
    }

    #[tool(
        description = "OPTIONAL/legacy: spawn triage+review+tour agent CLIs (uses agent slot pool — prefer prepare_review with all kinds, then upload_artifacts per kind)."
    )]
    async fn run_ai_suite(
        &self,
        Parameters(args): Parameters<RunReviewArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (owner, name, project_name) =
            resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
                .map_err(|e| tool_err(e.to_string()))?;
        let number = args.number;
        let provider_id = args.provider_id;
        let model_id = args.model_id;

        let jobs = tokio::task::spawn_blocking(move || {
            let kinds = [
                HeadlessJobKind::Triage,
                HeadlessJobKind::Review,
                HeadlessJobKind::Tour,
            ];
            let mut out = Vec::with_capacity(kinds.len());
            for kind in kinds {
                let info = start_job(HeadlessJobRequest {
                    kind,
                    owner: owner.clone(),
                    repo: name.clone(),
                    pr: number,
                    base_ref: None,
                    head_ref: None,
                    ignore_globs: vec![],
                    provider_id: provider_id.clone(),
                    model_id: model_id.clone(),
                    dry_run: false,
                })?;
                out.push(info);
            }
            Ok::<_, anyhow::Error>(out)
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;

        text_json(&json!({
            "project": project_name,
            "note": "Started triage + review + tour agent jobs (slot pool). Prefer prepare_review + upload_artifacts to avoid pool contention.",
            "jobs": jobs,
        }))
    }

    #[tool(description = "List headless review jobs started by this MCP process (run_* only).")]
    async fn list_review_jobs(&self) -> Result<CallToolResult, McpError> {
        text_json(&json!({ "jobs": list_jobs() }))
    }

    #[tool(description = "Get status for a headless review job from run_* (queued/running/done/failed).")]
    async fn review_job_status(
        &self,
        Parameters(args): Parameters<JobIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let info =
            job_status(&args.id).ok_or_else(|| tool_err(format!("unknown job id: {}", args.id)))?;
        text_json(&json!({ "job": info }))
    }

    #[tool(
        description = "Cancel a queued headless review job from run_* (running jobs cannot be cancelled yet)."
    )]
    async fn cancel_review_job(
        &self,
        Parameters(args): Parameters<JobIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let info = cancel_job(&args.id).map_err(|e| tool_err(e.to_string()))?;
        text_json(&json!({ "job": info }))
    }

    #[tool(description = "Catalog of Easy Review MCP tools (shipped + future ideas).")]
    async fn tool_ideas(&self) -> Result<CallToolResult, McpError> {
        text_json(&json!({
            "shipped": SHIPPED_TOOLS,
            "ideas": FUTURE_IDEAS,
        }))
    }
}

async fn spawn_review_job(
    kind: HeadlessJobKind,
    args: RunReviewArgs,
) -> Result<CallToolResult, McpError> {
    let (owner, name, project_name) =
        resolve_repo(args.repo.as_deref(), args.project_id.as_deref())
            .map_err(|e| tool_err(e.to_string()))?;
    let number = args.number;
    let provider_id = args.provider_id;
    let model_id = args.model_id;

    let info = tokio::task::spawn_blocking(move || {
        start_job(HeadlessJobRequest {
            kind,
            owner: owner.clone(),
            repo: name.clone(),
            pr: number,
            base_ref: None,
            head_ref: None,
            ignore_globs: vec![],
            provider_id,
            model_id,
            dry_run: false,
        })
    })
    .await
    .map_err(|e| McpError::internal_error(e.to_string(), None))?
    .map_err(|e| tool_err(e.to_string()))?;

    text_json(&json!({
        "project": project_name,
        "note": "Spawned a local agent CLI into the shared slot pool. Prefer prepare_review + upload_artifacts to avoid pool contention.",
        "job": info,
    }))
}

fn parse_kind(s: &str) -> Result<HeadlessJobKind, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "triage" => Ok(HeadlessJobKind::Triage),
        "review" => Ok(HeadlessJobKind::Review),
        "tour" => Ok(HeadlessJobKind::Tour),
        other => Err(format!(
            "unknown kind '{other}'; expected triage|review|tour"
        )),
    }
}

fn parse_kinds(kinds: Option<Vec<String>>) -> Result<Vec<HeadlessJobKind>, String> {
    match kinds {
        None => Ok(vec![
            HeadlessJobKind::Triage,
            HeadlessJobKind::Review,
            HeadlessJobKind::Tour,
        ]),
        Some(list) if list.is_empty() => Err("kinds must not be empty".into()),
        Some(list) => list.iter().map(|s| parse_kind(s)).collect(),
    }
}

fn parse_status(s: &str) -> Result<ReviewStatus, McpError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "ready_to_review" | "ready" => Ok(ReviewStatus::ReadyToReview),
        "draft" => Ok(ReviewStatus::Draft),
        "outdated" | "behind" => Ok(ReviewStatus::Outdated),
        "blocked_conflicts" | "conflicts" => Ok(ReviewStatus::BlockedConflicts),
        "waiting_on_author" | "changes_requested" => Ok(ReviewStatus::WaitingOnAuthor),
        "approved" => Ok(ReviewStatus::Approved),
        "merge_ready" | "ready_to_merge" => Ok(ReviewStatus::MergeReady),
        "inactive" | "closed" | "merged" => Ok(ReviewStatus::Inactive),
        other => Err(tool_err(format!(
            "unknown status '{other}'; expected ready_to_review|draft|outdated|blocked_conflicts|waiting_on_author|approved|merge_ready (for CI/conflicts combo use prs_blocked)"
        ))),
    }
}

const SHIPPED_TOOLS: &[&str] = &[
    "list_projects",
    "list_prs",
    "priority_prs",
    "low_hanging_fruit",
    "pr_diff_stats",
    "prs_by_status",
    "prs_blocked",
    "prs_failing_ci",
    "my_review_debt",
    "prs_stale",
    "prs_already_addressed",
    "cross_repo_queue",
    "open_in_easy_review",
    "summarize_triage",
    "diff_hotspots",
    "compare_prod_size",
    "prepare_review",
    "upload_artifacts",
    "run_triage",
    "run_review",
    "run_tour",
    "run_ai_suite",
    "list_review_jobs",
    "review_job_status",
    "cancel_review_job",
    "tool_ideas",
];

const FUTURE_IDEAS: &[&str] = &[
    "prepare/upload for expert / professor / arena artifacts",
    "er:// deep-link / single-instance desktop open from MCP",
    "cancel mid-run for legacy run_* jobs (kill agent PID)",
    "required-checks-only CI filter (GitHub branch protection)",
    "inbox_digest / export_review_brief / missing_tests",
];

#[tool_handler]
impl ServerHandler for ErMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("er-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Easy Review MCP for PR triage and client-owned AI reviews. \
             Queues: priority_prs, low_hanging_fruit, my_review_debt, cross_repo_queue. \
             Filters: prs_by_status, prs_stale, prs_blocked, prs_failing_ci, prs_already_addressed. \
             Sizing: pr_diff_stats, diff_hotspots, compare_prod_size. \
             Preferred AI path (no agent slot pool): prepare_review → you write the sidecars → upload_artifacts → summarize_triage. \
             Legacy spawn path (uses slot pool): run_triage / run_review / run_tour / run_ai_suite. \
             Open: open_in_easy_review.",
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
