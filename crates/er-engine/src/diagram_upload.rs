//! Prepare/list/upload PR Mermaid diagrams for MCP clients.
//!
//! Parallel to [`crate::sidecar_upload`] (triage/review/tour), but diagrams
//! don't fit [`crate::sidecar_upload::SidecarKind`]'s fixed-filename-per-kind
//! model: presets overwrite in place (`<kind>.json`) while `custom` diagrams
//! accumulate under timestamped ids, so a bucket can hold any number of
//! `diagrams/*.json` files. This module owns that variable-id upload path;
//! [`crate::ai::diagrams`] owns the sidecar shape and validation itself.
//!
//! Flow: [`prepare_diagram_kit`] writes shared `diff-tmp` + a prompt for one
//! kind; the MCP client (the reviewing agent itself — no subprocess spawn)
//! authors the diagram JSON; [`upload_diagram`] validates and stores it.

use crate::ai::diagrams::{
    diagram_output_file, diagram_sidecar_path, is_safe_diagram_id, is_valid_diagram_kind,
    persist_diagram_json, DIAGRAM_KIND_CUSTOM,
};
use crate::ai::prompts::build_diagram_prompt_mcp;
use crate::ai::{compute_diff_hash, load_diagrams, prepared_diff::diff_tmp_hash, ErDiagram};
use crate::github::owner_repo_storage_slug;
use crate::sidecar_upload::prepare_pr_diff_tmp;
use crate::storage::resolve_managed_root_for_pr_bucket;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Bucket + diff + prompt for one diagram kind (no agent spawn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedDiagramKit {
    pub owner: String,
    pub repo: String,
    pub pr: u64,
    pub er_dir: String,
    pub diff_tmp_path: String,
    /// SHA-256 hex of `diff-tmp` — embed this as `diff_hash` in the diagram JSON.
    pub diff_hash: String,
    pub kind: String,
    /// Filename to upload as (`diagrams/<output_file>`). Presets are stable
    /// (`<kind>.json`); `custom` is a fresh timestamped id each prepare call.
    pub output_file: String,
    pub prompt: String,
    pub instructions: String,
}

/// Fetch the PR diff into managed storage and return a prompt for one diagram kind.
pub fn prepare_diagram_kit(
    owner: &str,
    repo: &str,
    pr: u64,
    kind: &str,
    custom_prompt: Option<&str>,
    ignore_globs: &[String],
) -> Result<PreparedDiagramKit> {
    if !is_valid_diagram_kind(kind) {
        bail!("unknown diagram kind '{kind}'; expected mental-model|subsystems|flows|custom");
    }
    let custom_prompt = custom_prompt.map(str::trim).filter(|s| !s.is_empty());
    if kind == DIAGRAM_KIND_CUSTOM && custom_prompt.is_none() {
        bail!("kind=custom requires a non-empty prompt");
    }

    let (er_dir, diff_tmp_path) = prepare_pr_diff_tmp(owner, repo, pr, ignore_globs)?;
    let diff = std::fs::read_to_string(&diff_tmp_path)
        .with_context(|| format!("read prepared diff {diff_tmp_path}"))?;
    let diff_hash = compute_diff_hash(&diff);
    let output_file = diagram_output_file(kind);

    let prompt = build_diagram_prompt_mcp("PR diff", &er_dir, &diff_hash, kind, custom_prompt);

    Ok(PreparedDiagramKit {
        owner: owner.to_string(),
        repo: repo.to_string(),
        pr,
        er_dir,
        diff_tmp_path,
        diff_hash: diff_hash.clone(),
        kind: kind.to_string(),
        output_file: output_file.clone(),
        prompt,
        instructions: format!(
            "Read the prepared diff at diff_tmp_path (do not re-fetch). Embed diff_hash={diff_hash} \
             in the diagram JSON, then call pr_diagram with action=upload, the same kind, and \
             files={{\"{output_file}\": \"<diagram JSON>\"}}."
        ),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadDiagramResult {
    pub er_dir: String,
    pub id: String,
    pub kind: String,
    pub diff_hash: String,
}

/// Filename (minus `.json`) must match the kind's naming convention: presets
/// upload as exactly `<kind>.json`; `custom` uploads accumulate under
/// `custom-*` ids so they never clobber a preset or each other.
fn validate_output_id(kind: &str, id: &str) -> Result<()> {
    if !is_safe_diagram_id(id) {
        bail!("invalid diagram id '{id}'");
    }
    if kind == DIAGRAM_KIND_CUSTOM {
        if !id.starts_with("custom-") {
            bail!("custom diagrams must upload as 'custom-<id>.json' (got '{id}.json')");
        }
    } else if id != kind {
        bail!("{kind} diagram must upload as '{kind}.json' (got '{id}.json')");
    }
    Ok(())
}

fn resolve_er_dir(owner: &str, repo: &str, pr: u64) -> String {
    let slug = owner_repo_storage_slug(owner, repo);
    resolve_managed_root_for_pr_bucket(&slug, pr).er_dir()
}

/// Resolve the PR bucket (optionally refresh `diff-tmp`), validate the
/// diagram JSON against the current diff hash, and atomically write it.
#[allow(clippy::too_many_arguments)]
pub fn upload_diagram(
    owner: &str,
    repo: &str,
    pr: u64,
    kind: &str,
    file_name: &str,
    content: &str,
    custom_prompt: Option<&str>,
    refresh_diff: bool,
) -> Result<UploadDiagramResult> {
    if !is_valid_diagram_kind(kind) {
        bail!("unknown diagram kind '{kind}'; expected mental-model|subsystems|flows|custom");
    }
    let id = file_name.trim().strip_suffix(".json").ok_or_else(|| {
        anyhow::anyhow!("diagram file must be named '<id>.json' (got '{file_name}')")
    })?;
    validate_output_id(kind, id)?;

    let er_dir = if refresh_diff {
        prepare_pr_diff_tmp(owner, repo, pr, &[])?.0
    } else {
        let er_dir = resolve_er_dir(owner, repo, pr);
        let diff_path = Path::new(&er_dir).join("diff-tmp");
        if !diff_path.exists() {
            prepare_pr_diff_tmp(owner, repo, pr, &[])?.0
        } else {
            er_dir
        }
    };
    let diff_path = Path::new(&er_dir).join("diff-tmp");
    let diff = std::fs::read_to_string(&diff_path)
        .with_context(|| format!("read {}", diff_path.display()))?;
    let expected_hash = compute_diff_hash(&diff);

    let output_path = diagram_sidecar_path(&er_dir, file_name)
        .ok_or_else(|| anyhow::anyhow!("invalid diagram output file: {file_name}"))?;

    persist_diagram_json(content, kind, &expected_hash, custom_prompt, &output_path)
        .with_context(|| format!("diagram upload rejected (expected diff_hash={expected_hash})"))?;

    Ok(UploadDiagramResult {
        er_dir,
        id: id.to_string(),
        kind: kind.to_string(),
        diff_hash: expected_hash,
    })
}

/// List existing `diagrams/*.json` for a PR bucket, marking staleness against
/// the last prepared `diff-tmp` (or unconditionally stale if none exists yet).
pub fn list_diagrams(owner: &str, repo: &str, pr: u64) -> (String, Vec<ErDiagram>) {
    let er_dir = resolve_er_dir(owner, repo, pr);
    let current_hash = diff_tmp_hash(&er_dir).unwrap_or_default();
    let diagrams = load_diagrams(&er_dir, &current_hash);
    (er_dir, diagrams)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::STORAGE_TEST_ENV_LOCK;

    fn with_storage_root<T>(f: impl FnOnce() -> T) -> T {
        let _guard = STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        std::env::set_var("ER_STORAGE_ROOT", root.path());
        let out = f();
        std::env::remove_var("ER_STORAGE_ROOT");
        out
    }

    fn write_diff_tmp(er_dir: &str, diff: &str) -> String {
        std::fs::create_dir_all(er_dir).unwrap();
        std::fs::write(format!("{er_dir}/diff-tmp"), diff).unwrap();
        let hash = compute_diff_hash(diff);
        std::fs::write(format!("{er_dir}/.diff-tmp.sha256"), &hash).unwrap();
        hash
    }

    #[test]
    fn validate_output_id_enforces_naming_convention() {
        assert!(validate_output_id("flows", "flows").is_ok());
        assert!(validate_output_id("flows", "mental-model").is_err());
        assert!(validate_output_id("custom", "custom-1700000000000").is_ok());
        assert!(validate_output_id("custom", "flows").is_err());
        assert!(validate_output_id("flows", "../escape").is_err());
    }

    #[test]
    fn prepare_rejects_custom_without_prompt() {
        with_storage_root(|| {
            let err = prepare_diagram_kit("acme", "widgets", 1, "custom", None, &[]).unwrap_err();
            assert!(err.to_string().contains("requires a non-empty prompt"));
        });
    }

    #[test]
    fn upload_writes_preset_diagram_and_pins_hash() {
        with_storage_root(|| {
            let er_dir = resolve_er_dir("acme", "widgets", 5);
            let diff = "diff --git a/x b/x\n+hello\n";
            let hash = write_diff_tmp(&er_dir, diff);

            let body = r#"{"version":1,"diff_hash":"stale","kind":"subsystems","title":"T","prompt":"","mermaid":"flowchart TD\n  A-->B"}"#;
            let result = upload_diagram(
                "acme",
                "widgets",
                5,
                "flows",
                "flows.json",
                body,
                None,
                false,
            )
            .unwrap();
            assert_eq!(result.id, "flows");
            assert_eq!(result.kind, "flows");
            assert_eq!(result.diff_hash, hash);

            let (_, diagrams) = list_diagrams("acme", "widgets", 5);
            assert_eq!(diagrams.len(), 1);
            assert_eq!(diagrams[0].kind, "flows");
            assert!(!diagrams[0].stale);
        });
    }

    #[test]
    fn upload_rejects_wrong_filename_for_kind() {
        with_storage_root(|| {
            let er_dir = resolve_er_dir("acme", "widgets", 6);
            write_diff_tmp(&er_dir, "diff --git a/x b/x\n+hi\n");
            let err = upload_diagram(
                "acme",
                "widgets",
                6,
                "flows",
                "mental-model.json",
                "{}",
                None,
                false,
            )
            .unwrap_err();
            assert!(err.to_string().contains("must upload as 'flows.json'"));
        });
    }

    #[test]
    fn upload_custom_pins_prompt_and_accumulates() {
        with_storage_root(|| {
            let er_dir = resolve_er_dir("acme", "widgets", 7);
            let hash = write_diff_tmp(&er_dir, "diff --git a/x b/x\n+hi\n");
            let body = format!(
                r#"{{"version":1,"diff_hash":"{hash}","kind":"custom","title":"T","prompt":"ignored","mermaid":"flowchart TD\n  A-->B"}}"#
            );
            upload_diagram(
                "acme",
                "widgets",
                7,
                "custom",
                "custom-1.json",
                &body,
                Some("what the user asked"),
                false,
            )
            .unwrap();
            upload_diagram(
                "acme",
                "widgets",
                7,
                "custom",
                "custom-2.json",
                &body,
                Some("a different ask"),
                false,
            )
            .unwrap();

            let (_, diagrams) = list_diagrams("acme", "widgets", 7);
            assert_eq!(diagrams.len(), 2);
            assert!(diagrams.iter().all(|d| d.kind == "custom"));
            let prompts: Vec<&str> = diagrams.iter().map(|d| d.prompt.as_str()).collect();
            assert!(prompts.contains(&"what the user asked"));
            assert!(prompts.contains(&"a different ask"));
        });
    }

    #[test]
    fn list_diagrams_on_empty_bucket_is_empty() {
        with_storage_root(|| {
            let (_, diagrams) = list_diagrams("acme", "widgets", 99);
            assert!(diagrams.is_empty());
        });
    }
}
