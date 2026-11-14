//! The expert reviewer table and the expert sidecar types.

use super::professor::{PROFESSOR_ID, PROFESSOR_ID_PREFIX};
use super::review::{ErReview, Finding, GENERAL_LENS};
use crate::artifact::Artifact;
use crate::slot::ExpertId;
use std::collections::HashMap;

/// One registered expert reviewer (v1 hardcoded list).
#[derive(Debug, Clone, Copy)]
pub struct ExpertDef {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub skill_name: &'static str,
    /// Short prefix for finding ids (`sec-1`, `pat-2`).
    pub id_prefix: &'static str,
}

pub const EXPERTS: &[ExpertDef] = &[
    ExpertDef {
        id: "security",
        label: "Security",
        description: "AuthZ/authN, injection, secrets, unsafe defaults",
        skill_name: "er-review-security",
        id_prefix: "sec",
    },
    ExpertDef {
        id: "performance",
        label: "Performance",
        description: "Hot paths, allocations, blocking I/O, unnecessary work",
        skill_name: "er-review-performance",
        id_prefix: "perf",
    },
    ExpertDef {
        id: "reliability",
        label: "Reliability",
        description: "Error handling, retries, timeouts, resource cleanup",
        skill_name: "er-review-reliability",
        id_prefix: "rel",
    },
    ExpertDef {
        id: "testing",
        label: "Testing",
        description: "Assertion quality, missing negative cases",
        skill_name: "er-review-testing",
        id_prefix: "tst",
    },
    ExpertDef {
        id: "api",
        label: "API / contracts",
        description: "Breaking changes, public surface, semver impact",
        skill_name: "er-review-api",
        id_prefix: "api",
    },
    ExpertDef {
        id: "patterns",
        label: "Patterns",
        description: "Consistency with existing code in the same module/package",
        skill_name: "er-review-patterns",
        id_prefix: "pat",
    },
    ExpertDef {
        id: "simplifying",
        label: "Simplifying",
        description: "Hard-to-read complexity — simplify or document with comments",
        skill_name: "er-review-simplifying",
        id_prefix: "simp",
    },
    ExpertDef {
        id: "mentorship",
        label: "Mentorship",
        description: "Exemplary patterns and quality worth fostering on the team",
        skill_name: "er-review-mentorship",
        id_prefix: "ment",
    },
];

pub fn expert_by_id(id: &str) -> Option<&'static ExpertDef> {
    EXPERTS.iter().find(|e| e.id == id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewerKind {
    Triage,
    General,
    Expert(String),
    Professor,
}

/// Parse `reviewer_kind` from UI/API.
pub fn parse_reviewer_kind(kind: &str) -> Option<ReviewerKind> {
    match kind {
        "general" => Some(ReviewerKind::General),
        "triage" => Some(ReviewerKind::Triage),
        "professor" => Some(ReviewerKind::Professor),
        s if s.starts_with("expert:") => {
            let id = s.strip_prefix("expert:")?;
            expert_by_id(id).map(|_| ReviewerKind::Expert(id.to_string()))
        }
        _ => None,
    }
}

/// The `Finding.lens` a producer id maps to.
///
/// Producer ids are *task kinds* (`general`, `professor`, `expert:<id>`,
/// `triage`); `lens` is the flat finding vocabulary in CONTEXT.md. Anything
/// that is not a named reviewer — an unset kind, or `triage`, which routes
/// rather than producing findings — attributes to the general pass.
pub fn lens_for_producer_id(kind: &str) -> &'static str {
    match parse_reviewer_kind(kind) {
        Some(ReviewerKind::Expert(id)) => expert_by_id(&id).map_or(GENERAL_LENS, |e| e.id),
        Some(ReviewerKind::Professor) => PROFESSOR_ID,
        _ => GENERAL_LENS,
    }
}

/// `.er/experts/{id}.json` — findings + lens-specific summary (no order/checklist/summary.md).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ExpertReview {
    pub version: u32,
    pub expert_id: String,
    pub diff_hash: String,
    #[serde(default)]
    pub diff_scope: String,
    #[serde(default)]
    pub created_at: String,
    /// 2–3 short markdown paragraphs from this expert's lens (shown in AI Review when filtered).
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: HashMap<String, ExpertFileReview>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ExpertFileReview {
    #[serde(default, deserialize_with = "super::review::lenient_findings")]
    pub findings: Vec<Finding>,
}

impl Artifact for ExpertReview {
    const KIND: &'static str = "expert";
    const VERSION: u32 = 1;
    const FILE: &'static str = "experts/{slot}.json";
    type Slot = ExpertId;

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

/// Expert id owning a finding-id prefix (`sec` → `security`).
pub fn expert_id_for_id_prefix(prefix: &str) -> Option<&'static str> {
    EXPERTS.iter().find(|e| e.id_prefix == prefix).map(|e| e.id)
}

/// Fill `lens` for findings whose sidecar predates the field.
///
/// Expert and professor findings are recognised by the id prefix their merge
/// path assigns; anything else in `review.json` is general-review output. Call
/// this once, right after `review.json` is parsed and before the sidecar merges
/// — they set `lens` themselves and skip anything already attributed.
pub fn backfill_finding_lenses(review: &mut ErReview) {
    for file in review.files.values_mut() {
        for finding in &mut file.findings {
            if !finding.lens.is_empty() {
                continue;
            }
            finding.lens = match finding.id.split_once('-') {
                Some((prefix, _)) => expert_id_for_id_prefix(prefix)
                    .or_else(|| (prefix == PROFESSOR_ID_PREFIX).then_some(PROFESSOR_ID))
                    .unwrap_or(GENERAL_LENS),
                None => GENERAL_LENS,
            }
            .to_string();
            if finding.raised_by.is_empty() {
                finding.raised_by = vec![finding.lens.clone()];
            }
        }
    }
}

/// Whether an expert sidecar belongs on the review card: generated against the
/// diff the tab shows now, or against the same diff as the general review it is
/// merged into. A selected-files run hashes only the filtered diff, so its
/// general + expert sidecars share a hash the full tab diff never matches — the
/// card is already flagged stale for that, and skipping the experts on top
/// silently hid their findings.
pub fn expert_hash_accepted(
    expert: &ExpertReview,
    review_hash: &str,
    current_diff_hash: &str,
) -> bool {
    expert.diff_hash == current_diff_hash
        || (!review_hash.is_empty() && expert.diff_hash == review_hash)
}

/// Background task kind label, e.g. `expert:security`.
pub fn expert_task_kind(expert_id: &str) -> String {
    format!("expert:{expert_id}")
}
