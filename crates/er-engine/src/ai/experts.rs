//! Specialized expert reviewers — registry, sidecar types, merge into general review.

use super::professor::{PROFESSOR_ID, PROFESSOR_ID_PREFIX};
use super::review::{ErFileReview, ErReview, Finding, RiskLevel, GENERAL_LENS};
use std::collections::HashMap;
use std::path::Path;

/// Caps for findings in prompts (general vs expert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindingCaps {
    pub per_file: u32,
    pub total: u32,
    pub is_expert: bool,
}

impl FindingCaps {
    pub const fn general() -> Self {
        Self {
            per_file: 4,
            total: 15,
            is_expert: false,
        }
    }

    pub const fn expert() -> Self {
        Self {
            per_file: 2,
            total: 10,
            is_expert: true,
        }
    }
}

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

/// What the `summary` field in an expert sidecar should cover (prompt + skills).
pub fn expert_summary_focus(expert_id: &str) -> &'static str {
    match expert_id {
        "security" => {
            "security posture of this diff: trust boundaries touched, authZ/authN, secrets, injection, and whether findings are blocking"
        }
        "performance" => {
            "performance impact: hot paths, allocations, blocking I/O, and whether the diff adds overhead or removes waste"
        }
        "reliability" => {
            "reliability: error handling, retries, timeouts, resource cleanup, and failure modes introduced or fixed"
        }
        "testing" => {
            "test coverage and quality: what is exercised, missing negative cases, and assertion strength"
        }
        "api" => {
            "API and contract impact: breaking changes, public surface changes, and semver implications"
        }
        "patterns" => {
            "consistency with existing patterns in the codebase — where the diff matches or diverges from established usage"
        }
        "simplifying" => {
            "readability and complexity: what is hard to follow, what should be simplified or documented, and review friction"
        }
        "mentorship" => {
            "exemplary patterns worth fostering (positive-only): what this diff does well and why it is worth emulating"
        }
        _ => "findings from this expert lens in 2–3 short paragraphs",
    }
}

pub fn expert_label_for_id(id: &str) -> Option<&'static str> {
    expert_by_id(id).map(|e| e.label)
}

/// Display label for a producer: an expert id, `professor`, or `triage`. Fed a
/// finding's `lens` and a background task's kind — the two places a producer id
/// is recorded. Never a finding's `category`, which names a kind of defect.
pub fn agent_label_for_id(id: &str) -> &'static str {
    if id == super::triage::TRIAGE_ID {
        return super::triage::TRIAGE_LABEL;
    }
    if id == super::professor::PROFESSOR_ID {
        return super::professor::PROFESSOR_LABEL;
    }
    if let Some(def) = expert_by_id(id) {
        return def.label;
    }
    "General"
}

/// One selectable reviewer in the AI Hub / file-review picker.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewerInfo {
    pub kind: String,
    pub label: String,
    pub description: String,
}

pub fn list_ai_reviewers() -> Vec<ReviewerInfo> {
    let mut out = vec![
        ReviewerInfo {
            kind: super::triage::TRIAGE_ID.to_string(),
            label: super::triage::TRIAGE_LABEL.to_string(),
            description: "Fast branch scan — first impression and review routing".to_string(),
        },
        ReviewerInfo {
            kind: "general".to_string(),
            label: "General".to_string(),
            description: "Risk, order, checklist, and summary".to_string(),
        },
    ];
    for e in EXPERTS {
        out.push(ReviewerInfo {
            kind: format!("expert:{}", e.id),
            label: e.label.to_string(),
            description: e.description.to_string(),
        });
    }
    out.push(ReviewerInfo {
        kind: super::professor::PROFESSOR_ID.to_string(),
        label: super::professor::PROFESSOR_LABEL.to_string(),
        description: "Learn the implementation — key mechanisms in this diff".to_string(),
    });
    out
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
        Some(ReviewerKind::Professor) => super::professor::PROFESSOR_ID,
        _ => GENERAL_LENS,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewerKind {
    Triage,
    General,
    Expert(String),
    Professor,
}

/// Desktop / Tauri list payload.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExpertInfo {
    pub id: String,
    pub label: String,
    pub description: String,
}

pub fn list_expert_info() -> Vec<ExpertInfo> {
    EXPERTS
        .iter()
        .map(|e| ExpertInfo {
            id: e.id.to_string(),
            label: e.label.to_string(),
            description: e.description.to_string(),
        })
        .collect()
}

/// `.er/experts/{id}.json` — findings + lens-specific summary (no order/checklist/summary.md).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExpertFileReview {
    #[serde(default, deserialize_with = "super::review::lenient_findings")]
    pub findings: Vec<Finding>,
}

const MAX_SIDECAR_BYTES: u64 = 10_000_000;

fn read_expert_sidecar(path: &Path) -> Option<ExpertReview> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_SIDECAR_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Load all `.er/experts/*.json` sidecars.
pub fn load_expert_reviews(er_dir: &str) -> Vec<ExpertReview> {
    let experts_dir = Path::new(er_dir).join("experts");
    let Ok(entries) = std::fs::read_dir(&experts_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(review) = read_expert_sidecar(&path) {
            out.push(review);
        }
    }
    out
}

fn prefix_finding_id(prefix: &str, id: &str) -> String {
    if id.starts_with(&format!("{prefix}-")) {
        id.to_string()
    } else {
        format!("{prefix}-{id}")
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

/// Merge expert findings into `review` at load time (skip stale expert files).
pub fn merge_experts_into_review(
    review: &mut ErReview,
    experts: &[ExpertReview],
    current_diff_hash: &str,
) {
    for expert in experts {
        if !expert_hash_accepted(expert, &review.diff_hash, current_diff_hash) {
            continue;
        }
        let Some(def) = expert_by_id(&expert.expert_id) else {
            continue;
        };
        for (path, efr) in &expert.files {
            let entry = review
                .files
                .entry(path.clone())
                .or_insert_with(|| ErFileReview {
                    risk: RiskLevel::Info,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: Vec::new(),
                });
            for mut finding in efr.findings.clone() {
                finding.id = prefix_finding_id(def.id_prefix, &finding.id);
                finding.lens = def.id.to_string();
                // A sidecar's own findings come from this one expert, so it is
                // the whole raiser set until something merges several.
                if finding.raised_by.is_empty() {
                    finding.raised_by = vec![def.id.to_string()];
                }
                entry.findings.push(finding);
            }
        }
    }
}

/// Build a minimal `ErReview` shell when only expert sidecars exist.
pub fn synthesize_review_from_experts(
    experts: &[ExpertReview],
    current_diff_hash: &str,
) -> Option<ErReview> {
    let fresh: Vec<&ExpertReview> = experts
        .iter()
        .filter(|e| e.diff_hash == current_diff_hash)
        .collect();
    if fresh.is_empty() {
        return None;
    }
    let first = fresh[0];
    let mut review = ErReview {
        version: 1,
        diff_hash: current_diff_hash.to_string(),
        created_at: first.created_at.clone(),
        base_branch: String::new(),
        head_branch: String::new(),
        files: HashMap::new(),
        file_hashes: HashMap::new(),
    };
    merge_experts_into_review(&mut review, experts, current_diff_hash);
    if review.files.is_empty() {
        return None;
    }
    Some(review)
}

/// Background task kind label, e.g. `expert:security`.
pub fn expert_task_kind(expert_id: &str) -> String {
    format!("expert:{expert_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{Confidence, RiskLevel};

    /// A finding as an expert sidecar writes it: a defect kind, no lens (the
    /// merge assigns that), no line text.
    fn sample_finding(id: &str) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Medium,
            lens: String::new(),
            category: "correctness".to_string(),
            raised_by: Vec::new(),
            title: "Test".to_string(),
            description: String::new(),
            hunk_index: Some(0),
            line_start: Some(1),
            line_end: None,
            line_content: String::new(),
            stale: false,
            suggestion: String::new(),
            related_files: vec![],
            outside_diff: false,
            confidence: Confidence::Confirmed,
            verification_plan: String::new(),
            evidence: vec![],
            responses: vec![],
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    #[test]
    fn merge_prefixes_ids_and_skips_stale() {
        let hash = "abc123";
        let mut review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        let experts = vec![
            ExpertReview {
                version: 1,
                expert_id: "security".to_string(),
                diff_hash: hash.to_string(),
                diff_scope: "branch".to_string(),
                created_at: String::new(),
                summary: String::new(),
                files: HashMap::from([(
                    "src/a.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![sample_finding("1")],
                    },
                )]),
            },
            ExpertReview {
                version: 1,
                expert_id: "patterns".to_string(),
                diff_hash: "stale".to_string(),
                diff_scope: String::new(),
                created_at: String::new(),
                summary: String::new(),
                files: HashMap::from([(
                    "src/b.rs".to_string(),
                    ExpertFileReview {
                        findings: vec![sample_finding("9")],
                    },
                )]),
            },
        ];
        merge_experts_into_review(&mut review, &experts, hash);
        assert_eq!(review.files.len(), 1);
        let f = &review.files["src/a.rs"];
        assert_eq!(f.findings.len(), 1);
        assert_eq!(f.findings[0].id, "sec-1");
        assert_eq!(f.findings[0].lens, "security");
    }

    /// Regression: the merge used to assign `finding.category = def.id` outright,
    /// so an expert classifying its finding as a correctness issue had that
    /// classification replaced by its own lens name. The producer now goes to
    /// `lens` and `category` is left alone.
    #[test]
    fn merge_keeps_the_experts_defect_category() {
        let hash = "abc123";
        let mut review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        let experts = vec![ExpertReview {
            version: 1,
            expert_id: "security".to_string(),
            diff_hash: hash.to_string(),
            diff_scope: "branch".to_string(),
            created_at: String::new(),
            summary: String::new(),
            files: HashMap::from([(
                "src/a.rs".to_string(),
                ExpertFileReview {
                    findings: vec![sample_finding("1")],
                },
            )]),
        }];

        merge_experts_into_review(&mut review, &experts, hash);

        let f = &review.files["src/a.rs"].findings[0];
        assert_eq!(f.category, "correctness", "defect kind was overwritten");
        assert_eq!(f.lens, "security");
    }

    #[test]
    fn backfill_attributes_findings_written_before_lens_existed() {
        let mut review = ErReview {
            version: 1,
            diff_hash: "h".to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::from([(
                "src/a.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings: vec![
                        sample_finding("sec-1"),
                        sample_finding("pat-2"),
                        sample_finding("prof-3"),
                        sample_finding("f-4"),
                    ],
                },
            )]),
            file_hashes: HashMap::new(),
        };
        // A finding already attributed by a merge path keeps that attribution
        // even when its id prefix says otherwise.
        review.files.get_mut("src/a.rs").unwrap().findings[0].lens = "api".to_string();

        backfill_finding_lenses(&mut review);

        let lenses: Vec<&str> = review.files["src/a.rs"]
            .findings
            .iter()
            .map(|f| f.lens.as_str())
            .collect();
        assert_eq!(lenses, vec!["api", "patterns", "professor", "general"]);
    }

    #[test]
    fn expert_summary_focus_non_empty_for_all_experts() {
        for e in EXPERTS {
            assert!(!expert_summary_focus(e.id).is_empty());
        }
    }

    /// Producer ids are task kinds; `lens` is the flat finding vocabulary. An
    /// unset kind or `triage` — which routes rather than producing findings —
    /// attributes to the general pass.
    #[test]
    fn lens_for_producer_id_normalizes_task_kinds() {
        assert_eq!(lens_for_producer_id("expert:security"), "security");
        assert_eq!(lens_for_producer_id("expert:simplifying"), "simplifying");
        assert_eq!(lens_for_producer_id("professor"), "professor");
        assert_eq!(lens_for_producer_id("general"), GENERAL_LENS);
        assert_eq!(lens_for_producer_id("triage"), GENERAL_LENS);
        assert_eq!(lens_for_producer_id(""), GENERAL_LENS);
        // An expert task kind with an id no longer in the registry.
        assert_eq!(lens_for_producer_id("expert:retired"), GENERAL_LENS);
    }

    #[test]
    fn agent_label_maps_general_expert_professor() {
        assert_eq!(agent_label_for_id("security"), "Security");
        assert_eq!(agent_label_for_id("professor"), "Professor");
        assert_eq!(agent_label_for_id("logic"), "General");
    }

    #[test]
    fn parse_reviewer_kind_accepts_all_kinds() {
        assert_eq!(parse_reviewer_kind("general"), Some(ReviewerKind::General));
        assert_eq!(
            parse_reviewer_kind("expert:api"),
            Some(ReviewerKind::Expert("api".to_string()))
        );
        assert_eq!(
            parse_reviewer_kind("professor"),
            Some(ReviewerKind::Professor)
        );
        assert_eq!(parse_reviewer_kind("triage"), Some(ReviewerKind::Triage));
        assert!(parse_reviewer_kind("expert:unknown").is_none());
    }

    #[test]
    fn list_ai_reviewers_includes_general_and_professor() {
        let list = list_ai_reviewers();
        assert!(list.iter().any(|r| r.kind == "general"));
        assert!(list.iter().any(|r| r.kind == "professor"));
        assert!(list.first().is_some_and(|r| r.kind == "triage"));
        assert_eq!(list.len(), EXPERTS.len() + 3);
    }

    #[test]
    fn merge_adds_to_existing_file_entry() {
        let hash = "h";
        let mut review = ErReview {
            version: 1,
            diff_hash: hash.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::from([(
                "x.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::Low,
                    risk_reason: String::new(),
                    summary: "s".to_string(),
                    findings: vec![sample_finding("f-1")],
                },
            )]),
            file_hashes: HashMap::new(),
        };
        let experts = vec![ExpertReview {
            version: 1,
            expert_id: "api".to_string(),
            diff_hash: hash.to_string(),
            diff_scope: String::new(),
            created_at: String::new(),
            summary: String::new(),
            files: HashMap::from([(
                "x.rs".to_string(),
                ExpertFileReview {
                    findings: vec![sample_finding("2")],
                },
            )]),
        }];
        merge_experts_into_review(&mut review, &experts, hash);
        assert_eq!(review.files["x.rs"].findings.len(), 2);
        assert!(review.files["x.rs"]
            .findings
            .iter()
            .any(|f| f.id == "api-2"));
    }

    #[test]
    fn merge_keeps_experts_sharing_the_review_hash_after_tab_diff_moved_on() {
        // Selected-files run: general + expert sidecars hash the filtered diff
        // ("scoped") while the tab hashes the full diff ("full"). The review
        // loads as stale; its experts must ride along instead of vanishing.
        // An expert from an older generation still stays out.
        let mut review = ErReview {
            version: 1,
            diff_hash: "scoped".to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::new(),
            file_hashes: HashMap::new(),
        };
        let expert = |id: &str, hash: &str| ExpertReview {
            version: 1,
            expert_id: id.to_string(),
            diff_hash: hash.to_string(),
            diff_scope: String::new(),
            created_at: String::new(),
            summary: String::new(),
            files: HashMap::from([(
                "m.sql".to_string(),
                ExpertFileReview {
                    findings: vec![sample_finding("1")],
                },
            )]),
        };
        let experts = vec![expert("reliability", "scoped"), expert("security", "older")];
        merge_experts_into_review(&mut review, &experts, "full");
        let lenses: Vec<&str> = review.files["m.sql"]
            .findings
            .iter()
            .map(|f| f.lens.as_str())
            .collect();
        assert_eq!(lenses, vec!["reliability"]);
        assert!(
            !expert_hash_accepted(&experts[0], "", "full"),
            "without a review hash only the tab's own hash is accepted"
        );
    }
}
