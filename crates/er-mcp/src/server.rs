//! MCP tool surface for Easy Review — thin REST API over er-engine.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use er_engine::diagram_upload::{list_diagrams, prepare_diagram_kit, upload_diagram};
use er_engine::git::ProdDiffStats;
use er_engine::github::{
    gh_pr_checks_state_remote, gh_pr_list_queue, gh_pr_prod_diff_stats,
    gh_pr_thread_addressing_remote,
};
use er_engine::pr_review_feedback::{
    get_pr_review_feedback, reply_to_pr_finding, reply_to_pr_note, reply_to_pr_question,
};
use er_engine::projects_pins::{self, PinnedPr};
use er_engine::review_queue::{
    filter_blocked, filter_by_status, filter_failing_ci, filter_review_debt, filter_stale,
    rank_low_hanging, rank_priority, score_pr, QueuePr, RankedPr, ReviewStatus,
};
use er_engine::sidecar_specs::artifact_specs_for_dir;
use er_engine::sidecar_summary::{list_repo_pr_artifacts, present_kinds, summarize_pr_sidecars};
use er_engine::sidecar_upload::{
    prepare_review_kit, upload_pr_artifacts, SidecarKind, UploadArtifactsRequest,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::projects::{self, PrTargetInput, ResolvedPr};

#[derive(Clone)]
pub struct ErMcp {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PrRefFields {
    /// Universal ref: PR URL, worktree path, `owner/repo`, `owner/repo#N`, branch name, or bare PR number.
    #[serde(default)]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub number: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrResolveArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrsQueryArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    #[serde(default)]
    pub cross_repo: Option<bool>,
    /// `priority` | `smallest` | `updated` (default `priority`).
    #[serde(default)]
    pub sort: Option<String>,
    /// `review_debt` | `stale` | `blocked` | `failing_ci` | `ready` | `outdated` |
    /// `waiting_on_author` | `addressed` | `draft` | `approved` | `merge_ready`.
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub stale_days: Option<u32>,
    #[serde(default)]
    pub production_lines: Option<bool>,
    #[serde(default)]
    pub include_drafts: Option<bool>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub scan_limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrStatsArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    /// Compare multiple PR numbers in one repo (max 12). When set, `target.number` is ignored.
    #[serde(default)]
    pub numbers: Option<Vec<u64>>,
    #[serde(default)]
    pub include_files: Option<bool>,
    #[serde(default)]
    pub include_hotspots: Option<bool>,
    #[serde(default)]
    pub hotspot_limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrPrepareArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrUploadArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    pub kind: String,
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub refresh_diff: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrGuideArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    /// `prepare` (default) fetches diff + tour spec; `upload` writes tour.json.
    #[serde(default)]
    pub action: Option<String>,
    /// Required for `upload`: `{ "tour.json": "..." }`.
    #[serde(default)]
    pub files: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub refresh_diff: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrSummarizeArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrDiagramArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    /// `list` (default) fetches existing diagrams; `prepare` fetches diff + prompt for
    /// one kind; `upload` writes the diagram JSON.
    #[serde(default)]
    pub action: Option<String>,
    /// Required for `prepare`/`upload`: `mental-model` | `subsystems` | `flows` | `custom`.
    #[serde(default)]
    pub kind: Option<String>,
    /// For `kind=custom`: the diagram instructions (required on `prepare`; pinned onto
    /// the sidecar on `upload`, overriding whatever `prompt` the uploaded JSON carries).
    #[serde(default)]
    pub prompt: Option<String>,
    /// Required for `upload`: exactly one entry, `{ "<output_file>": "<diagram JSON>" }`
    /// using the filename from `action=prepare`'s `kit.output_file`.
    #[serde(default)]
    pub files: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub refresh_diff: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrFeedbackGetArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    #[serde(default)]
    pub include_resolved: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrFeedbackReplyArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    /// `question` | `note` | `finding`.
    pub r#type: String,
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrSavedArgs {
    #[serde(flatten)]
    pub target: PrRefFields,
    /// `pin` | `unpin` | `list`.
    pub action: String,
    /// For `list`: `pinned` | `artifacts` | `all` (default `all`).
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

fn clamp_limit(limit: Option<u32>, default: u32, max: u32) -> usize {
    limit.unwrap_or(default).clamp(1, max) as usize
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

fn target_input(target: &PrRefFields) -> PrTargetInput<'_> {
    PrTargetInput {
        ref_str: target.r#ref.as_deref(),
        pr_url: target.pr_url.as_deref(),
        repo: target.repo.as_deref(),
        project_id: target.project_id.as_deref(),
        number: target.number,
    }
}

fn resolve_target(target: &PrRefFields) -> Result<ResolvedPr, McpError> {
    projects::resolve_pr_target(&target_input(target)).map_err(|e| tool_err(e.to_string()))
}

/// Repo slug for queue/list tools: explicit `repo`, or `ref` when it names a repo (`owner/repo`).
fn query_repo_args(target: &PrRefFields) -> (Option<&str>, Option<&str>) {
    if target.repo.as_deref().is_some_and(|s| !s.trim().is_empty()) {
        return (target.repo.as_deref(), target.project_id.as_deref());
    }
    let Some(raw) = target
        .r#ref
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return (None, target.project_id.as_deref());
    };
    if let Some((left, _)) = raw.split_once('#').or_else(|| raw.split_once('!')) {
        return (Some(left), target.project_id.as_deref());
    }
    if er_engine::github::parse_github_pr_url(raw).is_some() {
        return (None, target.project_id.as_deref());
    }
    if raw.contains('/') && !raw.starts_with('/') && !raw.starts_with('~') {
        return (Some(raw), target.project_id.as_deref());
    }
    (None, target.project_id.as_deref())
}

fn fetch_pr_title(owner: &str, repo: &str, number: u64) -> Option<String> {
    er_engine::github::gh_pr_title(owner, repo, number)
}

fn pinned_sidecar_json(
    entry: &PinnedPr,
    summary: &er_engine::sidecar_summary::PrSidecarSummary,
) -> serde_json::Value {
    json!({
        "number": entry.number,
        "title": entry.title,
        "saved_at_ms": entry.saved_at_ms,
        "kinds": present_kinds(summary),
        "bucket_path": summary.bucket_path,
        "triage": summary.triage,
        "review": summary.review,
        "tour": summary.tour,
        "missing": summary.missing,
    })
}

async fn load_queue(
    repo: Option<&str>,
    project_id: Option<&str>,
) -> Result<(String, String, Option<String>, Vec<QueuePr>), McpError> {
    let (owner, name, project_name) =
        projects::resolve_repo(repo, project_id).map_err(|e| tool_err(e.to_string()))?;
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
    let jobs: Vec<_> = prs
        .iter()
        .take(max_enrich)
        .map(|pr| {
            let owner = owner.to_string();
            let repo = repo.to_string();
            let number = pr.number;
            (
                number,
                tokio::task::spawn_blocking(move || gh_pr_prod_diff_stats(&owner, &repo, number)),
            )
        })
        .collect();
    let mut by_number = std::collections::HashMap::new();
    for (number, handle) in jobs {
        if let Ok(Ok(stats)) = handle.await {
            by_number.insert(number, stats.production.lines_changed() as u64);
        }
    }
    for pr in prs.iter_mut().take(max_enrich) {
        if let Some(lines) = by_number.get(&pr.number) {
            pr.production_lines = Some(*lines);
        }
    }
}

async fn enrich_ci(owner: &str, repo: &str, prs: &mut [QueuePr], max_enrich: usize) {
    let jobs: Vec<_> = prs
        .iter()
        .take(max_enrich)
        .map(|pr| {
            let owner = owner.to_string();
            let repo = repo.to_string();
            let number = pr.number;
            (
                number,
                tokio::task::spawn_blocking(move || {
                    gh_pr_checks_state_remote(&owner, &repo, number).unwrap_or("unknown")
                }),
            )
        })
        .collect();
    let mut by_number = std::collections::HashMap::new();
    for (number, handle) in jobs {
        if let Ok(state) = handle.await {
            by_number.insert(number, state);
        }
    }
    for pr in prs.iter_mut().take(max_enrich) {
        if let Some(state) = by_number.get(&pr.number) {
            pr.checks_state = Some((*state).to_string());
        }
    }
}

fn parse_status_filter(filter: &str) -> Result<ReviewStatus, McpError> {
    match filter.trim().to_ascii_lowercase().as_str() {
        "ready" | "ready_to_review" => Ok(ReviewStatus::ReadyToReview),
        "draft" => Ok(ReviewStatus::Draft),
        "outdated" | "behind" => Ok(ReviewStatus::Outdated),
        "blocked_conflicts" | "conflicts" => Ok(ReviewStatus::BlockedConflicts),
        "waiting_on_author" | "changes_requested" => Ok(ReviewStatus::WaitingOnAuthor),
        "approved" => Ok(ReviewStatus::Approved),
        "merge_ready" | "ready_to_merge" => Ok(ReviewStatus::MergeReady),
        "inactive" | "closed" | "merged" => Ok(ReviewStatus::Inactive),
        other => Err(tool_err(format!(
            "unknown filter status '{other}'; use review_debt|stale|blocked|failing_ci|ready|outdated|waiting_on_author|addressed|draft|approved|merge_ready"
        ))),
    }
}

fn parse_kind(s: &str) -> Result<SidecarKind, McpError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "triage" => Ok(SidecarKind::Triage),
        "review" => Ok(SidecarKind::Review),
        "tour" => Ok(SidecarKind::Tour),
        other => Err(tool_err(format!(
            "unknown kind '{other}'; expected triage|review|tour"
        ))),
    }
}

fn parse_prepare_kinds(kinds: Option<Vec<String>>) -> Result<Vec<SidecarKind>, McpError> {
    let parsed: Vec<SidecarKind> = match kinds {
        None => vec![SidecarKind::Triage],
        Some(list) if list.is_empty() => return Err(tool_err("kinds must not be empty")),
        Some(list) => list
            .iter()
            .map(|s| parse_kind(s))
            .collect::<Result<_, _>>()?,
    };
    if parsed.contains(&SidecarKind::Tour) {
        return Err(tool_err(
            "tour uses pr_guide — omit tour from pr_prepare kinds",
        ));
    }
    Ok(parsed)
}

async fn prepare_kit_json(
    pr: &ResolvedPr,
    kinds: &[SidecarKind],
    note: &str,
) -> Result<serde_json::Value, McpError> {
    let kinds_vec = kinds.to_vec();
    let kinds_for_specs = kinds.to_vec();
    let owner = pr.owner.clone();
    let name = pr.repo.clone();
    let number = pr.number;

    let kit = tokio::task::spawn_blocking(move || {
        prepare_review_kit(&owner, &name, number, &kinds_vec, &[])
    })
    .await
    .map_err(|e| McpError::internal_error(e.to_string(), None))?
    .map_err(|e| tool_err(e.to_string()))?;

    let specs = artifact_specs_for_dir(&kinds_for_specs, &kit.er_dir, &kit.base_ref, &kit.head_ref);
    let mut kit = kit;
    for artifact in &mut kit.artifacts {
        artifact.prompt.clear();
    }

    Ok(json!({
        "project": pr.project_name,
        "repo": format!("{}/{}", pr.owner, pr.repo),
        "number": pr.number,
        "bucket_path": pr.bucket_path,
        "pr_url": pr.pr_url,
        "kit": kit,
        "artifact_specs": specs,
        "note": note,
    }))
}

#[derive(Serialize)]
struct TaggedRankedPr<'a> {
    project: Option<&'a str>,
    repo: String,
    #[serde(flatten)]
    ranked: RankedPr,
}

async fn query_single_repo(args: &PrsQueryArgs) -> Result<serde_json::Value, McpError> {
    let limit = clamp_limit(args.limit, 10, 50);
    let scan = clamp_limit(args.scan_limit, 15, 20);
    let sort = args
        .sort
        .as_deref()
        .unwrap_or("priority")
        .to_ascii_lowercase();
    let filter = args.filter.as_deref().map(|s| s.to_ascii_lowercase());

    let (repo, project_id) = query_repo_args(&args.target);
    let (owner, name, project_name, mut prs) = load_queue(repo, project_id).await?;

    let needs_ci = matches!(filter.as_deref(), Some("blocked") | Some("failing_ci"));
    if needs_ci {
        let n = prs.len().min(scan);
        enrich_ci(&owner, &name, &mut prs[..n], scan).await;
    }

    if args.production_lines.unwrap_or(false)
        || sort == "smallest"
        || filter.as_deref() == Some("blocked")
    {
        let window = prs.len().min(if sort == "smallest" {
            25
        } else {
            scan.max(limit)
        });
        enrich_production_lines(&owner, &name, &mut prs[..window], window).await;
    }

    let ranked: Vec<RankedPr> = match filter.as_deref() {
        Some("review_debt") => filter_review_debt(&prs, limit),
        Some("stale") => {
            let days = args.stale_days.unwrap_or(14).clamp(1, 365) as u64;
            filter_stale(&prs, days, now_epoch_secs(), limit)
        }
        Some("blocked") => filter_blocked(&prs, limit),
        Some("failing_ci") => filter_failing_ci(&prs, limit),
        Some("addressed") => filter_addressed(&owner, &name, &prs, scan, limit).await?,
        Some(f) => {
            let status = parse_status_filter(f)?;
            let mut rows = filter_by_status(&prs, status);
            rows.truncate(limit);
            rows
        }
        None => match sort.as_str() {
            "smallest" => rank_low_hanging(&prs, limit, args.include_drafts.unwrap_or(false)),
            "updated" => {
                let mut scored: Vec<_> = prs.iter().map(score_pr).collect();
                scored.sort_by(|a, b| b.pr.updated_at.cmp(&a.pr.updated_at));
                scored.truncate(limit);
                scored
            }
            _ => rank_priority(&prs, limit),
        },
    };

    Ok(json!({
        "repo": format!("{owner}/{name}"),
        "project": project_name,
        "sort": sort,
        "filter": filter,
        "count": ranked.len(),
        "prs": ranked,
    }))
}

async fn filter_addressed(
    owner: &str,
    name: &str,
    prs: &[QueuePr],
    scan: usize,
    limit: usize,
) -> Result<Vec<RankedPr>, McpError> {
    let mut out = Vec::new();
    for pr in prs
        .iter()
        .filter(|p| p.state.eq_ignore_ascii_case("OPEN"))
        .take(scan)
    {
        let owner_c = owner.to_string();
        let name_c = name.to_string();
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
            out.push(ranked);
        }
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

async fn query_cross_repo(args: &PrsQueryArgs) -> Result<serde_json::Value, McpError> {
    let limit = clamp_limit(args.limit, 10, 50);
    let file = projects::load_projects();
    let mut tagged: Vec<TaggedRankedPr<'_>> = Vec::new();
    let mut batches: Vec<(String, String, String, Vec<QueuePr>)> = Vec::new();

    let mut list_jobs = Vec::new();
    for project in &file.projects {
        let Some(remote) = project.remote.as_deref() else {
            continue;
        };
        let Ok((owner, name)) = projects::parse_repo_slug(remote) else {
            continue;
        };
        let project_name = project.name.clone();
        list_jobs.push((
            project_name,
            owner.clone(),
            name.clone(),
            tokio::task::spawn_blocking(move || gh_pr_list_queue(&owner, &name, "open", 50)),
        ));
    }
    for (project_name, owner, name, handle) in list_jobs {
        let prs = handle.await.ok().and_then(|r| r.ok()).unwrap_or_default();
        batches.push((project_name, owner, name, prs));
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

    Ok(json!({
        "projects_scanned": batches.len(),
        "limit": limit,
        "prs": tagged,
    }))
}

#[tool_router]
impl ErMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List Easy Review projects from ~/.config/er/projects.json.")]
    async fn projects_list(&self) -> Result<CallToolResult, McpError> {
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
        description = "Resolve a PR ref (URL, worktree path, owner/repo, owner/repo#N, branch, or number) to owner/repo/number + bucket_path."
    )]
    async fn pr_resolve(
        &self,
        Parameters(args): Parameters<PrResolveArgs>,
    ) -> Result<CallToolResult, McpError> {
        let pr = resolve_target(&args.target)?;
        text_json(&pr)
    }

    #[tool(
        description = "Query open PRs: sort (priority|smallest|updated), filter (review_debt|stale|blocked|failing_ci|ready|addressed|…), optional cross_repo."
    )]
    async fn prs_query(
        &self,
        Parameters(args): Parameters<PrsQueryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let body = if args.cross_repo.unwrap_or(false) {
            query_cross_repo(&args).await?
        } else {
            query_single_repo(&args).await?
        };
        text_json(&body)
    }

    #[tool(
        description = "PR diff stats (production vs test/docs). Single PR via ref, or batch via numbers=[…]. Optional hotspots."
    )]
    async fn pr_stats(
        &self,
        Parameters(args): Parameters<PrStatsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let hotspot_limit = clamp_limit(args.hotspot_limit, 10, 50);

        if let Some(numbers) = args.numbers.filter(|n| !n.is_empty()) {
            if numbers.len() > 12 {
                return Err(tool_err("compare at most 12 PRs at a time"));
            }
            let (repo, project_id) = query_repo_args(&args.target);
            let (owner, name, project_name) =
                projects::resolve_repo(repo, project_id).map_err(|e| tool_err(e.to_string()))?;
            let mut rows = Vec::new();
            for number in numbers {
                let owner_c = owner.clone();
                let name_c = name.clone();
                let stats = tokio::task::spawn_blocking(move || {
                    gh_pr_prod_diff_stats(&owner_c, &name_c, number)
                })
                .await
                .ok()
                .and_then(|r| r.ok());
                rows.push(match stats {
                    Some(stats) => json!({
                        "number": number,
                        "production_lines": stats.production.lines_changed(),
                        "total_lines": stats.total.lines_changed(),
                        "production_files": stats.production.files,
                    }),
                    None => json!({ "number": number, "error": "failed to fetch diff" }),
                });
            }
            rows.sort_by_key(|row| {
                row.get("production_lines")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(u64::MAX)
            });
            return text_json(&json!({
                "repo": format!("{owner}/{name}"),
                "project": project_name,
                "prs": rows,
            }));
        }

        let pr = resolve_target(&args.target)?;
        let owner = pr.owner.clone();
        let name = pr.repo.clone();
        let number = pr.number;
        let include_files = args.include_files.unwrap_or(false);
        let include_hotspots = args.include_hotspots.unwrap_or(false);

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
        let mut body = json!({
            "repo": format!("{owner}/{name}"),
            "project": pr.project_name,
            "number": number,
            "bucket_path": pr.bucket_path,
            "stats": stats,
            "production_lines": stats.production.lines_changed(),
            "total_lines": stats.total.lines_changed(),
        });
        if include_hotspots {
            body["hotspots"] = json!(stats.production_hotspots(hotspot_limit));
        }
        text_json(&body)
    }

    #[tool(
        description = "Prepare a PR review kit (diff-tmp + diff_hash + artifact_specs). You review; then call pr_upload."
    )]
    async fn pr_prepare(
        &self,
        Parameters(args): Parameters<PrPrepareArgs>,
    ) -> Result<CallToolResult, McpError> {
        let pr = resolve_target(&args.target)?;
        let kinds = parse_prepare_kinds(args.kinds)?;
        let body = prepare_kit_json(
            &pr,
            &kinds,
            "Author files per artifact_specs; embed kit.diff_hash; then pr_upload.",
        )
        .await?;
        text_json(&body)
    }

    #[tool(
        description = "Create a guided tour (tour.json) for a PR. action=prepare fetches diff + tour schema; action=upload writes tour.json."
    )]
    async fn pr_guide(
        &self,
        Parameters(args): Parameters<PrGuideArgs>,
    ) -> Result<CallToolResult, McpError> {
        let action = args
            .action
            .as_deref()
            .unwrap_or("prepare")
            .to_ascii_lowercase();
        let pr = resolve_target(&args.target)?;

        match action.as_str() {
            "prepare" => {
                let body = prepare_kit_json(
                    &pr,
                    &[SidecarKind::Tour],
                    "Read diff_tmp_path, author tour.json per artifact_specs (3–7 pillars), embed kit.diff_hash, then pr_guide action=upload.",
                )
                .await?;
                text_json(&body)
            }
            "upload" => {
                let files = args
                    .files
                    .filter(|f| !f.is_empty())
                    .ok_or_else(|| tool_err("upload requires files: { \"tour.json\": \"...\" }"))?;
                let owner = pr.owner.clone();
                let name = pr.repo.clone();
                let number = pr.number;
                let refresh_diff = args.refresh_diff.unwrap_or(false);

                let result = tokio::task::spawn_blocking(move || {
                    upload_pr_artifacts(UploadArtifactsRequest {
                        owner,
                        repo: name,
                        pr: number,
                        kind: SidecarKind::Tour,
                        files,
                        refresh_diff,
                    })
                })
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
                .map_err(|e| tool_err(e.to_string()))?;

                text_json(&json!({
                    "project": pr.project_name,
                    "repo": format!("{}/{}", pr.owner, pr.repo),
                    "number": pr.number,
                    "bucket_path": pr.bucket_path,
                    "uploaded": result,
                    "note": "Open the PR in Desktop/TUI Guide tab, or pr_summarize.",
                }))
            }
            _ => Err(tool_err("action must be prepare or upload")),
        }
    }

    #[tool(
        description = "Upload triage or review sidecars into shared Easy Review storage. Tours use pr_guide."
    )]
    async fn pr_upload(
        &self,
        Parameters(args): Parameters<PrUploadArgs>,
    ) -> Result<CallToolResult, McpError> {
        let pr = resolve_target(&args.target)?;
        let kind = parse_kind(&args.kind)?;
        if kind == SidecarKind::Tour {
            return Err(tool_err(
                "tour uploads use pr_guide with action=upload — not pr_upload",
            ));
        }
        let owner = pr.owner.clone();
        let name = pr.repo.clone();
        let number = pr.number;
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
            "project": pr.project_name,
            "repo": format!("{}/{}", pr.owner, pr.repo),
            "number": pr.number,
            "bucket_path": pr.bucket_path,
            "uploaded": result,
        }))
    }

    #[tool(description = "Summarize managed triage/review/tour sidecars for a PR.")]
    async fn pr_summarize(
        &self,
        Parameters(args): Parameters<PrSummarizeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let pr = resolve_target(&args.target)?;
        let owner = pr.owner.clone();
        let name = pr.repo.clone();
        let number = pr.number;
        let summary =
            tokio::task::spawn_blocking(move || summarize_pr_sidecars(&owner, &name, number))
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        text_json(&json!({
            "project": pr.project_name,
            "repo": format!("{}/{}", pr.owner, pr.repo),
            "number": pr.number,
            "bucket_path": pr.bucket_path,
            "summary": summary,
        }))
    }

    #[tool(
        description = "List, prepare, or upload Mermaid diagrams for a PR (kind: mental-model|subsystems|flows|custom). action=list (default) reads diagrams/*.json; action=prepare fetches diff + prompt for one kind; action=upload writes the diagram JSON."
    )]
    async fn pr_diagram(
        &self,
        Parameters(args): Parameters<PrDiagramArgs>,
    ) -> Result<CallToolResult, McpError> {
        let action = args
            .action
            .as_deref()
            .unwrap_or("list")
            .to_ascii_lowercase();
        let pr = resolve_target(&args.target)?;
        let owner = pr.owner.clone();
        let name = pr.repo.clone();
        let number = pr.number;

        match action.as_str() {
            "list" => {
                let (er_dir, diagrams) =
                    tokio::task::spawn_blocking(move || list_diagrams(&owner, &name, number))
                        .await
                        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                text_json(&json!({
                    "project": pr.project_name,
                    "repo": format!("{}/{}", pr.owner, pr.repo),
                    "number": pr.number,
                    "bucket_path": pr.bucket_path,
                    "er_dir": er_dir,
                    "diagrams": diagrams,
                }))
            }
            "prepare" => {
                let kind = args
                    .kind
                    .filter(|k| !k.trim().is_empty())
                    .ok_or_else(|| tool_err("prepare requires kind"))?;
                let prompt = args.prompt.clone();
                let kit = tokio::task::spawn_blocking(move || {
                    prepare_diagram_kit(&owner, &name, number, &kind, prompt.as_deref(), &[])
                })
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
                .map_err(|e| tool_err(e.to_string()))?;

                text_json(&json!({
                    "project": pr.project_name,
                    "repo": format!("{}/{}", pr.owner, pr.repo),
                    "number": pr.number,
                    "bucket_path": pr.bucket_path,
                    "kit": kit,
                }))
            }
            "upload" => {
                let kind = args
                    .kind
                    .filter(|k| !k.trim().is_empty())
                    .ok_or_else(|| tool_err("upload requires kind"))?;
                let files = args.files.filter(|f| !f.is_empty()).ok_or_else(|| {
                    tool_err("upload requires files: { \"<output_file>\": \"...\" }")
                })?;
                if files.len() != 1 {
                    return Err(tool_err(
                        "upload accepts exactly one file entry (the kit.output_file from prepare)",
                    ));
                }
                let (file_name, content) = files.into_iter().next().expect("checked len == 1");
                let custom_prompt = args.prompt.clone();
                let refresh_diff = args.refresh_diff.unwrap_or(false);

                let result = tokio::task::spawn_blocking(move || {
                    upload_diagram(
                        &owner,
                        &name,
                        number,
                        &kind,
                        &file_name,
                        &content,
                        custom_prompt.as_deref(),
                        refresh_diff,
                    )
                })
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
                .map_err(|e| tool_err(e.to_string()))?;

                text_json(&json!({
                    "project": pr.project_name,
                    "repo": format!("{}/{}", pr.owner, pr.repo),
                    "number": pr.number,
                    "bucket_path": pr.bucket_path,
                    "uploaded": result,
                }))
            }
            _ => Err(tool_err("action must be list, prepare, or upload")),
        }
    }

    #[tool(description = "Read review questions, notes, and AI findings for a PR.")]
    async fn pr_feedback_get(
        &self,
        Parameters(args): Parameters<PrFeedbackGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let pr = resolve_target(&args.target)?;
        let owner = pr.owner.clone();
        let name = pr.repo.clone();
        let number = pr.number;
        let include_resolved = args.include_resolved.unwrap_or(false);
        let feedback = tokio::task::spawn_blocking(move || {
            get_pr_review_feedback(&owner, &name, number, include_resolved)
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;
        text_json(&json!({
            "project": pr.project_name,
            "repo": format!("{}/{}", pr.owner, pr.repo),
            "number": pr.number,
            "bucket_path": pr.bucket_path,
            "feedback": feedback,
        }))
    }

    #[tool(description = "Reply to a question, note, or AI finding on a PR.")]
    async fn pr_feedback_reply(
        &self,
        Parameters(args): Parameters<PrFeedbackReplyArgs>,
    ) -> Result<CallToolResult, McpError> {
        let pr = resolve_target(&args.target)?;
        let owner = pr.owner.clone();
        let name = pr.repo.clone();
        let number = pr.number;
        let text = args.text;
        let author = args.author;

        let reply = match args.r#type.to_ascii_lowercase().as_str() {
            "question" => {
                let question_id = args.id.clone();
                tokio::task::spawn_blocking(move || {
                    reply_to_pr_question(
                        &owner,
                        &name,
                        number,
                        &question_id,
                        &text,
                        author.as_deref(),
                    )
                })
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
                .map_err(|e| tool_err(e.to_string()))?
            }
            "note" => {
                let note_id = args.id.clone();
                tokio::task::spawn_blocking(move || {
                    reply_to_pr_note(&owner, &name, number, &note_id, &text, author.as_deref())
                })
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
                .map_err(|e| tool_err(e.to_string()))?
            }
            "finding" => {
                let finding_id = args.id.clone();
                tokio::task::spawn_blocking(move || {
                    reply_to_pr_finding(&owner, &name, number, &finding_id, &text)
                })
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
                .map_err(|e| tool_err(e.to_string()))?
            }
            other => {
                return Err(tool_err(format!(
                    "type must be question, note, or finding (got '{other}')"
                )));
            }
        };

        text_json(&json!({
            "project": pr.project_name,
            "repo": format!("{}/{}", pr.owner, pr.repo),
            "number": pr.number,
            "bucket_path": pr.bucket_path,
            "reply": reply,
        }))
    }

    #[tool(description = "Pin, unpin, or list saved PRs and uploaded artifacts.")]
    async fn pr_saved(
        &self,
        Parameters(args): Parameters<PrSavedArgs>,
    ) -> Result<CallToolResult, McpError> {
        match args.action.to_ascii_lowercase().as_str() {
            "pin" => self.pr_saved_pin(args).await,
            "unpin" => self.pr_saved_unpin(args).await,
            "list" => self.pr_saved_list(args).await,
            _ => Err(tool_err("action must be pin, unpin, or list")),
        }
    }
}

impl ErMcp {
    async fn pr_saved_pin(&self, args: PrSavedArgs) -> Result<CallToolResult, McpError> {
        let pr = resolve_target(&args.target)?;
        let owner = pr.owner.clone();
        let name = pr.repo.clone();
        let number = pr.number;
        let title_arg = args.title.clone();
        let project_id_arg = args.target.project_id.clone();

        let result = tokio::task::spawn_blocking(move || {
            let (project_id, project_name) =
                projects_pins::resolve_project_for_pin(project_id_arg.as_deref(), &owner, &name)?;
            let title = title_arg
                .filter(|t| !t.trim().is_empty())
                .or_else(|| fetch_pr_title(&owner, &name, number))
                .unwrap_or_default();
            let pinned = projects_pins::pin_pr(&project_id, number, &title)?;
            let summary = summarize_pr_sidecars(&owner, &name, number);
            Ok::<_, anyhow::Error>((project_id, project_name, owner, name, pinned, summary))
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;

        let (project_id, project_name, owner, name, pinned, summary) = result;
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project_id": project_id,
            "project": project_name,
            "pinned": pinned_sidecar_json(&pinned, &summary),
        }))
    }

    async fn pr_saved_unpin(&self, args: PrSavedArgs) -> Result<CallToolResult, McpError> {
        let pr = resolve_target(&args.target)?;
        let owner = pr.owner.clone();
        let name = pr.repo.clone();
        let number = pr.number;
        let project_id_arg = args.target.project_id.clone();

        let result = tokio::task::spawn_blocking(move || {
            let Some((project_id, project_name)) =
                projects_pins::resolve_project_for_list(project_id_arg.as_deref(), &owner, &name)?
            else {
                return Ok((None, None, owner, name, false));
            };
            let removed = projects_pins::unpin_pr(&project_id, number)?;
            Ok::<_, anyhow::Error>((Some(project_id), Some(project_name), owner, name, removed))
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;

        let (project_id, project_name, owner, name, removed) = result;
        text_json(&json!({
            "repo": format!("{owner}/{name}"),
            "project_id": project_id,
            "project": project_name,
            "number": number,
            "removed": removed,
        }))
    }

    async fn pr_saved_list(&self, args: PrSavedArgs) -> Result<CallToolResult, McpError> {
        let source = args.source.as_deref().unwrap_or("all").to_ascii_lowercase();
        let (repo, project_id) = query_repo_args(&args.target);
        let (owner, name, project_name) =
            projects::resolve_repo(repo, project_id).map_err(|e| tool_err(e.to_string()))?;
        let limit = clamp_limit(args.limit, 50, 50);
        let project_id_arg = args.target.project_id.clone();
        let kinds = match args.kinds {
            None => None,
            Some(list) if list.is_empty() => None,
            Some(list) => Some(
                list.iter()
                    .map(|s| parse_kind(s))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        };

        let result = tokio::task::spawn_blocking(move || {
            let project =
                projects_pins::resolve_project_for_list(project_id_arg.as_deref(), &owner, &name)?;
            let (project_id, project_name, pinned_set, pinned_rows) = match project {
                Some((id, pname)) => {
                    let pinned = projects_pins::list_pinned(&id)?;
                    let set = projects_pins::pinned_numbers(&id);
                    let rows: Vec<_> = pinned
                        .iter()
                        .map(|entry| {
                            let summary = summarize_pr_sidecars(&owner, &name, entry.number);
                            pinned_sidecar_json(entry, &summary)
                        })
                        .collect();
                    (Some(id), Some(pname), set, rows)
                }
                None => (None, None, std::collections::HashSet::new(), Vec::new()),
            };

            let artifact_rows: Vec<_> = if source == "pinned" {
                Vec::new()
            } else {
                let filter = kinds.as_deref();
                list_repo_pr_artifacts(&owner, &name, filter, limit)
                    .into_iter()
                    .map(|entry| {
                        let number = entry.summary.number;
                        json!({
                            "number": number,
                            "kinds": entry.kinds,
                            "pinned": pinned_set.contains(&number),
                            "mtime_secs": entry.mtime_secs,
                            "bucket_path": entry.summary.bucket_path,
                            "triage": entry.summary.triage,
                            "review": entry.summary.review,
                            "tour": entry.summary.tour,
                            "missing": entry.summary.missing,
                        })
                    })
                    .collect()
            };

            Ok::<_, anyhow::Error>((
                project_id,
                project_name,
                owner,
                name,
                pinned_rows,
                artifact_rows,
                source,
            ))
        })
        .await
        .map_err(|e| McpError::internal_error(e.to_string(), None))?
        .map_err(|e| tool_err(e.to_string()))?;

        let (project_id, proj_name, owner, name, pinned_rows, artifact_rows, source) = result;
        let body = match source.as_str() {
            "pinned" => json!({
                "repo": format!("{owner}/{name}"),
                "project_id": project_id,
                "project": proj_name.or(project_name),
                "pinned": pinned_rows,
            }),
            "artifacts" => json!({
                "repo": format!("{owner}/{name}"),
                "project_id": project_id,
                "project": proj_name.or(project_name),
                "artifacts": artifact_rows,
            }),
            _ => json!({
                "repo": format!("{owner}/{name}"),
                "project_id": project_id,
                "project": proj_name.or(project_name),
                "pinned": pinned_rows,
                "artifacts": artifact_rows,
            }),
        };
        text_json(&body)
    }
}

#[tool_handler]
impl ServerHandler for ErMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(Implementation::new("er-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Easy Review MCP — REST-style PR API. Resolve refs with pr_resolve. \
                 Query queues with prs_query. Review: pr_prepare → pr_upload. \
                 Guided tour: pr_guide (prepare → upload tour.json). \
                 Diagrams: pr_diagram (list | prepare → upload diagram JSON). \
                 Feedback: pr_feedback_get / pr_feedback_reply. Saved: pr_saved. \
                 Skills: er-review, er-guide, er-queue, er-low-hanging-fruit, er-get-feedback, er-respond, er-saved.",
            )
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(ProtocolVersion::KNOWN_VERSIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_mcp_2026_07_28() {
        let server = ErMcp::new();
        assert_eq!(
            server.get_info().protocol_version,
            ProtocolVersion::V_2026_07_28
        );
        assert!(server.get_info().capabilities.tools.is_some());
    }

    #[test]
    fn registers_pr_diagram_tool() {
        let server = ErMcp::new();
        let tools = server.tool_router.list_all();
        let diagram_tool = tools
            .iter()
            .find(|t| t.name == "pr_diagram")
            .expect("pr_diagram tool registered");
        let schema = &diagram_tool.input_schema;
        let props = schema
            .get("properties")
            .and_then(|p| p.as_object())
            .expect("input schema has properties");
        for field in ["action", "kind", "prompt", "files", "refresh_diff"] {
            assert!(props.contains_key(field), "missing field: {field}");
        }
    }
}
