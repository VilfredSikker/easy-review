//! JSON schemas + prepared-diff prompts for MCP client agents.
//!
//! Use [`artifact_specs`] before authoring sidecars so the LLM follows the
//! exact shapes `upload_artifacts` / Desktop expect.

use crate::ai::prompts::{
    build_review_prompt_prepared_diff, build_tour_prompt_prepared_diff,
    build_triage_review_prompt_prepared_diff,
};
use crate::sidecar_upload::SidecarKind;
use serde::Serialize;
use serde_json::{json, Value};

/// Placeholder substituted for the managed PR bucket in standalone specs.
pub const OUTPUT_DIR_PLACEHOLDER: &str = "<OUTPUT_DIR>";

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

fn triage_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "triage.json",
        "type": "object",
        "required": ["version", "diff_hash"],
        "additionalProperties": true,
        "properties": {
            "version": { "type": "integer", "const": 1 },
            "diff_hash": {
                "type": "string",
                "description": "SHA-256 hex of the prepared diff-tmp bytes"
            },
            "diff_scope": { "type": "string" },
            "created_at": { "type": "string", "description": "ISO 8601" },
            "first_impression": { "type": "string" },
            "diff_stats": {
                "type": "object",
                "properties": {
                    "files_changed": { "type": "integer" },
                    "approx_risk": { "type": "string", "enum": ["low", "medium", "high"] },
                    "domains": { "type": "array", "items": { "type": "string" } }
                }
            },
            "verdict": {
                "type": "object",
                "properties": {
                    "primary": {
                        "type": "string",
                        "enum": ["general", "expert", "arena", "professor", "skip"]
                    },
                    "experts": { "type": "array", "items": { "type": "string" } },
                    "rationale": { "type": "string" },
                    "confidence": { "type": "string", "enum": ["high", "medium", "low"] }
                }
            },
            "priority_files": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": { "type": "string" },
                        "reason": { "type": "string" },
                        "risk": { "type": "string" }
                    }
                }
            }
        }
    })
}

fn finding_schema() -> Value {
    json!({
        "type": "object",
        "required": ["id", "severity", "title"],
        "additionalProperties": true,
        "properties": {
            "id": { "type": "string" },
            "severity": { "type": "string", "enum": ["high", "medium", "low", "info"] },
            "category": { "type": "string" },
            "title": { "type": "string", "maxLength": 60 },
            "description": { "type": "string" },
            "hunk_index": { "type": ["integer", "null"], "minimum": 0 },
            "line_start": { "type": ["integer", "null"], "minimum": 0 },
            "line_end": { "type": ["integer", "null"], "minimum": 0 },
            "suggestion": { "type": "string" },
            "related_files": { "type": "array", "items": { "type": "string" } },
            "outside_diff": { "type": "boolean" },
            "confidence": {
                "type": "string",
                "enum": ["tentative", "confirmed", "informational", "dropped"]
            },
            "verification_plan": { "type": "string" },
            "evidence": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["file", "note"],
                    "properties": {
                        "file": { "type": "string" },
                        "line_start": { "type": ["integer", "null"] },
                        "line_end": { "type": ["integer", "null"] },
                        "note": { "type": "string" }
                    }
                }
            },
            "resolved": { "type": "boolean", "description": "Leave false; validate pass sets this" },
            "resolved_note": { "type": "string" },
            "resolved_at": { "type": "string" }
        }
    })
}

fn review_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "review.json",
        "type": "object",
        "required": ["version", "diff_hash"],
        "additionalProperties": true,
        "properties": {
            "version": { "type": "integer", "const": 1 },
            "diff_hash": { "type": "string" },
            "created_at": { "type": "string" },
            "base_branch": { "type": "string" },
            "head_branch": {
                "type": "string",
                "description": "Must match the PR head branch or Desktop may discard the review"
            },
            "file_hashes": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "files": {
                "type": "object",
                "additionalProperties": {
                    "type": "object",
                    "required": ["risk"],
                    "properties": {
                        "risk": { "type": "string", "enum": ["high", "medium", "low", "info"] },
                        "risk_reason": { "type": "string" },
                        "summary": { "type": "string" },
                        "findings": { "type": "array", "items": finding_schema() }
                    }
                }
            }
        }
    })
}

fn order_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "order.json",
        "type": "object",
        "required": ["version", "diff_hash"],
        "properties": {
            "version": { "type": "integer", "const": 1 },
            "diff_hash": { "type": "string" },
            "order": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": { "type": "string" },
                        "reason": { "type": "string" },
                        "group": { "type": "string" }
                    }
                }
            },
            "groups": {
                "type": "object",
                "additionalProperties": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "color": { "type": "string" }
                    }
                }
            }
        }
    })
}

fn checklist_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "checklist.json",
        "type": "object",
        "required": ["version", "diff_hash"],
        "properties": {
            "version": { "type": "integer", "const": 1 },
            "diff_hash": { "type": "string" },
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "text"],
                    "properties": {
                        "id": { "type": "string" },
                        "text": { "type": "string" },
                        "category": { "type": "string" },
                        "checked": { "type": "boolean" },
                        "related_findings": { "type": "array", "items": { "type": "string" } },
                        "related_files": { "type": "array", "items": { "type": "string" } }
                    }
                }
            }
        }
    })
}

fn summary_md_schema() -> Value {
    json!({
        "title": "summary.md",
        "type": "string",
        "description": "Non-empty markdown (3–5 paragraphs) summarizing the overall changes. Not JSON."
    })
}

fn tour_schema() -> Value {
    // Parsed from a string to avoid json! macro recursion limits on deep nests.
    serde_json::from_str(
        r#"{
      "$schema": "http://json-schema.org/draft-07/schema#",
      "title": "tour.json",
      "type": "object",
      "required": ["version", "diff_hash"],
      "additionalProperties": true,
      "properties": {
        "version": { "type": "integer", "const": 1 },
        "diff_hash": { "type": "string" },
        "created_at": { "type": "string" },
        "title": { "type": "string" },
        "overview": { "type": "string" },
        "pillars": {
          "type": "array",
          "items": {
            "type": "object",
            "required": ["id", "title"],
            "properties": {
              "id": { "type": "string" },
              "title": { "type": "string" },
              "description": { "type": "string" },
              "order": { "type": "integer", "minimum": 0 },
              "importance": { "type": "integer", "minimum": 0, "maximum": 100 },
              "foundation": { "type": "boolean" },
              "files": {
                "type": "array",
                "items": {
                  "type": "object",
                  "required": ["path"],
                  "properties": {
                    "path": { "type": "string" },
                    "reason": { "type": "string" },
                    "finding_ids": { "type": "array", "items": { "type": "string" } },
                    "related": {
                      "type": "array",
                      "items": {
                        "type": "object",
                        "required": ["path"],
                        "properties": {
                          "path": { "type": "string" },
                          "kind": {
                            "type": "string",
                            "enum": ["test", "style", "story", "snapshot", "other"]
                          },
                          "reason": { "type": "string" }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }"#,
    )
    .expect("tour schema JSON")
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
        ]
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
            schemas: json!({ "triage.json": triage_schema() }),
            examples: json!({ "triage.json": triage_example() }),
            prompt: build_triage_review_prompt_prepared_diff("branch", output_dir),
            notes: vec![
                "Write only triage.json — no review/order/checklist/summary.",
                "Replace <DIFF_HASH> with the prepare_review diff_hash.",
            ],
        },
        SidecarKind::Review => ArtifactSpec {
            kind,
            required_files: vec!["review.json", "order.json", "checklist.json", "summary.md"],
            schemas: json!({
                "review.json": review_schema(),
                "order.json": order_schema(),
                "checklist.json": checklist_schema(),
                "summary.md": summary_md_schema(),
            }),
            examples: review_examples(),
            prompt: build_review_prompt_prepared_diff("branch", output_dir, base, head),
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
            schemas: json!({ "tour.json": tour_schema() }),
            examples: json!({ "tour.json": tour_example() }),
            prompt: build_tour_prompt_prepared_diff("PR diff", output_dir, "tour.json"),
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

    #[test]
    fn all_kinds_have_schemas_and_prompts() {
        let resp = artifact_specs(&[SidecarKind::Triage, SidecarKind::Review, SidecarKind::Tour]);
        assert_eq!(resp.specs.len(), 3);
        for spec in &resp.specs {
            assert!(!spec.required_files.is_empty());
            assert!(spec.schemas.is_object());
            assert!(spec.prompt.contains("diff-tmp") || spec.prompt.contains("diff_hash"));
            for file in &spec.required_files {
                assert!(
                    spec.schemas.get(file).is_some(),
                    "missing schema for {file}"
                );
            }
        }
    }

    #[test]
    fn review_requires_four_files() {
        let resp = artifact_specs(&[SidecarKind::Review]);
        assert_eq!(resp.specs[0].required_files.len(), 4);
    }
}
