//! Prepare PR review kits and upload sidecar JSON for MCP / non-UI callers.
//!
//! Flow: [`prepare_review_kit`] writes shared `diff-tmp` + prompts; the client
//! agent produces artifacts; [`upload_pr_artifacts`] validates and stores them.
//! No agent CLI spawn — avoids competing for [`er_agent::agent_slots`].

use crate::ai::compute_diff_hash;
use crate::ai::prompts::{
    build_review_prompt_prepared_diff, build_tour_prompt_prepared_diff,
    build_triage_review_prompt_prepared_diff,
};
use crate::github::{gh_pr_diff_remote, gh_pr_metadata_remote, owner_repo_storage_slug};
use crate::storage::resolve_managed_root_for_pr_bucket;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Placeholder substituted for the managed PR bucket in standalone specs.
pub const OUTPUT_DIR_PLACEHOLDER: &str = "<OUTPUT_DIR>";

/// Which Easy Review sidecar set to prepare or upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarKind {
    Triage,
    Review,
    Tour,
}

impl SidecarKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Review => "review",
            Self::Tour => "tour",
        }
    }
}

/// One artifact kind ready for a client agent to produce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedArtifactSpec {
    pub kind: SidecarKind,
    /// Absolute paths the client must produce (then pass to upload).
    pub required_files: Vec<String>,
    /// Relative names inside the PR bucket.
    pub required_relative: Vec<String>,
    /// Full Easy Review prompt for this kind (same as Desktop prepared-diff).
    /// Cleared in `prepare_review` MCP responses when `artifact_specs` carries prompts.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prompt: String,
}

/// Bucket + diff + prompts for client-side review (no agent spawn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedReviewKit {
    pub owner: String,
    pub repo: String,
    pub pr: u64,
    pub base_ref: String,
    pub head_ref: String,
    pub er_dir: String,
    pub diff_tmp_path: String,
    /// SHA-256 hex of `diff-tmp` — embed this as `diff_hash` in every JSON sidecar.
    pub diff_hash: String,
    pub diff_bytes: u64,
    pub artifacts: Vec<PreparedArtifactSpec>,
    pub instructions: String,
    /// Absolute path of `review-md.md` in `er_dir`; set when the base branch
    /// has a `REVIEW.md`.
    #[serde(default)]
    pub review_md_path: Option<String>,
}

/// Files to write for one kind (`relative_name` → content).
#[derive(Debug, Clone)]
pub struct UploadArtifactsRequest {
    pub owner: String,
    pub repo: String,
    pub pr: u64,
    pub kind: SidecarKind,
    pub files: BTreeMap<String, String>,
    /// When true, re-fetch the PR diff before validating (default: reuse existing `diff-tmp`).
    pub refresh_diff: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadArtifactsResult {
    pub er_dir: String,
    pub kind: SidecarKind,
    pub written: Vec<String>,
    pub diff_hash: String,
}

fn relative_files(kind: SidecarKind) -> Vec<&'static str> {
    match kind {
        SidecarKind::Triage => vec!["triage.json"],
        SidecarKind::Tour => vec!["tour.json"],
        SidecarKind::Review => {
            vec!["review.json", "order.json", "checklist.json", "summary.md"]
        }
    }
}

fn build_prompt(kind: SidecarKind, er_dir: &str, base: &str, head: &str) -> String {
    use crate::ai::prompts::{
        build_review_prompt_prepared_diff, build_tour_prompt_prepared_diff,
        build_triage_review_prompt_prepared_diff,
    };
    match kind {
        SidecarKind::Triage => build_triage_review_prompt_prepared_diff(
            "branch",
            er_dir,
            &crate::ai::prepared_diff::diff_tmp_hash(er_dir)
                .unwrap_or_else(|| "<DIFF_HASH>".into()),
        ),
        SidecarKind::Review => build_review_prompt_prepared_diff(
            "branch",
            er_dir,
            base,
            head,
            &crate::ai::prepared_diff::diff_tmp_hash(er_dir)
                .unwrap_or_else(|| "<DIFF_HASH>".into()),
            None,
        ),
        SidecarKind::Tour => build_tour_prompt_prepared_diff(
            "PR diff",
            er_dir,
            "tour.json",
            &crate::ai::prepared_diff::diff_tmp_hash(er_dir)
                .unwrap_or_else(|| "<DIFF_HASH>".into()),
        ),
    }
}

/// Compare an upload's recorded hash with the prepared diff.
///
/// The ingest gate lives here, not in the store: the store deliberately loads a
/// stale artifact so callers can mark it (`is_stale`, `expert_hash_accepted`).
/// Only the point where agent output enters the bucket refuses it.
pub(crate) fn check_diff_hash(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("stale {label} (expected diff_hash {expected}, got {actual})");
    }
    Ok(())
}

pub(crate) fn check_version(label: &str, found: u32, expected: u32) -> Result<()> {
    if found != expected {
        bail!("{label} has version {found}; this build accepts {expected}");
    }
    Ok(())
}

/// A file the upload's required-file check already confirmed is present.
fn required_file<'a>(files: &'a BTreeMap<String, String>, name: &str) -> Result<&'a String> {
    files
        .get(name)
        .with_context(|| format!("missing required file '{name}'"))
}

/// The parsed payloads of one upload, validated but not yet written.
enum UploadBundle {
    Triage(crate::ai::TriageReview),
    Tour(crate::ai::ErTour),
    Review(ReviewBundle),
}

struct ReviewBundle {
    review: crate::ai::ErReview,
    order: crate::ai::ErOrder,
    checklist: crate::ai::ErChecklist,
    summary: String,
}

/// Parse, version-check and hash-check in memory so failed uploads never touch
/// the destination files.
fn validate_contents_before_write(
    kind: SidecarKind,
    files: &BTreeMap<String, String>,
    expected_hash: &str,
) -> Result<UploadBundle> {
    use crate::ai::{ErChecklist, ErOrder, ErReview, ErTour, TriageReview};
    use er_store::Artifact as _;

    match kind {
        SidecarKind::Triage => {
            let raw = required_file(files, "triage.json")?;
            let parsed: TriageReview = serde_json::from_str(raw).context("invalid triage.json")?;
            check_version("triage.json", parsed.version, TriageReview::VERSION)?;
            check_diff_hash("triage.json", &parsed.diff_hash, expected_hash)?;
            Ok(UploadBundle::Triage(parsed))
        }
        SidecarKind::Tour => {
            let raw = required_file(files, "tour.json")?;
            let parsed: ErTour = serde_json::from_str(raw).context("invalid tour.json")?;
            check_version("tour.json", parsed.version, ErTour::VERSION)?;
            check_diff_hash("tour.json", &parsed.diff_hash, expected_hash)?;
            Ok(UploadBundle::Tour(parsed))
        }
        SidecarKind::Review => {
            let review: ErReview = serde_json::from_str(required_file(files, "review.json")?)
                .context("invalid review.json")?;
            check_diff_hash("review.json", &review.diff_hash, expected_hash)?;
            check_version("review.json", review.version, ErReview::VERSION)?;
            let order: ErOrder = serde_json::from_str(required_file(files, "order.json")?)
                .context("invalid order.json")?;
            check_diff_hash("order.json", &order.diff_hash, expected_hash)?;
            check_version("order.json", order.version, ErOrder::VERSION)?;
            let checklist: ErChecklist =
                serde_json::from_str(required_file(files, "checklist.json")?)
                    .context("invalid checklist.json")?;
            check_version("checklist.json", checklist.version, ErChecklist::VERSION)?;
            check_diff_hash("checklist.json", &checklist.diff_hash, expected_hash)?;
            let summary = required_file(files, "summary.md")?.clone();
            if summary.trim().is_empty() {
                bail!("summary.md must be non-empty markdown");
            }
            Ok(UploadBundle::Review(ReviewBundle {
                review,
                order,
                checklist,
                summary,
            }))
        }
    }
}

/// Write the validated bundle under the caller's scope guard.
///
/// A review bundle is committed under one hold, so a reader never sees a new
/// `review.json` beside an old `order.json`.
fn commit_bundle(
    guard: &er_store::ScopeGuard,
    kind: SidecarKind,
    bundle: UploadBundle,
) -> Result<Vec<String>> {
    use crate::ai::{ErChecklist, ErOrder, ErReview, ErTour, TriageReview};

    let store = crate::store_registry::store();
    let scope = guard.scope();
    let mut written = Vec::new();
    match (kind, bundle) {
        (SidecarKind::Triage, UploadBundle::Triage(triage)) => {
            let key = er_store::ArtifactKey::<TriageReview>::new(scope.clone(), ())
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            store.write_locked(guard, &key, &triage)?;
            written.push(store.path(&key).to_string_lossy().into_owned());
        }
        (SidecarKind::Tour, UploadBundle::Tour(tour)) => {
            let key = er_store::ArtifactKey::<ErTour>::new(scope.clone(), ())
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            store.write_locked(guard, &key, &tour)?;
            written.push(store.path(&key).to_string_lossy().into_owned());
        }
        (SidecarKind::Review, UploadBundle::Review(bundle)) => {
            let review_key = er_store::ArtifactKey::<ErReview>::new(scope.clone(), ())
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            store.write_locked(guard, &review_key, &bundle.review)?;
            written.push(store.path(&review_key).to_string_lossy().into_owned());
            let order_key = er_store::ArtifactKey::<ErOrder>::new(scope.clone(), ())
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            store.write_locked(guard, &order_key, &bundle.order)?;
            written.push(store.path(&order_key).to_string_lossy().into_owned());
            let checklist_key = er_store::ArtifactKey::<ErChecklist>::new(scope.clone(), ())
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            store.write_locked(guard, &checklist_key, &bundle.checklist)?;
            written.push(store.path(&checklist_key).to_string_lossy().into_owned());
            let summary_file =
                er_store::RelPath::new("summary.md").map_err(|error| anyhow::anyhow!("{error}"))?;
            store.write_text_locked(guard, &summary_file, &bundle.summary)?;
            written.push(
                store
                    .text_path(scope, &summary_file)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        _ => bail!(
            "internal error: upload bundle does not match {}",
            kind.as_str()
        ),
    }
    Ok(written)
}

fn resolve_er_dir(owner: &str, repo: &str, pr: u64) -> String {
    let slug = owner_repo_storage_slug(owner, repo);
    resolve_managed_root_for_pr_bucket(&slug, pr).er_dir()
}

/// Resolve managed PR bucket and write `diff-tmp` for a remote PR.
pub fn prepare_pr_diff_tmp(
    owner: &str,
    repo: &str,
    pr: u64,
    ignore_globs: &[String],
    review_md: Option<&str>,
) -> Result<(String, String)> {
    let mut raw = gh_pr_diff_remote(&*crate::ports::Ports::cli().git, owner, repo, pr)?;
    if !ignore_globs.is_empty() {
        raw = crate::git::filter_raw_diff_exclude_globs(&raw, ignore_globs);
    }
    if raw.trim().is_empty() {
        bail!("PR #{pr} has an empty diff after filters");
    }

    let slug = owner_repo_storage_slug(owner, repo);
    let er_dir = resolve_managed_root_for_pr_bucket(&slug, pr).er_dir();
    if er_dir.is_empty() {
        bail!("failed to resolve managed PR storage for {owner}/{repo}#{pr}");
    }
    // Rule tables are keyed the way `storage::slug_repo` names a clone of this
    // repo (the origin URL's basename), not by the owner/repo storage slug.
    let key = crate::storage::rules_key(
        &*crate::ports::Ports::cli().git,
        "",
        Some(&format!("{owner}/{repo}")),
    );
    crate::ai::prepared_diff::ensure_review_inputs_from_config(&er_dir, &raw, &key, review_md)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let diff_path = format!("{er_dir}/diff-tmp");
    Ok((er_dir, diff_path))
}

/// Fetch PR diff into managed storage and return prompts for the client agent.
pub fn prepare_review_kit(
    owner: &str,
    repo: &str,
    pr: u64,
    kinds: &[SidecarKind],
    ignore_globs: &[String],
) -> Result<PreparedReviewKit> {
    if kinds.is_empty() {
        bail!("at least one artifact kind is required (triage, review, tour)");
    }

    let (meta_base, meta_head) = gh_pr_metadata_remote(owner, repo, pr)
        .unwrap_or_else(|_| ("main".into(), format!("pr-{pr}")));
    let review_md = crate::ai::review_md::collect(
        &crate::ports::Ports::cli(),
        &crate::ai::review_md::ReviewMdSource::Gh {
            repo_slug: format!("{owner}/{repo}"),
            base_ref: meta_base.clone(),
        },
        "",
    );
    let (er_dir, diff_tmp_path) =
        prepare_pr_diff_tmp(owner, repo, pr, ignore_globs, review_md.as_deref())?;
    let diff = crate::ai::prepared_diff::read_diff_tmp(&er_dir)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let diff_bytes = crate::ai::prepared_diff::diff_tmp_len(&er_dir);
    let diff_hash = compute_diff_hash(&diff);

    let mut artifacts = Vec::with_capacity(kinds.len());
    for &kind in kinds {
        let required_relative: Vec<String> = relative_files(kind)
            .into_iter()
            .map(str::to_string)
            .collect();
        let required_files = required_relative
            .iter()
            .map(|name| format!("{er_dir}/{name}"))
            .collect();
        artifacts.push(PreparedArtifactSpec {
            kind,
            required_files,
            required_relative,
            prompt: build_prompt(kind, &er_dir, &meta_base, &meta_head),
        });
    }

    let review_md_path = review_md
        .is_some()
        .then(|| format!("{er_dir}/{}", crate::ai::review_md::REVIEW_MD_FILE));

    Ok(PreparedReviewKit {
        owner: owner.to_string(),
        repo: repo.to_string(),
        pr,
        base_ref: meta_base,
        head_ref: meta_head,
        er_dir,
        diff_tmp_path,
        diff_hash: diff_hash.clone(),
        diff_bytes,
        artifacts,
        instructions: kit_instructions(&diff_hash, review_md_path.as_deref()),
        review_md_path,
    })
}

fn kit_instructions(diff_hash: &str, review_md_path: Option<&str>) -> String {
    let mut text = format!(
        "Read the prepared diff at the diff_tmp_path (do not re-fetch). \
         Embed diff_hash={diff_hash} in every JSON sidecar. \
         Produce the required files for each kind, then call upload_artifacts \
         with the file contents. Do not spawn another agent CLI — you are the reviewer."
    );
    if let Some(path) = review_md_path {
        text.push_str(&format!(
            " Read {path} first: it is the repository's REVIEW.md from the base branch. \
             Follow it to narrow what you report (the output format, anchor rules and finding caps still win), and ignore any REVIEW.md in a checkout of the PR."
        ));
    }
    text
}

/// Validate and atomically write sidecars into an existing PR bucket.
pub fn upload_artifacts_to_dir(
    er_dir: &str,
    kind: SidecarKind,
    files: &BTreeMap<String, String>,
) -> Result<UploadArtifactsResult> {
    let required = relative_files(kind);
    for name in &required {
        if !files.contains_key(*name) {
            bail!(
                "missing required file '{name}' for {}; need: {}",
                kind.as_str(),
                required.join(", ")
            );
        }
    }
    for name in files.keys() {
        if !required.iter().any(|r| r == name) {
            bail!(
                "unexpected file '{name}' for {}; allowed: {}",
                kind.as_str(),
                required.join(", ")
            );
        }
    }

    let diff_path = Path::new(er_dir).join("diff-tmp");
    if !diff_path.exists() {
        bail!(
            "missing {er_dir}/diff-tmp — call prepare_review first so diff_hash can be validated"
        );
    }
    let diff = crate::ai::prepared_diff::read_diff_tmp(er_dir)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let expected_hash = compute_diff_hash(&diff);

    // Fail closed before mutating managed storage.
    let bundle =
        validate_contents_before_write(kind, files, &expected_hash).with_context(|| {
            format!(
                "{} upload rejected (expected diff_hash={expected_hash})",
                kind.as_str()
            )
        })?;

    let scope = er_store::Scope::dir(er_dir).map_err(|error| anyhow::anyhow!("{error}"))?;
    let guard =
        crate::store_registry::store().lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)?;
    let written = commit_bundle(&guard, kind, bundle)?;

    Ok(UploadArtifactsResult {
        er_dir: er_dir.to_string(),
        kind,
        written,
        diff_hash: expected_hash,
    })
}

/// Resolve the PR bucket (optionally refresh `diff-tmp`) and upload artifacts.
pub fn upload_pr_artifacts(req: UploadArtifactsRequest) -> Result<UploadArtifactsResult> {
    let er_dir = if req.refresh_diff {
        prepare_pr_diff_tmp(&req.owner, &req.repo, req.pr, &[], None)?.0
    } else {
        let er_dir = resolve_er_dir(&req.owner, &req.repo, req.pr);
        let diff_path = Path::new(&er_dir).join("diff-tmp");
        if !diff_path.exists() {
            prepare_pr_diff_tmp(&req.owner, &req.repo, req.pr, &[], None)?.0
        } else {
            er_dir
        }
    };
    upload_artifacts_to_dir(&er_dir, req.kind, &req.files)
}

/// One sidecar kind's prompt, required files, JSON Schema, and examples.
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactSpec {
    pub kind: SidecarKind,
    /// Relative filenames that `upload_artifacts` requires for this kind.
    pub required_files: Vec<&'static str>,
    /// JSON Schema (draft-07 style) keyed by relative filename.
    /// `summary.md` is plain markdown — schema is a short description object.
    pub schemas: Value,
    /// Minimal valid examples keyed by relative filename.
    pub examples: Value,
    /// Same prepared-diff prompt Desktop uses (`diff-tmp` already written).
    pub prompt: String,
    pub notes: Vec<&'static str>,
}

/// Bundle returned to MCP clients.
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactSpecsResponse {
    /// Embed this exact SHA-256 hex of `diff-tmp` as every JSON `diff_hash`.
    pub diff_hash_rule: &'static str,
    pub output_dir_placeholder: &'static str,
    pub specs: Vec<ArtifactSpec>,
}

/// A draft-07 schema generated from the payload type the store validates.
fn titled_schema<T: schemars::JsonSchema>(title: &str) -> Value {
    let mut schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<T>()
        .as_value()
        .clone();
    if let Value::Object(map) = &mut schema {
        map.insert("title".into(), Value::String(title.into()));
    }
    permissive_objects(schema)
}

/// The hand-written schemas agents used accepted unknown keys, and serde
/// ignores them; keep that shape now the schema comes from the derive.
fn permissive_objects(mut value: Value) -> Value {
    fn walk(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if map.get("type").and_then(Value::as_str) == Some("object") {
                    // Keep a map's `additionalProperties` value: it carries the
                    // `$ref` that describes the entry type (review files, order
                    // groups, …). Only add permissiveness where the schema did
                    // not already say what the values are.
                    map.entry("additionalProperties")
                        .or_insert(Value::Bool(true));
                }
                for child in map.values_mut() {
                    walk(child);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item);
                }
            }
            _ => {}
        }
    }
    walk(&mut value);
    value
}

fn summary_md_schema() -> Value {
    json!({
        "title": "summary.md",
        "type": "string",
        "description": "Non-empty markdown (3–5 paragraphs) summarizing the overall changes. Not JSON."
    })
}

fn triage_example() -> Value {
    json!({
        "version": 1,
        "diff_hash": "<DIFF_HASH>",
        "diff_scope": "pr",
        "created_at": "2026-01-01T00:00:00Z",
        "first_impression": "Small focused change.",
        "diff_stats": {
            "files_changed": 2,
            "approx_risk": "low",
            "domains": ["ui"]
        },
        "verdict": {
            "primary": "general",
            "experts": [],
            "rationale": "Straightforward UI tweak",
            "confidence": "high"
        },
        "priority_files": [
            { "path": "src/app.rs", "reason": "Main change", "risk": "low" }
        ],
        "sensitive": { "reasons": [] }
    })
}

fn review_examples() -> Value {
    json!({
        "review.json": {
            "version": 1,
            "diff_hash": "<DIFF_HASH>",
            "created_at": "2026-01-01T00:00:00Z",
            "base_branch": "main",
            "head_branch": "feat/example",
            "file_hashes": {},
            "files": {
                "src/app.rs": {
                    "risk": "medium",
                    "risk_reason": "Touches error handling",
                    "summary": "Adds retry for network calls",
                    "findings": [
                        {
                            "id": "f-1",
                            "severity": "medium",
                            "category": "correctness",
                            "title": "Retry may hide auth failures",
                            "description": "…",
                            "hunk_index": 0,
                            "line_start": 42,
                            "suggestion": "Fail fast on 401",
                            "confidence": "confirmed",
                            "outside_diff": false,
                            "related_files": [],
                            "evidence": [],
                            "resolved": false
                        }
                    ]
                }
            }
        },
        "order.json": {
            "version": 1,
            "diff_hash": "<DIFF_HASH>",
            "order": [
                { "path": "src/app.rs", "reason": "Core change", "group": "main" }
            ],
            "groups": {
                "main": { "label": "Main Changes", "color": "red" }
            }
        },
        "checklist.json": {
            "version": 1,
            "diff_hash": "<DIFF_HASH>",
            "items": [
                {
                    "id": "c-1",
                    "text": "Verify retry behavior on 401",
                    "category": "correctness",
                    "checked": false,
                    "related_findings": ["f-1"],
                    "related_files": ["src/app.rs"]
                }
            ]
        },
        "summary.md": "# Summary\n\nOverall description of the change…\n"
    })
}

fn tour_example() -> Value {
    json!({
        "version": 1,
        "diff_hash": "<DIFF_HASH>",
        "created_at": "2026-01-01T00:00:00Z",
        "title": "Tour: Example change",
        "overview": "Start with the foundation types, then the UI.",
        "pillars": [
            {
                "id": "p-1",
                "title": "Foundation",
                "summary": "Core types other files depend on",
                "description": "Core types other files depend on.",
                "order": 0,
                "importance": 90,
                "foundation": true,
                "files": [
                    {
                        "path": "src/types.rs",
                        "reason": "Shared model",
                        "finding_ids": [],
                        "related": [
                            {
                                "path": "src/types.test.rs",
                                "kind": "test",
                                "reason": "Unit tests"
                            }
                        ]
                    }
                ]
            }
        ]
    })
}

fn spec_for(kind: SidecarKind, output_dir: &str, base: &str, head: &str) -> ArtifactSpec {
    match kind {
        SidecarKind::Triage => ArtifactSpec {
            kind,
            required_files: vec!["triage.json"],
            schemas: json!({
                "triage.json": titled_schema::<crate::ai::TriageReview>("triage.json"),
            }),
            examples: json!({ "triage.json": triage_example() }),
            prompt: build_triage_review_prompt_prepared_diff("branch", output_dir, "<DIFF_HASH>"),
            notes: vec![
                "Write only triage.json — no review/order/checklist/summary.",
                "Replace <DIFF_HASH> with the prepare_review diff_hash.",
            ],
        },
        SidecarKind::Review => ArtifactSpec {
            kind,
            required_files: vec!["review.json", "order.json", "checklist.json", "summary.md"],
            schemas: json!({
                "review.json": titled_schema::<crate::ai::ErReview>("review.json"),
                "order.json": titled_schema::<crate::ai::ErOrder>("order.json"),
                "checklist.json": titled_schema::<crate::ai::ErChecklist>("checklist.json"),
                "summary.md": summary_md_schema(),
            }),
            examples: review_examples(),
            prompt: build_review_prompt_prepared_diff(
                "branch",
                output_dir,
                base,
                head,
                "<DIFF_HASH>",
                None,
            ),
            notes: vec![
                "All four files are required for upload_artifacts kind=review.",
                "Every JSON file must share the same diff_hash.",
                "head_branch must match the PR head or Desktop may discard the review.",
                "Leave finding resolved/resolved_note/resolved_at empty/false.",
            ],
        },
        SidecarKind::Tour => ArtifactSpec {
            kind,
            required_files: vec!["tour.json"],
            schemas: json!({
                "tour.json": titled_schema::<crate::ai::ErTour>("tour.json"),
            }),
            examples: json!({ "tour.json": tour_example() }),
            prompt: build_tour_prompt_prepared_diff(
                "PR diff",
                output_dir,
                "tour.json",
                "<DIFF_HASH>",
            ),
            notes: vec![
                "Write only tour.json.",
                "Every changed file appears once — as a pillar files[] entry or nested related[].",
                "3–7 pillars is ideal; foundation pillars first.",
            ],
        },
    }
}

/// Specs for the given kinds (templates use [`OUTPUT_DIR_PLACEHOLDER`]).
pub fn artifact_specs(kinds: &[SidecarKind]) -> ArtifactSpecsResponse {
    artifact_specs_for_dir(
        kinds,
        OUTPUT_DIR_PLACEHOLDER,
        "<base_branch>",
        "<head_branch>",
    )
}

/// Specs with prompts pointing at a real managed bucket (for `prepare_review`).
pub fn artifact_specs_for_dir(
    kinds: &[SidecarKind],
    output_dir: &str,
    base_branch: &str,
    head_branch: &str,
) -> ArtifactSpecsResponse {
    let specs = kinds
        .iter()
        .copied()
        .map(|k| spec_for(k, output_dir, base_branch, head_branch))
        .collect();
    ArtifactSpecsResponse {
        diff_hash_rule: "Every JSON sidecar must set diff_hash to the SHA-256 hex of the prepared diff-tmp file (prepare_review returns this as kit.diff_hash).",
        output_dir_placeholder: OUTPUT_DIR_PLACEHOLDER,
        specs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{
        ErChecklist, ErOrder, ErReview, ErTour, RiskLevel, TourFile, TourPillar, TriageReview,
    };
    use std::collections::HashMap;

    #[test]
    fn kit_instructions_name_review_md_when_present() {
        let with = kit_instructions("abc", Some("/x/review-md.md"));
        assert!(with.contains("/x/review-md.md"));
        assert!(with.contains("REVIEW.md from the base branch"));
        assert!(!kit_instructions("abc", None).contains("REVIEW.md"));
    }

    /// A remote PR must find the same `[risk.<repo>]` / `[file_kinds]` table a
    /// clone of that repo would, or the facts silently say "undeclared".
    #[test]
    fn rules_key_for_a_remote_matches_a_clones_slug_repo() {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap()
        };
        run(&["init", "-q"]);
        run(&[
            "remote",
            "add",
            "origin",
            "git@github.com:Acme/My_Service.git",
        ]);
        let clone_key = crate::storage::slug_repo(
            &*crate::ports::Ports::cli().git,
            dir.path().to_str().unwrap(),
        );
        assert_eq!(
            crate::storage::rules_key(
                &*crate::ports::Ports::for_test().git,
                "",
                Some("Acme/My_Service")
            ),
            clone_key
        );
        assert_eq!(
            crate::storage::rules_key(
                &*crate::ports::Ports::for_test().git,
                "/",
                Some("Acme/My_Service")
            ),
            clone_key
        );
        assert_eq!(
            crate::storage::rules_key(
                &*crate::ports::Ports::cli().git,
                dir.path().to_str().unwrap(),
                None
            ),
            clone_key
        );
        assert_ne!(owner_repo_storage_slug("Acme", "My_Service"), clone_key);
    }

    fn with_storage_root<T>(f: impl FnOnce() -> T) -> T {
        let root = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(root.path(), f)
    }

    fn write_diff_tmp(er_dir: &str, diff: &str) {
        std::fs::create_dir_all(er_dir).unwrap();
        std::fs::write(format!("{er_dir}/diff-tmp"), diff).unwrap();
    }

    #[test]
    fn upload_tour_validates_hash() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket(
                &crate::storage::repo_storage_slug("acme", "widgets"),
                7,
            )
            .er_dir();
            let diff = "diff --git a/x b/x\n+hello\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            let tour = ErTour {
                version: 1,
                diff_hash: hash,
                created_at: "2026-01-01T00:00:00Z".into(),
                title: "Tour".into(),
                overview: "hi".into(),
                pillars: vec![TourPillar {
                    id: "p1".into(),
                    title: "Core".into(),
                    description: "d".into(),
                    order: 0,
                    importance: 80,
                    summary: String::new(),
                    foundation: true,
                    files: vec![TourFile {
                        path: "x".into(),
                        reason: "changed".into(),
                        finding_ids: vec![],
                        related: vec![],
                    }],
                }],
            };
            let mut files = BTreeMap::new();
            files.insert(
                "tour.json".into(),
                serde_json::to_string_pretty(&tour).unwrap(),
            );
            let result = upload_artifacts_to_dir(&er_dir, SidecarKind::Tour, &files).unwrap();
            assert_eq!(result.written.len(), 1);
            assert!(Path::new(&er_dir).join("tour.json").exists());
        });
    }

    #[test]
    fn an_uploaded_bare_tour_is_committed_with_an_envelope() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket(
                &crate::storage::repo_storage_slug("acme", "widgets"),
                9,
            )
            .er_dir();
            let diff = "diff --git a/x b/x\n+hello\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            upload_artifacts_to_dir(&er_dir, SidecarKind::Tour, &tour_files(&hash)).unwrap();

            let raw: serde_json::Value = serde_json::from_slice(
                &std::fs::read(Path::new(&er_dir).join("tour.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(raw["kind"], "tour");
            assert_eq!(raw["payload"]["diff_hash"], hash);
            let tour: ErTour = found(&er_dir);
            assert_eq!(tour.diff_hash, hash);
        });
    }

    #[test]
    fn upload_rejects_stale_hash() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket(
                &crate::storage::repo_storage_slug("acme", "widgets"),
                8,
            )
            .er_dir();
            write_diff_tmp(&er_dir, "diff --git a/x b/x\n+hello\n");

            let tour = ErTour {
                version: 1,
                diff_hash: "deadbeef".into(),
                created_at: "".into(),
                title: "Tour".into(),
                overview: "".into(),
                pillars: vec![],
            };
            let mut files = BTreeMap::new();
            files.insert("tour.json".into(), serde_json::to_string(&tour).unwrap());
            assert!(upload_artifacts_to_dir(&er_dir, SidecarKind::Tour, &files).is_err());
            assert!(
                !Path::new(&er_dir).join("tour.json").exists(),
                "failed upload must not leave tour.json on disk"
            );
        });
    }

    #[test]
    fn relative_files_for_review_include_all_four() {
        assert_eq!(
            relative_files(SidecarKind::Review),
            vec!["review.json", "order.json", "checklist.json", "summary.md"]
        );
    }

    #[test]
    fn review_bundle_validates() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket(
                &crate::storage::repo_storage_slug("acme", "widgets"),
                9,
            )
            .er_dir();
            let diff = "diff --git a/a.rs b/a.rs\n+1\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            let mut review_files = HashMap::new();
            review_files.insert(
                "a.rs".into(),
                crate::ai::ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: "".into(),
                    summary: "ok".into(),
                    findings: vec![],
                },
            );
            let review = ErReview {
                version: 1,
                diff_hash: hash.clone(),
                created_at: "2026-01-01T00:00:00Z".into(),
                base_branch: "main".into(),
                head_branch: "feat".into(),
                files: review_files,
                file_hashes: HashMap::new(),
            };
            let order = ErOrder {
                version: 1,
                diff_hash: hash.clone(),
                order: vec![],
                groups: HashMap::new(),
            };
            let checklist = ErChecklist {
                version: 1,
                diff_hash: hash,
                items: vec![],
            };

            let mut files = BTreeMap::new();
            files.insert(
                "review.json".into(),
                serde_json::to_string(&review).unwrap(),
            );
            files.insert("order.json".into(), serde_json::to_string(&order).unwrap());
            files.insert(
                "checklist.json".into(),
                serde_json::to_string(&checklist).unwrap(),
            );
            files.insert("summary.md".into(), "# Summary\n".into());
            upload_artifacts_to_dir(&er_dir, SidecarKind::Review, &files).unwrap();
        });
    }

    #[test]
    fn triage_min_json_ok() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket(
                &crate::storage::repo_storage_slug("acme", "widgets"),
                10,
            )
            .er_dir();
            let diff = "diff --git a/t b/t\n+t\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);
            let triage = TriageReview {
                version: 1,
                diff_hash: hash,
                diff_scope: "pr".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                first_impression: "small".into(),
                diff_stats: Default::default(),
                verdict: Default::default(),
                priority_files: vec![],
                reach: Default::default(),
                sensitive: Default::default(),
            };
            let mut files = BTreeMap::new();
            files.insert(
                "triage.json".into(),
                serde_json::to_string(&triage).unwrap(),
            );
            upload_artifacts_to_dir(&er_dir, SidecarKind::Triage, &files).unwrap();
        });
    }

    #[test]
    fn upload_requires_diff_tmp() {
        with_storage_root(|| {
            let er_dir = resolve_managed_root_for_pr_bucket(
                &crate::storage::repo_storage_slug("acme", "widgets"),
                11,
            )
            .er_dir();
            std::fs::create_dir_all(&er_dir).unwrap();
            let mut files = BTreeMap::new();
            files.insert(
                "tour.json".into(),
                r#"{"version":1,"diff_hash":"x"}"#.into(),
            );
            let err = upload_artifacts_to_dir(&er_dir, SidecarKind::Tour, &files)
                .unwrap_err()
                .to_string();
            assert!(err.contains("diff-tmp"), "{err}");
        });
    }

    // ── Specs from the derived schemas ──

    #[test]
    fn all_kinds_have_schemas_and_prompts() {
        let resp = artifact_specs(&[SidecarKind::Triage, SidecarKind::Review, SidecarKind::Tour]);
        assert_eq!(resp.specs.len(), 3);
        for spec in &resp.specs {
            assert!(!spec.required_files.is_empty());
            assert!(spec.schemas.is_object());
            assert!(
                spec.prompt.contains("diff-tmp") || spec.prompt.contains("diff_hash"),
                "{}",
                spec.prompt
            );
            for file in &spec.required_files {
                assert!(
                    spec.schemas.get(file).is_some(),
                    "missing schema for {file}"
                );
            }
        }
    }

    #[test]
    fn triage_kit_exposes_sensitive_prompt_schema_and_example() {
        let response = artifact_specs(&[SidecarKind::Triage]);
        let spec = &response.specs[0];
        assert!(spec.schemas["triage.json"]["properties"]["sensitive"].is_object());
        assert!(spec.examples["triage.json"]["sensitive"]["reasons"].is_array());
        assert!(spec.prompt.contains("sensitive"));
    }

    #[test]
    fn review_requires_four_files() {
        let resp = artifact_specs(&[SidecarKind::Review]);
        assert_eq!(resp.specs[0].required_files.len(), 4);
    }

    #[test]
    fn derived_schema_lists_every_field_of_the_example() {
        let resp = artifact_specs(&[SidecarKind::Triage, SidecarKind::Review, SidecarKind::Tour]);
        for spec in &resp.specs {
            for file in &spec.required_files {
                if !file.ends_with(".json") {
                    continue;
                }
                let properties = spec.schemas[*file]["properties"]
                    .as_object()
                    .unwrap_or_else(|| panic!("{file}: schema has no properties"));
                for key in spec.examples[*file]
                    .as_object()
                    .unwrap_or_else(|| panic!("{file}: example is not an object"))
                    .keys()
                {
                    assert!(
                        properties.contains_key(key),
                        "{file}: schema lacks example field {key}"
                    );
                }
                assert!(properties.contains_key("version"), "{file}: version");
                assert!(properties.contains_key("diff_hash"), "{file}: diff_hash");
            }
        }
    }

    /// Every dotted path the hand-written release schemas described, relative
    /// to each file's schema. The generated schemas must still carry all of
    /// them; this is the release snapshot the derive is checked against.
    const RELEASE_SCHEMA_PATHS: &[(&str, &[&str])] = &[
        (
            "triage.json",
            &[
                "version",
                "diff_hash",
                "diff_scope",
                "created_at",
                "first_impression",
                "diff_stats",
                "diff_stats.files_changed",
                "diff_stats.approx_risk",
                "diff_stats.domains",
                "verdict",
                "verdict.primary",
                "verdict.experts",
                "verdict.rationale",
                "verdict.confidence",
                "priority_files",
                "priority_files.path",
                "priority_files.reason",
                "priority_files.risk",
                "reach",
                "reach.level",
                "reach.reason",
                "reach.touch_points",
                "reach.guard",
                "reach.guard.kind",
                "reach.guard.name",
                "reach.guard.evidence",
                "sensitive",
                "sensitive.reasons",
                "sensitive.reasons.kind",
                "sensitive.reasons.evidence",
                "sensitive.reasons.note",
            ],
        ),
        (
            "review.json",
            &[
                "version",
                "diff_hash",
                "created_at",
                "base_branch",
                "head_branch",
                "file_hashes",
                "files",
                "files.risk",
                "files.risk_reason",
                "files.summary",
                "files.findings",
                "files.findings.id",
                "files.findings.severity",
                "files.findings.category",
                "files.findings.title",
                "files.findings.description",
                "files.findings.hunk_index",
                "files.findings.line_start",
                "files.findings.line_end",
                "files.findings.line_content",
                "files.findings.suggestion",
                "files.findings.related_files",
                "files.findings.outside_diff",
                "files.findings.confidence",
                "files.findings.verification_plan",
                "files.findings.evidence",
                "files.findings.evidence.file",
                "files.findings.evidence.line_start",
                "files.findings.evidence.line_end",
                "files.findings.evidence.note",
                "files.findings.resolved",
                "files.findings.resolved_note",
                "files.findings.resolved_at",
            ],
        ),
        (
            "order.json",
            &[
                "version",
                "diff_hash",
                "order",
                "order.path",
                "order.reason",
                "order.group",
                "groups",
                "groups.label",
                "groups.color",
            ],
        ),
        (
            "checklist.json",
            &[
                "version",
                "diff_hash",
                "items",
                "items.id",
                "items.text",
                "items.category",
                "items.checked",
                "items.related_findings",
                "items.related_files",
            ],
        ),
        (
            "tour.json",
            &[
                "version",
                "diff_hash",
                "created_at",
                "title",
                "overview",
                "pillars",
                "pillars.id",
                "pillars.title",
                "pillars.description",
                "pillars.summary",
                "pillars.order",
                "pillars.importance",
                "pillars.foundation",
                "pillars.files",
                "pillars.files.path",
                "pillars.files.reason",
                "pillars.files.finding_ids",
                "pillars.files.related",
                "pillars.files.related.path",
                "pillars.files.related.kind",
                "pillars.files.related.reason",
            ],
        ),
    ];

    /// Flatten a schema into dotted property paths, following `$ref`s and the
    /// collection shapes schemars emits (items, additionalProperties, anyOf).
    fn schema_paths(schema: &Value) -> std::collections::BTreeSet<String> {
        fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
            if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
                let name = reference
                    .strip_prefix("#/$defs/")
                    .or_else(|| reference.strip_prefix("#/definitions/"));
                if let Some(name) = name {
                    if let Some(target) = root
                        .get("$defs")
                        .or_else(|| root.get("definitions"))
                        .and_then(|defs| defs.get(name))
                    {
                        return target;
                    }
                }
            }
            node
        }
        fn walk(
            root: &Value,
            node: &Value,
            prefix: &str,
            out: &mut std::collections::BTreeSet<String>,
        ) {
            let node = resolve(root, node);
            if let Some(properties) = node.get("properties").and_then(Value::as_object) {
                for (name, child) in properties {
                    let path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}.{name}")
                    };
                    out.insert(path.clone());
                    walk(root, child, &path, out);
                }
            }
            if let Some(items) = node.get("items") {
                walk(root, items, prefix, out);
            }
            if let Some(values) = node.get("additionalProperties") {
                if values.is_object() {
                    walk(root, values, prefix, out);
                }
            }
            for key in ["anyOf", "oneOf", "allOf"] {
                if let Some(items) = node.get(key).and_then(Value::as_array) {
                    for item in items {
                        walk(root, item, prefix, out);
                    }
                }
            }
        }
        let mut out = std::collections::BTreeSet::new();
        walk(schema, schema, "", &mut out);
        out
    }

    #[test]
    fn derived_schema_keeps_every_release_path() {
        let response =
            artifact_specs(&[SidecarKind::Triage, SidecarKind::Review, SidecarKind::Tour]);
        for (file, expected) in RELEASE_SCHEMA_PATHS {
            let schema = response
                .specs
                .iter()
                .find_map(|spec| spec.schemas.get(*file))
                .unwrap_or_else(|| panic!("no schema for {file}"));
            let paths = schema_paths(schema);
            for path in *expected {
                assert!(paths.contains(*path), "{file}: schema lost {path}");
            }
        }
    }

    #[test]
    fn spec_examples_deserialize_into_their_types() {
        let hash = "abc123";
        let resp = artifact_specs(&[SidecarKind::Triage, SidecarKind::Review, SidecarKind::Tour]);
        for spec in &resp.specs {
            match spec.kind {
                SidecarKind::Triage => {
                    let raw = spec.examples["triage.json"]
                        .to_string()
                        .replace("<DIFF_HASH>", hash);
                    let parsed: TriageReview = serde_json::from_str(&raw).unwrap();
                    assert_eq!(parsed.version, 1);
                }
                SidecarKind::Tour => {
                    let raw = spec.examples["tour.json"]
                        .to_string()
                        .replace("<DIFF_HASH>", hash);
                    let parsed: ErTour = serde_json::from_str(&raw).unwrap();
                    assert_eq!(parsed.pillars.len(), 1);
                }
                SidecarKind::Review => {
                    let review_raw = spec.examples["review.json"]
                        .to_string()
                        .replace("<DIFF_HASH>", hash);
                    let review: ErReview = serde_json::from_str(&review_raw).unwrap();
                    assert_eq!(review.files.len(), 1);
                    let order_raw = spec.examples["order.json"]
                        .to_string()
                        .replace("<DIFF_HASH>", hash);
                    let order: ErOrder = serde_json::from_str(&order_raw).unwrap();
                    assert_eq!(order.order.len(), 1);
                    let checklist_raw = spec.examples["checklist.json"]
                        .to_string()
                        .replace("<DIFF_HASH>", hash);
                    let checklist: ErChecklist = serde_json::from_str(&checklist_raw).unwrap();
                    assert_eq!(checklist.items.len(), 1);
                    assert!(spec.examples["summary.md"]
                        .as_str()
                        .is_some_and(|summary| !summary.trim().is_empty()));
                }
            }
        }
    }

    // ── Store commits ──

    fn bucket(number: u64) -> String {
        resolve_managed_root_for_pr_bucket(
            &crate::storage::repo_storage_slug("acme", "widgets"),
            number,
        )
        .er_dir()
    }

    fn review_bundle(hash: &str) -> BTreeMap<String, String> {
        let mut review_files = HashMap::new();
        review_files.insert(
            "a.rs".to_string(),
            crate::ai::ErFileReview {
                risk: RiskLevel::Low,
                risk_reason: String::new(),
                summary: "ok".to_string(),
                findings: vec![],
            },
        );
        let review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            base_branch: "main".to_string(),
            head_branch: "feat".to_string(),
            files: review_files,
            file_hashes: HashMap::new(),
        };
        let order = ErOrder {
            version: 1,
            diff_hash: hash.to_string(),
            order: vec![],
            groups: HashMap::new(),
        };
        let checklist = ErChecklist {
            version: 1,
            diff_hash: hash.to_string(),
            items: vec![],
        };
        let mut files = BTreeMap::new();
        files.insert(
            "review.json".to_string(),
            serde_json::to_string_pretty(&review).unwrap(),
        );
        files.insert(
            "order.json".to_string(),
            serde_json::to_string_pretty(&order).unwrap(),
        );
        files.insert(
            "checklist.json".to_string(),
            serde_json::to_string_pretty(&checklist).unwrap(),
        );
        files.insert("summary.md".to_string(), "# Summary\n".to_string());
        files
    }

    fn triage_files(hash: &str) -> BTreeMap<String, String> {
        let triage = TriageReview {
            version: 1,
            diff_hash: hash.to_string(),
            diff_scope: "pr".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            first_impression: "small".to_string(),
            diff_stats: Default::default(),
            verdict: Default::default(),
            priority_files: vec![],
            reach: Default::default(),
            sensitive: Default::default(),
        };
        BTreeMap::from([(
            "triage.json".to_string(),
            serde_json::to_string(&triage).unwrap(),
        )])
    }

    fn tour_files(hash: &str) -> BTreeMap<String, String> {
        let tour = ErTour {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            title: "Tour".to_string(),
            overview: String::new(),
            pillars: vec![],
        };
        BTreeMap::from([(
            "tour.json".to_string(),
            serde_json::to_string(&tour).unwrap(),
        )])
    }

    fn found<T: er_store::Artifact<Slot = ()>>(er_dir: &str) -> T {
        let store = crate::store_registry::store();
        let scope = er_store::Scope::dir(er_dir).unwrap();
        let key = er_store::ArtifactKey::<T>::new(scope, ()).unwrap();
        match store.read(&key).unwrap() {
            er_store::Loaded::Found(stored) => stored.value,
            _ => panic!("expected Found"),
        }
    }

    fn summary_text(er_dir: &str) -> Option<String> {
        let store = crate::store_registry::store();
        let scope = er_store::Scope::dir(er_dir).unwrap();
        let file = er_store::RelPath::new("summary.md").unwrap();
        match store.read_text(&scope, &file).unwrap() {
            er_store::Loaded::Found(stored) => Some(stored.value),
            _ => None,
        }
    }

    #[test]
    fn upload_review_reads_back_through_the_store() {
        with_storage_root(|| {
            let er_dir = bucket(20);
            let diff = "diff --git a/a.rs b/a.rs\n+1\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            upload_artifacts_to_dir(&er_dir, SidecarKind::Review, &review_bundle(&hash)).unwrap();

            let review: ErReview = found(&er_dir);
            assert_eq!(review.diff_hash, hash);
            let _: ErOrder = found(&er_dir);
            let _: ErChecklist = found(&er_dir);
            assert_eq!(summary_text(&er_dir).as_deref(), Some("# Summary\n"));
            let state = crate::ai::load_ai_state(&er_dir, &hash, None);
            assert_eq!(state.review.expect("review").files.len(), 1);
        });
    }

    #[test]
    fn failed_upload_leaves_existing_artifacts_untouched() {
        with_storage_root(|| {
            let er_dir = bucket(21);
            let diff = "diff --git a/a.rs b/a.rs\n+1\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);
            upload_artifacts_to_dir(&er_dir, SidecarKind::Review, &review_bundle(&hash)).unwrap();
            let before = std::fs::read_to_string(format!("{er_dir}/review.json")).unwrap();

            let mut files = review_bundle(&hash);
            let mut checklist: ErChecklist =
                serde_json::from_str(&files["checklist.json"]).unwrap();
            checklist.diff_hash = "deadbeef".to_string();
            files.insert(
                "checklist.json".to_string(),
                serde_json::to_string(&checklist).unwrap(),
            );

            assert!(upload_artifacts_to_dir(&er_dir, SidecarKind::Review, &files).is_err());
            assert_eq!(
                std::fs::read_to_string(format!("{er_dir}/review.json")).unwrap(),
                before
            );
            let review: ErReview = found(&er_dir);
            assert_eq!(review.diff_hash, hash);
        });
    }

    #[test]
    fn stale_upload_is_rejected_at_ingest_with_a_reason() {
        with_storage_root(|| {
            let er_dir = bucket(22);
            let diff = "diff --git a/a.rs b/a.rs\n+1\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            let cases: [(SidecarKind, &str, BTreeMap<String, String>); 3] = [
                (SidecarKind::Triage, "triage.json", triage_files("deadbeef")),
                (SidecarKind::Tour, "tour.json", tour_files("deadbeef")),
                (SidecarKind::Review, "review.json", {
                    let mut files = review_bundle(&hash);
                    let mut review: ErReview = serde_json::from_str(&files["review.json"]).unwrap();
                    review.diff_hash = "deadbeef".to_string();
                    files.insert(
                        "review.json".to_string(),
                        serde_json::to_string(&review).unwrap(),
                    );
                    files
                }),
            ];
            for (kind, file, files) in cases {
                let error = upload_artifacts_to_dir(&er_dir, kind, &files).unwrap_err();
                let text = format!("{error:#}");
                assert!(text.contains(&format!("stale {file}")), "{file}: {text}");
                assert!(text.contains(&hash), "{file}: {text}");
                assert!(text.contains("deadbeef"), "{file}: {text}");
            }
            assert!(summary_text(&er_dir).is_none());
            let store = crate::store_registry::store();
            let scope = er_store::Scope::dir(&er_dir).unwrap();
            let review = er_store::ArtifactKey::<ErReview>::new(scope.clone(), ()).unwrap();
            let order = er_store::ArtifactKey::<ErOrder>::new(scope.clone(), ()).unwrap();
            let checklist = er_store::ArtifactKey::<ErChecklist>::new(scope, ()).unwrap();
            assert!(
                !store.path(&review).is_file(),
                "{}",
                store.path(&review).display()
            );
            assert!(
                !store.path(&order).is_file(),
                "{}",
                store.path(&order).display()
            );
            assert!(
                !store.path(&checklist).is_file(),
                "{}",
                store.path(&checklist).display()
            );
        });
    }

    #[test]
    fn fresh_upload_is_committed_at_ingest() {
        with_storage_root(|| {
            let er_dir = bucket(23);
            let diff = "diff --git a/a.rs b/a.rs\n+1\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            let result =
                upload_artifacts_to_dir(&er_dir, SidecarKind::Review, &review_bundle(&hash))
                    .unwrap();

            assert_eq!(result.diff_hash, hash);
            let review: ErReview = found(&er_dir);
            let order: ErOrder = found(&er_dir);
            let checklist: ErChecklist = found(&er_dir);
            assert_eq!(review.diff_hash, hash);
            assert_eq!(order.diff_hash, hash);
            assert_eq!(checklist.diff_hash, hash);
        });
    }

    #[test]
    fn upload_rejects_a_non_one_version() {
        with_storage_root(|| {
            let er_dir = bucket(24);
            let diff = "diff --git a/t b/t\n+t\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);

            let bump = |files: &mut BTreeMap<String, String>, name: &str| {
                let mut value: Value = serde_json::from_str(&files[name]).unwrap();
                value["version"] = json!(2);
                files.insert(name.to_string(), value.to_string());
            };

            let mut triage = triage_files(&hash);
            bump(&mut triage, "triage.json");
            let mut tour = tour_files(&hash);
            bump(&mut tour, "tour.json");
            let mut cases: Vec<(SidecarKind, &str, BTreeMap<String, String>)> = vec![
                (SidecarKind::Triage, "triage.json", triage),
                (SidecarKind::Tour, "tour.json", tour),
            ];
            for name in ["review.json", "order.json", "checklist.json"] {
                let mut files = review_bundle(&hash);
                bump(&mut files, name);
                cases.push((SidecarKind::Review, name, files));
            }

            for (kind, name, files) in cases {
                let error = upload_artifacts_to_dir(&er_dir, kind, &files).unwrap_err();
                let text = format!("{error:#}");
                assert!(text.contains(name), "{name}: {text}");
                assert!(text.contains("version 2"), "{name}: {text}");
            }
            for name in [
                "triage.json",
                "tour.json",
                "review.json",
                "order.json",
                "checklist.json",
            ] {
                assert!(
                    !Path::new(&er_dir).join(name).exists(),
                    "{name} was written"
                );
            }
            assert!(summary_text(&er_dir).is_none());
        });
    }

    #[test]
    fn stale_stored_review_still_reads_as_found() {
        with_storage_root(|| {
            let er_dir = bucket(25);
            let stale = ErReview {
                version: 1,
                diff_hash: "old".to_string(),
                created_at: String::new(),
                base_branch: String::new(),
                head_branch: String::new(),
                files: HashMap::new(),
                file_hashes: HashMap::new(),
            };
            let store = crate::store_registry::store();
            let scope = er_store::Scope::dir(&er_dir).unwrap();
            let key = er_store::ArtifactKey::<ErReview>::new(scope, ()).unwrap();
            store.write(&key, &stale).unwrap();

            match store.read(&key).unwrap() {
                er_store::Loaded::Found(stored) => assert_eq!(stored.value.diff_hash, "old"),
                other => panic!("expected Found, got {other:?}"),
            }
            assert!(crate::ai::load_ai_state(&er_dir, "new", None).is_stale);
        });
    }

    #[test]
    fn upload_blocks_on_a_held_scope_lock() {
        with_storage_root(|| {
            let er_dir = bucket(26);
            let diff = "diff --git a/a.rs b/a.rs\n+1\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);
            let store = crate::store_registry::store();
            let scope = er_store::Scope::dir(&er_dir).unwrap();
            let guard = store
                .lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)
                .unwrap();

            let error =
                upload_artifacts_to_dir(&er_dir, SidecarKind::Review, &review_bundle(&hash))
                    .unwrap_err();
            drop(guard);

            let text = format!("{error:#}").to_lowercase();
            assert!(text.contains("already holds"), "{text}");
            assert!(!Path::new(&er_dir).join("review.json").exists());
        });
    }

    #[test]
    fn review_bundle_commit_uses_one_guard() {
        with_storage_root(|| {
            let er_dir = bucket(27);
            let diff = "diff --git a/a.rs b/a.rs\n+1\n";
            write_diff_tmp(&er_dir, diff);
            let hash = compute_diff_hash(diff);
            let files = review_bundle(&hash);
            let bundle =
                validate_contents_before_write(SidecarKind::Review, &files, &hash).unwrap();
            let store = crate::store_registry::store();
            let scope = er_store::Scope::dir(&er_dir).unwrap();
            let guard = store
                .lock_scope(&scope, er_store::DEFAULT_LOCK_TIMEOUT)
                .unwrap();

            let written = commit_bundle(&guard, SidecarKind::Review, bundle).unwrap();
            assert_eq!(written.len(), 4);
            let error = store
                .lock_scope(&scope, std::time::Duration::from_millis(1))
                .err()
                .expect("the second lock must fail while the guard is held");
            assert!(
                matches!(error, er_store::StoreError::AlreadyLocked { .. }),
                "{error:?}"
            );
            drop(guard);

            let _: ErReview = found(&er_dir);
            let _: ErOrder = found(&er_dir);
            let _: ErChecklist = found(&er_dir);
            assert!(summary_text(&er_dir).is_some());
        });
    }
}
