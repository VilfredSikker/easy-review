//! Golden pins for the findings projection.
//!
//! Cases 1–6 drive `load_ai_state` over a temp bucket. They pin what the front
//! ends show today, so they must pass before and after the projection replaces
//! the in-place merges, with no golden file changed between the two commits.
//! Cases 7–8 call `build_findings_projection` directly; they land with the
//! projection because the function does not exist before it.
#![expect(
    clippy::expect_used,
    reason = "the whole file is test code; clippy exempts #[test] fns but not their helpers"
)]

use std::collections::{HashMap, HashSet};
use std::path::Path;

use er_engine::ai::accepted_findings::AcceptedFinding;
use er_engine::ai::{
    build_findings_projection, finding_key, load_ai_state, AiState, ArbiterEffect, ArbiterReview,
    ArbiterRuling, ErReview, ExpertReview, ProjectionInputs,
};
use serde_json::{json, Value};

/// Write a fixture in the store's envelope shape, with the kind the registry
/// maps the file to.
fn write_sidecar(dir: &Path, rel: &str, content: &str) {
    let kind = match rel {
        "review.json" => "review",
        "professor.json" => "professor",
        "arbiter.json" => "arbiter",
        "accepted-findings.json" => "accepted-findings",
        other if other.starts_with("experts/") => "expert",
        other => panic!("no registered kind for {other}"),
    };
    let payload: Value = serde_json::from_str(content).expect("the payload is JSON");
    let enveloped = json!({
        "kind": kind,
        "version": 1,
        "producer": "findings_projection test",
        "written_at": "2026-01-01T00:00:00Z",
        "payload": payload,
    });
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create sidecar dir");
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&enveloped).expect("the envelope serialises"),
    )
    .expect("write sidecar");
}

fn load(dir: &Path) -> AiState {
    er_engine::storage::with_test_storage_root(dir, || {
        load_ai_state(dir.to_str().expect("utf-8 temp path"), "H", None)
    })
}

fn ids(state: &AiState, file: &str) -> Vec<String> {
    state
        .review
        .as_ref()
        .expect("a projected review")
        .files
        .get(file)
        .expect("file in the review")
        .findings
        .iter()
        .map(|finding| finding.id.clone())
        .collect()
}

/// The compared form. `Finding.providers` is `skip_serializing`, so the derive
/// drops it; reinsert it or a provenance change could not fail a golden.
fn canonical(
    review: Option<&ErReview>,
    effect: &ArbiterEffect,
    graded_keys: &HashSet<String>,
    verdicts: &HashMap<String, (ArbiterRuling, String)>,
) -> Value {
    let mut review_value = match review {
        Some(review) => serde_json::to_value(review).expect("review serialises"),
        None => Value::Null,
    };
    if let Some(review) = review {
        for (path, file) in &review.files {
            for (index, finding) in file.findings.iter().enumerate() {
                review_value["files"][path.as_str()]["findings"][index]["providers"] =
                    json!(finding.providers);
            }
        }
    }
    let mut keys: Vec<&str> = graded_keys.iter().map(String::as_str).collect();
    keys.sort_unstable();
    let mut verdicts: Vec<(&String, &(ArbiterRuling, String))> = verdicts.iter().collect();
    verdicts.sort_unstable_by_key(|(key, _)| key.as_str());
    let verdicts: Vec<_> = verdicts
        .into_iter()
        .map(|(key, (ruling, rationale))| {
            json!({ "key": key, "ruling": ruling, "rationale": rationale })
        })
        .collect();
    json!({
        "review": review_value,
        "arbiter_effect": {
            "dropped": effect.dropped,
            "merged": effect.merged,
            "regraded": effect.regraded,
            "kept": effect.kept,
            "unmatched": effect.unmatched,
        },
        "arbiter_graded_keys": keys,
        "arbiter_verdicts": verdicts,
    })
}

fn assert_golden(case: &str, actual: Value, golden: &str) {
    let expected: Value = serde_json::from_str(golden).expect("golden parses");
    assert_eq!(
        actual,
        expected,
        "{case} differs from its golden\n--- actual ---\n{}\n--- expected ---\n{}",
        serde_json::to_string_pretty(&actual).expect("actual serialises"),
        serde_json::to_string_pretty(&expected).expect("expected serialises"),
    );
}

const GENERAL_REVIEW: &str = r#"{
  "version": 1,
  "diff_hash": "H",
  "created_at": "2026-01-01T00:00:00Z",
  "base_branch": "main",
  "head_branch": "feat/x",
  "files": {
    "a.rs": {
      "risk": "low",
      "risk_reason": "",
      "summary": "",
      "findings": [
        {"id": "f-1", "title": "general finding", "description": "", "severity": "low", "category": "logic", "hunk_index": 0, "line_start": 1}
      ]
    }
  },
  "file_hashes": {}
}"#;

fn case_01_general_only() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(
        dir.path(),
        "review.json",
        r#"{
  "version": 1,
  "diff_hash": "H",
  "created_at": "2026-01-01T00:00:00Z",
  "base_branch": "main",
  "head_branch": "feat/x",
  "files": {
    "a.rs": {
      "risk": "low",
      "risk_reason": "",
      "summary": "",
      "findings": [
        {"id": "sec-1", "title": "t", "description": "", "severity": "low", "category": "logic", "hunk_index": 0, "line_start": 1},
        {"id": "f-2", "title": "t2", "description": "", "severity": "low", "category": "logic", "hunk_index": 0, "line_start": 2}
      ]
    }
  },
  "file_hashes": {}
}"#,
    );

    let state = load(dir.path());
    assert_eq!(ids(&state, "a.rs"), ["sec-1", "f-2"]);
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_general_only() {
    assert_golden(
        "projection_general_only",
        case_01_general_only(),
        include_str!("golden/findings_projection/case_01_general_only.json"),
    );
}

fn case_02_general_plus_expert_per_file() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(dir.path(), "review.json", GENERAL_REVIEW);
    write_sidecar(
        dir.path(),
        "experts/security.json",
        r#"{
  "version": 1,
  "expert_id": "security",
  "diff_hash": "H",
  "created_at": "2026-01-02T00:00:00Z",
  "summary": "",
  "files": {
    "a.rs": {"findings": [{"id": "1", "title": "expert on a", "severity": "medium", "category": "correctness", "hunk_index": 0, "line_start": 1}]},
    "b.rs": {"findings": [{"id": "1", "title": "expert on b", "severity": "medium", "category": "correctness", "hunk_index": 0, "line_start": 2}]}
  }
}"#,
    );
    write_sidecar(
        dir.path(),
        "professor.json",
        r#"{
  "version": 1,
  "diff_hash": "H",
  "created_at": "2026-01-03T00:00:00Z",
  "summary": "",
  "files": {
    "a.rs": {"findings": [{"id": "1", "title": "insight", "description": "", "severity": "info", "hunk_index": 0, "line_start": 3}]}
  }
}"#,
    );

    let state = load(dir.path());
    assert_eq!(ids(&state, "a.rs"), ["f-1", "sec-1", "prof-1"]);
    assert_eq!(ids(&state, "b.rs"), ["sec-1"]);
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_general_plus_expert_per_file() {
    assert_golden(
        "projection_general_plus_expert_per_file",
        case_02_general_plus_expert_per_file(),
        include_str!("golden/findings_projection/case_02_general_plus_expert.json"),
    );
}

fn case_03_experts_only() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(
        dir.path(),
        "experts/security.json",
        r#"{
  "version": 1,
  "expert_id": "security",
  "diff_hash": "H",
  "created_at": "2026-01-02T00:00:00Z",
  "summary": "",
  "files": {
    "a.rs": {"findings": [{"id": "1", "title": "expert", "severity": "medium", "hunk_index": 0, "line_start": 1}]}
  }
}"#,
    );
    write_sidecar(
        dir.path(),
        "experts/patterns.json",
        r#"{
  "version": 1,
  "expert_id": "patterns",
  "diff_hash": "old",
  "created_at": "2020-01-01T00:00:00Z",
  "summary": "",
  "files": {
    "a.rs": {"findings": [{"id": "1", "title": "stale expert", "severity": "low", "hunk_index": 0, "line_start": 1}]}
  }
}"#,
    );

    let state = load(dir.path());
    let review = state.review.as_ref().expect("a synthesized review");
    assert_eq!(review.diff_hash, "H");
    assert_eq!(review.created_at, "2026-01-02T00:00:00Z");
    assert_eq!(ids(&state, "a.rs"), ["sec-1"]);
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_experts_only_without_review() {
    assert_golden(
        "projection_experts_only_without_review",
        case_03_experts_only(),
        include_str!("golden/findings_projection/case_03_experts_only.json"),
    );
}

fn case_04_stale_generation_expert() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(
        dir.path(),
        "review.json",
        r#"{
  "version": 1,
  "diff_hash": "scoped",
  "created_at": "2026-01-01T00:00:00Z",
  "base_branch": "",
  "head_branch": "",
  "files": {
    "a.rs": {
      "risk": "low",
      "risk_reason": "",
      "summary": "",
      "findings": [
        {"id": "f-1", "title": "general", "severity": "low", "hunk_index": 0, "line_start": 1}
      ]
    }
  },
  "file_hashes": {}
}"#,
    );
    write_sidecar(
        dir.path(),
        "experts/reliability.json",
        r#"{
  "version": 1,
  "expert_id": "reliability",
  "diff_hash": "scoped",
  "created_at": "2026-01-02T00:00:00Z",
  "summary": "",
  "files": {
    "a.rs": {"findings": [{"id": "1", "title": "same generation", "severity": "medium", "hunk_index": 0, "line_start": 2}]}
  }
}"#,
    );
    write_sidecar(
        dir.path(),
        "experts/security.json",
        r#"{
  "version": 1,
  "expert_id": "security",
  "diff_hash": "older",
  "created_at": "2020-01-01T00:00:00Z",
  "summary": "",
  "files": {
    "a.rs": {"findings": [{"id": "1", "title": "older generation", "severity": "medium", "hunk_index": 0, "line_start": 3}]}
  }
}"#,
    );

    let state = load(dir.path());
    assert_eq!(ids(&state, "a.rs"), ["f-1", "rel-1"]);
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_keeps_experts_sharing_the_stale_review_hash() {
    assert_golden(
        "projection_keeps_experts_sharing_the_stale_review_hash",
        case_04_stale_generation_expert(),
        include_str!("golden/findings_projection/case_04_stale_generation_expert.json"),
    );
}

const FRESH_PROFESSOR: &str = r#"{
  "version": 1,
  "diff_hash": "H",
  "created_at": "2026-01-03T00:00:00Z",
  "summary": "",
  "files": {
    "a.rs": {"findings": [{"id": "1", "title": "insight", "description": "", "severity": "info", "hunk_index": 0, "line_start": 1}]}
  }
}"#;

fn case_05a_professor_fresh() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(dir.path(), "professor.json", FRESH_PROFESSOR);

    let state = load(dir.path());
    let review = state.review.as_ref().expect("a professor shell");
    assert_eq!(review.diff_hash, "H");
    assert_eq!(review.created_at, "2026-01-03T00:00:00Z");
    assert_eq!(ids(&state, "a.rs"), ["prof-1"]);
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_professor_fresh() {
    assert_golden(
        "projection_professor_fresh",
        case_05a_professor_fresh(),
        include_str!("golden/findings_projection/case_05a_professor_fresh.json"),
    );
}

fn case_05b_professor_stale() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(
        dir.path(),
        "professor.json",
        r#"{
  "version": 1,
  "diff_hash": "old",
  "created_at": "2020-01-01T00:00:00Z",
  "summary": "",
  "files": {
    "a.rs": {"findings": [{"id": "1", "title": "insight", "description": "", "severity": "info", "hunk_index": 0, "line_start": 1}]}
  }
}"#,
    );

    let state = load(dir.path());
    assert!(state.review.is_none(), "a stale professor builds no review");
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_professor_stale() {
    assert_golden(
        "projection_professor_stale",
        case_05b_professor_stale(),
        include_str!("golden/findings_projection/case_05b_professor_stale.json"),
    );
}

fn case_06_arbiter_overlay() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(
        dir.path(),
        "review.json",
        r#"{
  "version": 1,
  "diff_hash": "H",
  "created_at": "2026-01-01T00:00:00Z",
  "base_branch": "",
  "head_branch": "",
  "files": {
    "a.rs": {
      "risk": "low",
      "risk_reason": "",
      "summary": "",
      "findings": [
        {"id": "a-1", "title": "Alpha", "severity": "high", "hunk_index": 0, "line_start": 1},
        {"id": "b-1", "title": "Beta", "severity": "high", "hunk_index": 0, "line_start": 2},
        {"id": "c-1", "title": "Gamma", "severity": "high", "hunk_index": 0, "line_start": 3, "confidence": "tentative"},
        {"id": "d-1", "title": "Delta", "severity": "high", "hunk_index": 0, "line_start": 4}
      ]
    }
  },
  "file_hashes": {}
}"#,
    );
    let verdicts = json!([
        {"id": finding_key("a.rs", Some(1), "Alpha"), "file": "a.rs", "verdict": "dropped", "rationale": "not a defect"},
        {"id": finding_key("a.rs", Some(2), "Beta"), "file": "a.rs", "verdict": "merged", "merged_into": "Alpha", "rationale": "duplicate of Alpha"},
        {"id": finding_key("a.rs", Some(3), "Gamma"), "file": "a.rs", "verdict": "kept", "confidence": "confirmed", "rationale": "checked by hand"},
        {"id": finding_key("a.rs", Some(4), "Delta"), "file": "a.rs", "verdict": "escalated", "rationale": "needs the owner"},
        {"id": finding_key("a.rs", Some(99), "Epsilon"), "file": "a.rs", "verdict": "kept", "rationale": "orphan"}
    ]);
    let arbiter = json!({
        "version": 1,
        "diff_hash": "H",
        "created_at": "",
        "verdicts": verdicts,
    });
    write_sidecar(
        dir.path(),
        "arbiter.json",
        &serde_json::to_string(&arbiter).expect("arbiter serialises"),
    );

    let state = load(dir.path());
    assert_eq!(ids(&state, "a.rs"), ["a-1", "b-1", "c-1", "d-1"]);
    let effect = state.arbiter_effect;
    assert_eq!(
        (
            effect.dropped,
            effect.merged,
            effect.regraded,
            effect.kept,
            effect.unmatched
        ),
        (1, 1, 1, 2, 1)
    );
    let review = state.review.as_ref().expect("a review");
    let gamma = &review.files["a.rs"].findings[2];
    assert_eq!(gamma.confidence, er_engine::ai::Confidence::Confirmed);
    assert!(
        gamma.responses[0].id.starts_with("arbiter-"),
        "{:?}",
        gamma.responses
    );
    let delta_key = finding_key("a.rs", Some(4), "Delta");
    assert_eq!(
        state.arbiter_verdicts[&delta_key].0,
        ArbiterRuling::Escalated
    );
    assert_eq!(state.arbiter_verdicts[&delta_key].1, "needs the owner");
    assert_eq!(state.arbiter_graded_keys.len(), 4);
    assert_eq!(state.arbiter_verdicts.len(), 4);
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_arbiter_overlay() {
    assert_golden(
        "projection_arbiter_overlay",
        case_06_arbiter_overlay(),
        include_str!("golden/findings_projection/case_06_arbiter_overlay.json"),
    );
}

fn case_07_expert_order() -> Value {
    let review: ErReview = serde_json::from_value(json!({
        "version": 1,
        "diff_hash": "H",
        "files": {},
        "file_hashes": {}
    }))
    .expect("an empty general review");
    let experts: Vec<ExpertReview> = vec![
        serde_json::from_value(json!({
            "version": 1,
            "expert_id": "security",
            "diff_hash": "H",
            "files": {"x.rs": {"findings": [
                {"id": "1", "title": "security claim", "severity": "medium", "hunk_index": 0, "line_start": 1}
            ]}}
        }))
        .expect("a security expert"),
        serde_json::from_value(json!({
            "version": 1,
            "expert_id": "api",
            "diff_hash": "H",
            "files": {"x.rs": {"findings": [
                {"id": "1", "title": "api claim", "severity": "medium", "hunk_index": 0, "line_start": 2}
            ]}}
        }))
        .expect("an api expert"),
    ];

    let projection = build_findings_projection(ProjectionInputs {
        general: Some(review),
        accepted: &[],
        experts: &experts,
        professor: None,
        arbiter: None,
        validation: None,
        current_diff_hash: "H",
    });
    let review = projection.review.as_ref().expect("a projected review");
    let ids: Vec<&str> = review.files["x.rs"]
        .findings
        .iter()
        .map(|finding| finding.id.as_str())
        .collect();
    assert_eq!(ids, ["api-1", "sec-1"]);
    canonical(
        projection.review.as_ref(),
        &projection.arbiter_effect,
        &projection.arbiter_graded_keys,
        &projection.arbiter_verdicts,
    )
}

#[test]
fn projection_orders_experts_by_id_not_by_directory_order() {
    assert_golden(
        "projection_orders_experts_by_id_not_by_directory_order",
        case_07_expert_order(),
        include_str!("golden/findings_projection/case_07_expert_order.json"),
    );
}

fn case_08_provenance_before_the_arbiter() -> Value {
    let review: ErReview = serde_json::from_value(json!({
        "version": 1,
        "diff_hash": "H",
        "files": {
            "b.rs": {"risk": "low", "findings": [
                {"id": "arena-run-1-b", "title": "B", "severity": "low", "hunk_index": 0, "line_start": 2}
            ]}
        }
    }))
    .expect("a general review with one row");
    let accepted: AcceptedFinding = serde_json::from_value(json!({
        "file": "a.rs",
        "run_finding_id": "a",
        "run_id": "run-1",
        "run_diff_hash": "H",
        "accepted_at": "2026-01-01T00:00:00Z",
        "providers": ["host-a"],
        "finding": {
            "id": "arena-run-1-a", "title": "A", "severity": "low",
            "hunk_index": 0, "line_start": 1, "lens": "security", "raised_by": ["security"]
        }
    }))
    .expect("an accepted row");
    let arbiter: ArbiterReview = serde_json::from_value(json!({
        "version": 1,
        "diff_hash": "H",
        "verdicts": [{
            "id": finding_key("b.rs", Some(2), "B"),
            "file": "b.rs",
            "verdict": "kept",
            "rationale": "kept",
            "providers": ["arbiter-b"]
        }]
    }))
    .expect("an arbiter overlay");

    let projection = build_findings_projection(ProjectionInputs {
        general: Some(review),
        accepted: std::slice::from_ref(&accepted),
        experts: &[],
        professor: None,
        arbiter: Some(&arbiter),
        validation: None,
        current_diff_hash: "H",
    });
    let review = projection.review.as_ref().expect("a projected review");
    let a = &review.files["a.rs"].findings[0];
    assert_eq!(a.providers, ["host-a"]);
    assert_eq!(a.lens, "security");
    let b = &review.files["b.rs"].findings[0];
    assert_eq!(b.providers, ["arbiter-b"]);
    canonical(
        projection.review.as_ref(),
        &projection.arbiter_effect,
        &projection.arbiter_graded_keys,
        &projection.arbiter_verdicts,
    )
}

#[test]
fn projection_provenance_runs_before_the_arbiter() {
    assert_golden(
        "projection_provenance_runs_before_the_arbiter",
        case_08_provenance_before_the_arbiter(),
        include_str!("golden/findings_projection/case_08_provenance_before_arbiter.json"),
    );
}

/// Accepted rows keep the place the import left them: after the general
/// review's own rows and before the expert rows, with the artifact's stored
/// providers restored after the provenance clear.
fn case_09_accepted_rows_keep_their_place() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(
        dir.path(),
        "review.json",
        r#"{
  "version": 1,
  "diff_hash": "H",
  "created_at": "2026-01-01T00:00:00Z",
  "files": {
    "a.rs": {"risk": "low", "findings": [{"id": "general-1", "title": "General", "severity": "low", "hunk_index": 0, "line_start": 1}]},
    "c.rs": {"risk": "low", "findings": [{"id": "general-c", "title": "General C", "severity": "low", "hunk_index": 0, "line_start": 7}]}
  }
}"#,
    );
    write_sidecar(
        dir.path(),
        "experts/security.json",
        r#"{
  "version": 1,
  "expert_id": "security",
  "diff_hash": "H",
  "files": {"a.rs": {"findings": [{"id": "1", "title": "Expert A", "severity": "medium", "hunk_index": 0, "line_start": 2}]}}
}"#,
    );
    write_sidecar(
        dir.path(),
        "accepted-findings.json",
        r#"{
  "version": 1,
  "findings": [
    {"file": "a.rs", "run_finding_id": "kept-a", "run_id": "run-x", "run_diff_hash": "H",
     "accepted_at": "2026-01-02T00:00:00Z", "providers": ["deep-provider"],
     "finding": {"id": "arena-run-x-kept-a", "title": "Accepted A", "severity": "high", "hunk_index": 0, "line_start": 5, "lens": "general", "raised_by": ["general"]}},
    {"file": "b.rs", "run_finding_id": "kept-b", "run_id": "run-x", "run_diff_hash": "H",
     "accepted_at": "2026-01-02T00:00:00Z", "providers": ["second-provider"],
     "finding": {"id": "arena-run-x-kept-b", "title": "Accepted B", "severity": "medium", "line_start": 3, "lens": "security", "raised_by": ["security"]}}
  ]
}"#,
    );

    let state = load(dir.path());
    assert_eq!(
        ids(&state, "a.rs"),
        ["general-1", "arena-run-x-kept-a", "sec-1"]
    );
    assert_eq!(ids(&state, "b.rs"), ["arena-run-x-kept-b"]);
    assert_eq!(ids(&state, "c.rs"), ["general-c"]);
    let accepted = &state.review.as_ref().expect("a review").files["a.rs"].findings[1];
    assert_eq!(accepted.providers, ["deep-provider"]);
    assert_eq!(accepted.lens, "general");
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_keeps_accepted_rows_in_place() {
    assert_golden(
        "projection_keeps_accepted_rows_in_place",
        case_09_accepted_rows_keep_their_place(),
        include_str!("golden/findings_projection/case_09_accepted_rows.json"),
    );
}

fn case_10_accepted_only() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sidecar(
        dir.path(),
        "accepted-findings.json",
        r#"{
  "version": 1,
  "findings": [
    {"file": "a.rs", "run_finding_id": "kept", "run_id": "run-y", "run_diff_hash": "H",
     "accepted_at": "2026-01-03T00:00:00Z", "providers": ["p1"],
     "finding": {"id": "arena-run-y-kept", "title": "Only accepted", "severity": "medium", "line_start": 4, "lens": "general", "raised_by": ["general"]}}
  ]
}"#,
    );

    let state = load(dir.path());
    assert_eq!(ids(&state, "a.rs"), ["arena-run-y-kept"]);
    let review = state.review.as_ref().expect("a synthesized review");
    assert_eq!(review.diff_hash, "H");
    assert_eq!(review.files["a.rs"].findings[0].providers, ["p1"]);
    canonical(
        state.review.as_ref(),
        &state.arbiter_effect,
        &state.arbiter_graded_keys,
        &state.arbiter_verdicts,
    )
}

#[test]
fn projection_synthesizes_a_review_for_accepted_rows_alone() {
    assert_golden(
        "projection_synthesizes_a_review_for_accepted_rows_alone",
        case_10_accepted_only(),
        include_str!("golden/findings_projection/case_10_accepted_only.json"),
    );
}
