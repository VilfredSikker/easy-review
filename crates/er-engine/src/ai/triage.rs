//! Triage reviewer — fast branch scan in `.er/triage.json` (routing verdict, not findings).

use super::review::RiskLevel;
use std::path::Path;

pub const TRIAGE_ID: &str = "triage";
pub const TRIAGE_LABEL: &str = "Triage";
pub const TRIAGE_SKILL: &str = "er-triage";

/// Recommended next review step from triage.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriageVerdictPrimary {
    #[default]
    General,
    Expert,
    Arena,
    Professor,
    Skip,
}

/// `.er/triage.json` — first impression + routing verdict + priority files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TriageReview {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub diff_scope: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub first_impression: String,
    #[serde(default)]
    pub diff_stats: TriageDiffStats,
    #[serde(default)]
    pub verdict: TriageVerdict,
    #[serde(default)]
    pub priority_files: Vec<TriagePriorityFile>,
    /// How much existing code the change touches, and what keeps new code off.
    /// Absent in triage written before reach existed; see `TriageReach`.
    #[serde(default, deserialize_with = "lenient_reach")]
    pub reach: TriageReach,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TriageDiffStats {
    #[serde(default)]
    pub files_changed: u32,
    #[serde(default, deserialize_with = "super::review::lenient_risk_level")]
    pub approx_risk: RiskLevel,
    #[serde(default)]
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TriageVerdict {
    #[serde(default)]
    pub primary: TriageVerdictPrimary,
    #[serde(default)]
    pub experts: Vec<String>,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub confidence: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TriagePriorityFile {
    pub path: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default, deserialize_with = "super::review::lenient_risk_level")]
    pub risk: RiskLevel,
}

/// How far a change reaches into code that existed before it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReachLevel {
    /// New code that nothing existing calls into, apart from wiring.
    Isolated,
    /// Edits existing code, but code few other places depend on.
    Contained,
    /// Edits code much of the repo depends on: shared modules, schemas, routing.
    Broad,
    /// Not judged — triage from before reach existed, or a word that did not read.
    #[default]
    Unknown,
}

impl ReachLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::Contained => "contained",
            Self::Broad => "broad",
            Self::Unknown => "unknown",
        }
    }
}

/// Triage's reach call. Engine facts (`change-facts.md`) feed it; the agent
/// judges it. `docs/adr/0039-reach-is-judged-from-engine-facts.md`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TriageReach {
    #[serde(default, deserialize_with = "lenient_reach_level")]
    pub level: ReachLevel,
    #[serde(default)]
    pub reason: String,
    /// Edits to existing code the new code is wired in through, as
    /// `path:line — what it does` (a route table, a DI registration).
    #[serde(default)]
    pub touch_points: Vec<String>,
    #[serde(default)]
    pub guard: Option<TriageGuard>,
}

/// A switch that keeps the new code off until someone turns it on.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TriageGuard {
    /// `feature_flag`, `permission`, `config`, `unrouted`, or another word.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    /// `path:line` where the guard is checked.
    #[serde(default)]
    pub evidence: String,
}

impl TriageGuard {
    /// A guard claim only counts with a place in the code to check it.
    pub fn is_evidenced(&self) -> bool {
        !self.evidence.trim().is_empty()
    }
}

fn lenient_reach_level<'de, D>(deserializer: D) -> Result<ReachLevel, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|v| serde_json::from_value::<ReachLevel>(v).ok())
        .unwrap_or_default())
}

/// A malformed `reach` block degrades to unknown rather than failing the parse:
/// `load_triage_review` drops the whole file on any error, and losing the
/// verdict over one optional block is the worse trade.
fn lenient_reach<'de, D>(deserializer: D) -> Result<TriageReach, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|v| serde_json::from_value::<TriageReach>(v).ok())
        .unwrap_or_default())
}

const MAX_SIDECAR_BYTES: u64 = 10_000_000;

fn read_triage_sidecar(path: &Path) -> Option<TriageReview> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_SIDECAR_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn load_triage_review(er_dir: &str) -> Option<TriageReview> {
    read_triage_sidecar(&Path::new(er_dir).join("triage.json"))
}

pub fn triage_is_fresh(triage: &TriageReview, current_diff_hash: &str) -> bool {
    triage.diff_hash == current_diff_hash
}

/// Background task kind for triage runs.
pub fn triage_task_kind() -> String {
    TRIAGE_ID.to_string()
}

pub const fn verdict_primary_str(v: &TriageVerdictPrimary) -> &'static str {
    match v {
        TriageVerdictPrimary::General => "general",
        TriageVerdictPrimary::Expert => "expert",
        TriageVerdictPrimary::Arena => "arena",
        TriageVerdictPrimary::Professor => "professor",
        TriageVerdictPrimary::Skip => "skip",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triage_without_reach_loads_as_unknown() {
        let json = r#"{ "version": 1, "diff_hash": "abc" }"#;
        let triage: TriageReview = serde_json::from_str(json).unwrap();
        assert_eq!(triage.reach.level, ReachLevel::Unknown);
        assert!(triage.reach.guard.is_none());
    }

    #[test]
    fn reach_with_evidenced_guard_reads() {
        let json = r#"{
            "version": 1,
            "diff_hash": "abc",
            "reach": {
                "level": "isolated",
                "reason": "New plates service; only the router line touches existing code",
                "touch_points": ["src/router.ts:42 — registers /plates/move"],
                "guard": { "kind": "feature_flag", "name": "plates.move", "evidence": "src/plates/routes.ts:8" }
            }
        }"#;
        let triage: TriageReview = serde_json::from_str(json).unwrap();
        assert_eq!(triage.reach.level, ReachLevel::Isolated);
        assert_eq!(triage.reach.touch_points.len(), 1);
        assert!(triage.reach.guard.as_ref().unwrap().is_evidenced());
    }

    /// `load_triage_review` drops the whole file on a parse error, so a model
    /// writing an odd reach must cost the reach block and nothing else.
    #[test]
    fn malformed_reach_keeps_the_rest_of_the_triage() {
        for reach in [
            r#""broad""#,
            r#"{ "level": "sprawling" }"#,
            r#"{ "level": "broad", "touch_points": "src/a.rs" }"#,
        ] {
            let json = format!(
                r#"{{ "version": 1, "diff_hash": "abc", "verdict": {{ "primary": "skip" }}, "reach": {reach} }}"#
            );
            let triage: TriageReview = serde_json::from_str(&json).unwrap();
            assert_eq!(
                triage.verdict.primary,
                TriageVerdictPrimary::Skip,
                "{reach}"
            );
        }
        let json = r#"{ "version": 1, "diff_hash": "abc", "reach": { "level": "sprawling", "reason": "r" } }"#;
        let triage: TriageReview = serde_json::from_str(json).unwrap();
        assert_eq!(triage.reach.level, ReachLevel::Unknown);
        assert_eq!(triage.reach.reason, "r");
    }

    #[test]
    fn guard_without_evidence_does_not_count() {
        let guard = TriageGuard {
            kind: "feature_flag".into(),
            name: "plates.move".into(),
            evidence: " ".into(),
        };
        assert!(!guard.is_evidenced());
    }

    #[test]
    fn deserialize_sample_triage_json() {
        let json = r#"{
            "version": 1,
            "diff_hash": "abc123",
            "diff_scope": "branch",
            "created_at": "2026-01-01T00:00:00Z",
            "first_impression": "Auth refactor touches session handling.",
            "diff_stats": {
                "files_changed": 12,
                "approx_risk": "medium",
                "domains": ["auth", "api"]
            },
            "verdict": {
                "primary": "expert",
                "experts": ["security"],
                "rationale": "New trust boundary in middleware",
                "confidence": "high"
            },
            "priority_files": [
                { "path": "src/auth.rs", "reason": "Session validation", "risk": "high" }
            ]
        }"#;
        let triage: TriageReview = serde_json::from_str(json).unwrap();
        assert_eq!(triage.verdict.primary, TriageVerdictPrimary::Expert);
        assert_eq!(triage.verdict.experts, vec!["security"]);
        assert_eq!(triage.diff_stats.approx_risk, RiskLevel::Medium);
        assert_eq!(triage.priority_files.len(), 1);
        assert_eq!(triage.priority_files[0].risk, RiskLevel::High);
        assert!(triage_is_fresh(&triage, "abc123"));
        assert!(!triage_is_fresh(&triage, "other"));
    }

    /// Both risk fields are typed now, so a model writing a word outside the
    /// four levels degrades that one value instead of discarding the verdict.
    #[test]
    fn unrecognised_risk_degrades_to_info() {
        let json = r#"{
            "version": 1,
            "diff_hash": "abc123",
            "diff_stats": { "files_changed": 3, "approx_risk": "banana" },
            "priority_files": [{ "path": "src/a.rs", "risk": "moderate" }]
        }"#;
        let triage: TriageReview = serde_json::from_str(json).unwrap();
        assert_eq!(triage.diff_stats.approx_risk, RiskLevel::Info);
        assert_eq!(triage.priority_files[0].risk, RiskLevel::Info);
    }

    /// Absent risk fields keep working — sidecars written before the fields were
    /// typed, and prompts that omit them, must not fail the whole file.
    #[test]
    fn missing_risk_fields_still_load() {
        let json = r#"{
            "version": 1,
            "diff_hash": "abc123",
            "diff_stats": { "files_changed": 3 },
            "priority_files": [{ "path": "src/a.rs" }]
        }"#;
        let triage: TriageReview = serde_json::from_str(json).unwrap();
        assert_eq!(triage.diff_stats.approx_risk, RiskLevel::Info);
        assert_eq!(triage.priority_files[0].risk, RiskLevel::Info);
    }
}
