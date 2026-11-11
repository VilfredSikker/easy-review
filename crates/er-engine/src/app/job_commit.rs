//! Host commit of a job's staged output.
//!
//! The agent writes into a [`super::job_staging::JobStaging`] directory. After
//! the run the host validates the staged files (the same version, diff-hash and
//! required-file rules the upload path uses), then commits them through
//! `ArtifactStore` under one scope lock. A malformed output fails the job with
//! a reason and leaves the stored artifacts untouched.

use anyhow::{Context, Result};
use er_store::{Artifact, ArtifactKey, ArtifactStore, Scope, DEFAULT_LOCK_TIMEOUT};

use crate::agent_runtime::{ArtifactBaseline, ArtifactContract};

/// The contract a background task kind must satisfy, or `None` for a kind that
/// writes no sidecar (stdout, importance, diagrams).
pub fn job_contract(task_kind: &str) -> ArtifactContract {
    match task_kind {
        "review" => ArtifactContract::Review,
        "professor" => ArtifactContract::Professor,
        "triage" => ArtifactContract::Triage,
        "tour" => ArtifactContract::Tour {
            filename: "tour.json".into(),
        },
        other => match other.strip_prefix("expert:") {
            Some(id) => ArtifactContract::Expert { id: id.to_string() },
            None => ArtifactContract::None,
        },
    }
}

/// The contract a tab-local prompt job must satisfy.
///
/// Every tab command name is listed here on purpose: an unmapped name is an
/// error, so a new sidecar-writing job cannot silently keep writing the
/// bucket. `None` marks the kinds that write nothing the host commits.
pub fn tab_job_contract(command_name: &str) -> Result<ArtifactContract> {
    Ok(match command_name {
        "review" => ArtifactContract::Review,
        "professor" => ArtifactContract::Professor,
        "triage" => ArtifactContract::Triage,
        "summary" => ArtifactContract::Summary,
        "validate" => ArtifactContract::ValidateReview,
        // User-edited kinds: the host merges their rewrite by id.
        "questions" => ArtifactContract::Questions,
        "notes" => ArtifactContract::Notes,
        "validate-comments" => ArtifactContract::ValidateComments,
        // Stdout-only tab jobs: the startup-trim probe and send-to-agent.
        "send-to-agent" => ArtifactContract::None,
        other if other.starts_with("trim-") => ArtifactContract::None,
        other => match other.strip_prefix("expert-") {
            Some(id) => ArtifactContract::Expert { id: id.to_string() },
            None => anyhow::bail!("tab job `{other}` has no artifact contract"),
        },
    })
}

/// Files a staged job reads and rewrites in place, whose launch-time copy the
/// update-kind merge needs as its base.
pub fn update_inputs(contract: ArtifactContract) -> &'static [&'static str] {
    match contract {
        ArtifactContract::Questions => &["questions.json"],
        ArtifactContract::Notes => &["notes.json"],
        ArtifactContract::ValidateComments => &["github-comments.json", "summary.md"],
        ArtifactContract::None
        | ArtifactContract::Review
        | ArtifactContract::Expert { .. }
        | ArtifactContract::Professor
        | ArtifactContract::Triage
        | ArtifactContract::Tour { .. }
        | ArtifactContract::Summary
        | ArtifactContract::ValidateReview => &[],
    }
}

/// Capture the pre-run state a contract's files must change from.
pub fn capture_contract(
    contract: ArtifactContract,
    staging: &str,
) -> Result<Option<ArtifactBaseline>> {
    if contract == ArtifactContract::None {
        return Ok(None);
    }
    ArtifactBaseline::capture(contract, staging)
        .map(Some)
        .with_context(|| "capture the pre-run baseline".to_string())
}

/// Capture the pre-run state a background task kind's files must change from.
pub fn capture_baseline(
    task_kind: &str,
    scoped: bool,
    staging: &str,
) -> Result<Option<ArtifactBaseline>> {
    if scoped {
        return Ok(None);
    }
    capture_contract(job_contract(task_kind), staging)
}

/// What one commit wrote, and which agent changes lost to user edits.
#[derive(Debug, Default)]
pub struct CommitReport {
    pub written: Vec<String>,
    pub dropped: Vec<String>,
}

/// Validate the staged files against a background task kind's contract and
/// commit them.
pub fn commit_staged(
    task_kind: &str,
    staging: &str,
    er_dir: &str,
    baseline: Option<&ArtifactBaseline>,
    store: &ArtifactStore,
) -> Result<CommitReport> {
    commit_contract(job_contract(task_kind), staging, er_dir, baseline, store)
        .map_err(|error| commit_error(task_kind, error))
}

/// Name the job in a commit failure, with the cause in the message.
///
/// The job's error reaches the UI as one string, and "failed to commit" alone
/// hides which check refused the output.
pub fn commit_error(label: &str, error: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!("{label} failed to commit its output: {error:#}")
}

/// Validate the staged files against `contract` and commit them.
pub fn commit_contract(
    contract: ArtifactContract,
    staging: &str,
    er_dir: &str,
    baseline: Option<&ArtifactBaseline>,
    store: &ArtifactStore,
) -> Result<CommitReport> {
    if contract == ArtifactContract::None {
        return Ok(CommitReport::default());
    }
    if let Some(baseline) = baseline {
        baseline
            .validate(staging)
            .context("the agent wrote an invalid sidecar")?;
    }
    let expected_hash = if contract.requires_diff_hash() {
        Some(staged_diff_hash(staging)?)
    } else {
        None
    };
    let scope = scope_for(er_dir)?;
    let guard = store
        .lock_scope(&scope, DEFAULT_LOCK_TIMEOUT)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let mut written = Vec::new();
    let mut dropped = Vec::new();
    match contract {
        ArtifactContract::None => {}
        ArtifactContract::Review => {
            let review = read_checked::<crate::ai::ErReview>(
                staging,
                "review.json",
                expected_hash.as_deref(),
                |v| v.version,
                |v| &v.diff_hash,
            )?;
            let order = read_checked::<crate::ai::ErOrder>(
                staging,
                "order.json",
                expected_hash.as_deref(),
                |v| v.version,
                |v| &v.diff_hash,
            )?;
            let checklist = read_checked::<crate::ai::ErChecklist>(
                staging,
                "checklist.json",
                expected_hash.as_deref(),
                |v| v.version,
                |v| &v.diff_hash,
            )?;
            let summary = read_non_empty(staging, "summary.md")?;
            written.push(write_artifact(store, &guard, scope.clone(), (), &review)?);
            written.push(write_artifact(store, &guard, scope.clone(), (), &order)?);
            written.push(write_artifact(
                store,
                &guard,
                scope.clone(),
                (),
                &checklist,
            )?);
            written.push(write_summary(store, &guard, &summary)?);
        }
        ArtifactContract::Expert { id } => {
            let expert = read_checked::<crate::ai::ExpertReview>(
                staging,
                &format!("experts/{id}.json"),
                expected_hash.as_deref(),
                |v| v.version,
                |v| &v.diff_hash,
            )?;
            let slot = er_store::ExpertId::new(&id).map_err(|error| anyhow::anyhow!("{error}"))?;
            written.push(write_artifact(store, &guard, scope.clone(), slot, &expert)?);
        }
        ArtifactContract::Professor => {
            let professor = read_checked::<crate::ai::ProfessorReview>(
                staging,
                "professor.json",
                expected_hash.as_deref(),
                |v| v.version,
                |v| &v.diff_hash,
            )?;
            written.push(write_artifact(
                store,
                &guard,
                scope.clone(),
                (),
                &professor,
            )?);
        }
        ArtifactContract::Triage => {
            let triage = read_checked::<crate::ai::TriageReview>(
                staging,
                "triage.json",
                expected_hash.as_deref(),
                |v| v.version,
                |v| &v.diff_hash,
            )?;
            written.push(write_artifact(store, &guard, scope.clone(), (), &triage)?);
        }
        ArtifactContract::Tour { filename } => {
            let tour = read_checked::<crate::ai::ErTour>(
                staging,
                &filename,
                expected_hash.as_deref(),
                |v| v.version,
                |v| &v.diff_hash,
            )?;
            written.push(write_artifact(store, &guard, scope.clone(), (), &tour)?);
        }
        ArtifactContract::Summary => {
            let summary = read_non_empty(staging, "summary.md")?;
            written.push(write_summary(store, &guard, &summary)?);
        }
        ArtifactContract::ValidateReview => {
            let validation = read_checked::<crate::ai::ValidationReview>(
                staging,
                "validation.json",
                expected_hash.as_deref(),
                |v| v.version,
                |v| &v.diff_hash,
            )?;
            let summary = read_non_empty(staging, "summary.md")?;
            written.push(write_artifact(
                store,
                &guard,
                scope.clone(),
                (),
                &validation,
            )?);
            written.push(write_summary(store, &guard, &summary)?);
        }
        // Update kinds are user-edited while the job runs, so they merge by id
        // instead of replacing the whole file.
        ArtifactContract::Questions => {
            let (path, lost) = commit_merged::<crate::ai::ErQuestions>(
                staging,
                "questions.json",
                expected_hash.as_deref(),
                scope.clone(),
                store,
                &guard,
            )?;
            written.push(path);
            dropped.extend(lost);
        }
        ArtifactContract::Notes => {
            let (path, lost) = commit_merged::<crate::ai::ErNotes>(
                staging,
                "notes.json",
                expected_hash.as_deref(),
                scope.clone(),
                store,
                &guard,
            )?;
            written.push(path);
            dropped.extend(lost);
        }
        ArtifactContract::ValidateComments => {
            let (path, lost) = commit_merged::<crate::ai::ErGithubComments>(
                staging,
                "github-comments.json",
                expected_hash.as_deref(),
                scope.clone(),
                store,
                &guard,
            )?;
            written.push(path);
            dropped.extend(lost);
            // The validate pass appends its counts to the summary when one was
            // staged; the summary is regenerated output, not a patch target.
            if std::path::Path::new(staging).join("summary.md").is_file() {
                let summary = read_text(staging, "summary.md")?;
                written.push(write_summary(store, &guard, &summary)?);
            }
        }
    }
    Ok(CommitReport { written, dropped })
}

/// Commit a user-edited kind by merging the agent's rewrite into the stored
/// copy, one id at a time. The base is the launch-time copy kept in staging.
fn commit_merged<T: crate::app::job_merge::MergeById<Slot = ()>>(
    staging: &str,
    name: &str,
    expected_hash: Option<&str>,
    scope: Scope,
    store: &ArtifactStore,
    guard: &er_store::ScopeGuard,
) -> Result<(String, Vec<String>)> {
    let theirs = read_checked::<T>(
        staging,
        name,
        expected_hash,
        |v| v.schema_version(),
        |v| v.diff_hash().unwrap_or_default(),
    )?;
    let base = read_optional::<T>(&base_path(staging, name))?;
    let key = ArtifactKey::<T>::new(scope, ()).map_err(|error| anyhow::anyhow!("{error}"))?;
    let current = match store
        .read(&key)
        .map_err(|error| anyhow::anyhow!("{error}"))?
    {
        er_store::Loaded::Found(stored) => Some(stored.value),
        er_store::Loaded::Absent => None,
        er_store::Loaded::Unreadable(reason) => {
            anyhow::bail!("the stored {name} is unreadable ({reason:?}); refusing to replace it")
        }
    };
    let (merged, dropped) =
        crate::app::job_merge::merge_by_id(base.as_ref(), &theirs, current.as_ref());
    store
        .write_locked(guard, &key, &merged)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok((store.path(&key).to_string_lossy().into_owned(), dropped))
}

/// Where the launch-time copy of an update-kind file lives in staging.
fn base_path(staging: &str, name: &str) -> std::path::PathBuf {
    std::path::Path::new(staging).join(".base").join(name)
}

fn read_optional<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<Option<T>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    serde_json::from_str(&text)
        .with_context(|| format!("parse the launch copy {}", path.display()))
        .map(Some)
}

/// Validate and commit a scoped run's overlay. The caller stamps it afterwards.
pub fn commit_staged_scoped(
    command_name: &str,
    staging: &str,
    er_dir: &str,
    store: &ArtifactStore,
) -> Result<Vec<String>> {
    let Some(overlay) = crate::ai::scoped_overlay_path(command_name) else {
        return Ok(Vec::new());
    };
    commit_staged_overlay(command_name, &overlay, staging, er_dir, store)
        .map_err(|error| commit_error(&format!("{command_name} scoped overlay"), error))
}

fn commit_staged_overlay(
    command_name: &str,
    overlay: &str,
    staging: &str,
    er_dir: &str,
    store: &ArtifactStore,
) -> Result<Vec<String>> {
    let text = read_text(staging, overlay)?;
    let version = serde_json::from_str::<serde_json::Value>(&text)
        .context("invalid scoped overlay JSON")?
        .get("version")
        .and_then(serde_json::Value::as_u64);
    if version != Some(1) {
        anyhow::bail!("scoped overlay has version {version:?}; this build accepts 1");
    }
    let diff_hash = staged_diff_hash(staging)?;
    let scope = scope_for(er_dir)?;
    let guard = store
        .lock_scope(&scope, DEFAULT_LOCK_TIMEOUT)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let written = match command_name {
        "review" => {
            let value: crate::ai::ScopedReview =
                serde_json::from_str(&text).context("invalid scoped review overlay")?;
            check_scoped_hash(&value, &diff_hash, "review")?;
            write_artifact(store, &guard, scope, (), &value)?
        }
        "professor" => {
            let value: crate::ai::ScopedProfessorReview =
                serde_json::from_str(&text).context("invalid scoped professor overlay")?;
            check_scoped_hash(&value, &diff_hash, "professor")?;
            write_artifact(store, &guard, scope, (), &value)?
        }
        _ => {
            let id = command_name
                .strip_prefix("expert-")
                .context("scoped overlay for an unknown command")?;
            let value: crate::ai::ScopedExpertReview =
                serde_json::from_str(&text).context("invalid scoped expert overlay")?;
            check_scoped_hash(&value, &diff_hash, "expert")?;
            let slot = er_store::ExpertId::new(id).map_err(|error| anyhow::anyhow!("{error}"))?;
            write_artifact(store, &guard, scope, slot, &value)?
        }
    };
    Ok(vec![written])
}

/// Persist a validated diagram through the store, returning its path.
pub fn commit_diagram(
    diagram: &crate::ai::ErDiagram,
    id: &str,
    er_dir: &str,
    store: &ArtifactStore,
) -> Result<String> {
    let scope = scope_for(er_dir)?;
    let guard = store
        .lock_scope(&scope, DEFAULT_LOCK_TIMEOUT)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let slot = er_store::DiagramId::new(id).map_err(|error| anyhow::anyhow!("{error}"))?;
    write_artifact(store, &guard, scope, slot, diagram)
}

fn scope_for(er_dir: &str) -> Result<Scope> {
    Scope::dir(er_dir).map_err(|error| anyhow::anyhow!("{error}"))
}

fn staged_diff_hash(staging: &str) -> Result<String> {
    let path = std::path::Path::new(staging).join("diff-tmp");
    let diff = std::fs::read_to_string(&path)
        .with_context(|| format!("read staged diff {}", path.display()))?;
    Ok(crate::ai::compute_diff_hash(&diff))
}

fn check_scoped_hash<T: Artifact>(value: &T, diff_hash: &str, label: &str) -> Result<()> {
    let found = value
        .diff_hash()
        .with_context(|| format!("scoped {label} overlay has no diff_hash"))?;
    crate::sidecar_upload::check_diff_hash(&format!("scoped {label} overlay"), found, diff_hash)
}

fn read_text(dir: &str, name: &str) -> Result<String> {
    let path = std::path::Path::new(dir).join(name);
    std::fs::read_to_string(&path).with_context(|| format!("read staged output {}", path.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(dir: &str, name: &str) -> Result<T> {
    let text = read_text(dir, name)?;
    serde_json::from_str(&text).with_context(|| format!("parse staged output {name}"))
}

/// Parse a staged artifact and apply the same version and diff-hash checks the
/// upload path applies to agent output.
fn read_checked<T: Artifact>(
    dir: &str,
    name: &str,
    expected_hash: Option<&str>,
    version: impl FnOnce(&T) -> u32,
    hash: impl FnOnce(&T) -> &str,
) -> Result<T> {
    let value: T = read_json(dir, name)?;
    crate::sidecar_upload::check_version(name, version(&value), T::VERSION)?;
    let expected = expected_hash.with_context(|| format!("{name} has no diff hash to check"))?;
    crate::sidecar_upload::check_diff_hash(name, hash(&value), expected)?;
    Ok(value)
}

fn read_non_empty(dir: &str, name: &str) -> Result<String> {
    let text = read_text(dir, name)?;
    if text.trim().is_empty() {
        anyhow::bail!("agent wrote an empty {name}");
    }
    Ok(text)
}

fn write_artifact<T: Artifact>(
    store: &ArtifactStore,
    guard: &er_store::ScopeGuard,
    scope: Scope,
    slot: T::Slot,
    value: &T,
) -> Result<String> {
    let key = ArtifactKey::<T>::new(scope, slot).map_err(|error| anyhow::anyhow!("{error}"))?;
    store
        .write_locked(guard, &key, value)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(store.path(&key).to_string_lossy().into_owned())
}

fn write_summary(
    store: &ArtifactStore,
    guard: &er_store::ScopeGuard,
    summary: &str,
) -> Result<String> {
    let path = er_store::RelPath::new("summary.md").map_err(|error| anyhow::anyhow!("{error}"))?;
    store
        .write_text_locked(guard, &path, summary)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(guard
        .scope_dir()
        .join(path.as_path())
        .to_string_lossy()
        .into_owned())
}
