use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::relocate::{relocate_comment, CommentAnchor, RelocationResult};
use crate::artifact::Artifact;
use crate::diff::DiffFile;

// ── Inline layer visibility ──

/// Inline annotation layer visibility (replaces ViewMode)
#[derive(Debug, Clone)]
pub struct InlineLayers {
    pub show_questions: bool,
    pub show_github_comments: bool,
    pub show_ai_findings: bool,
    pub hide_resolved: bool,
    /// Least trustworthy grade a finding may carry and still render. Resolved
    /// from whether an arbiter has graded the current diff, so the gate follows
    /// the state of the review rather than a remembered preference.
    pub min_trust: Confidence,
    /// The gate was set by hand, so a review reload must not overwrite it.
    pub min_trust_pinned: bool,
    /// List the findings the arbiter ruled out, under the count of them. Off by
    /// default: the count is the signal, and the rows are what you ask for when
    /// the count surprises you.
    pub show_dropped: bool,
}

impl Default for InlineLayers {
    fn default() -> Self {
        Self {
            show_questions: true,
            show_github_comments: true,
            show_ai_findings: true,
            hide_resolved: false,
            min_trust: Confidence::Informational,
            min_trust_pinned: false,
            show_dropped: false,
        }
    }
}

// ── .er-review.json ──

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErReview {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub base_branch: String,
    #[serde(default)]
    pub head_branch: String,
    #[serde(default)]
    pub files: HashMap<String, ErFileReview>,
    /// Per-file diff hashes for incremental staleness detection
    #[serde(default)]
    pub file_hashes: HashMap<String, String>,
}

impl Artifact for ErReview {
    const KIND: &'static str = "review";
    const VERSION: u32 = 1;
    const FILE: &'static str = "review.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErFileReview {
    pub risk: RiskLevel,
    #[serde(default)]
    pub risk_reason: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, deserialize_with = "lenient_findings")]
    pub findings: Vec<Finding>,
}

/// Deserialize an optional line anchor from model-authored JSON without
///
/// failing the whole sidecar. The annotated diff tags deleted lines as
/// `[h<N> L-<old>]`, and models sometimes copy the negative number into
/// `line_start` verbatim — which `Option<usize>` would reject, silently
/// discarding the entire review. Anything that isn't a non-negative integer
/// degrades to `None` (hunk-level anchor).
pub fn lenient_line_anchor<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(serde_json::Value::as_i64)
        .and_then(|n| usize::try_from(n).ok()))
}

/// Deserialize a findings array, skipping individual entries that fail to
///
/// parse instead of rejecting the whole sidecar. AI-authored files
/// occasionally contain one malformed finding; losing that finding is far
/// better than showing "no findings" for the entire review.
pub fn lenient_findings<'de, D>(deserializer: D) -> Result<Vec<Finding>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

/// Deserialize a verdicts array, skipping entries that fail to parse instead of
/// rejecting the whole `arbiter.json`. Same trade as `lenient_findings`: losing
/// one verdict beats losing every grade in the file.
pub fn lenient_verdicts<'de, D>(
    deserializer: D,
) -> Result<Vec<super::arbiter::ArbiterVerdict>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

/// Deserialize a risk level, degrading an unrecognised value to `Info` rather
/// than rejecting the sidecar that holds it. Models write free-form words here
/// (`"moderate"`, `"unknown"`), and losing a whole triage verdict over one
/// adjective is a worse trade than an imprecise tier.
pub fn lenient_risk_level<'de, D>(deserializer: D) -> Result<RiskLevel, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|v| serde_json::from_value::<RiskLevel>(v).ok())
        .unwrap_or(RiskLevel::Info))
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    High,
    Medium,
    Low,
    /// Default: no signal. Also where an unrecognised value lands — see
    /// `lenient_risk_level`.
    #[default]
    Info,
}

impl RiskLevel {
    /// Lowercase level name, matching serde's serialization: `"high"`,
    /// `"medium"`, `"low"`, `"info"`. The desktop's file-risk dot uses its own
    /// shorter vocabulary (`severity_str`, where `Medium` is `"med"`), so the
    /// two are deliberately not interchangeable.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Info => "info",
        }
    }

    pub const fn symbol(&self) -> &'static str {
        match self {
            Self::High => "●",
            Self::Medium => "●",
            Self::Low => "●",
            Self::Info => "○",
        }
    }
}

/// Confidence in a finding, set by the AI based on what it could verify.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Default for older review.json files. Review pass uses this when a finding
    /// couldn't be verified within the read budget. Validate pass should resolve.
    #[default]
    Tentative,
    /// Evidence supports the finding; the reviewer should act on it.
    Confirmed,
    /// Real, but pre-existing or low-impact relative to this PR. Optional fix.
    Informational,
    /// Validate disproved the concern. Kept for audit trail; rendered dimmed.
    Dropped,
}

impl Confidence {
    /// This level as the 0..1 the arena carries.
    ///
    /// The pair with `from_score` has to stay a round trip: a grade written by
    /// one mapping and read back by a diverging copy would silently change a
    /// finding's confidence on the way through the arena.
    pub const fn score(self) -> f32 {
        match self {
            Self::Confirmed => 0.9,
            Self::Tentative => 0.6,
            Self::Informational => 0.3,
            Self::Dropped => 0.0,
        }
    }

    /// The level a 0..1 score represents. Thresholds are `score`'s, inverted.
    pub const fn from_score(score: f32) -> Self {
        if score >= 0.75 {
            Self::Confirmed
        } else if score >= 0.5 {
            Self::Tentative
        } else {
            Self::Informational
        }
    }

    /// Trust ordering, **lower meaning more trustworthy**.
    ///
    /// Named `trust_rank` rather than `rank` so a call site cannot misread the
    /// direction: every gate and sort in the codebase compares these with `<=`
    /// or ascending order.
    pub const fn trust_rank(self) -> u8 {
        match self {
            Self::Confirmed => 0,
            Self::Tentative => 1,
            Self::Informational => 2,
            Self::Dropped => 3,
        }
    }
}

/// One read or grep result the AI used to justify (or revise) a finding.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EvidenceItem {
    pub file: String,
    #[serde(default, deserialize_with = "lenient_line_anchor")]
    pub line_start: Option<usize>,
    #[serde(default, deserialize_with = "lenient_line_anchor")]
    pub line_end: Option<usize>,
    /// What the agent learned from this file/range.
    pub note: String,
}

/// Lens id for findings produced by the general review pass — the fallback when
/// nothing else claims a finding. See CONTEXT.md, "Lens".
pub const GENERAL_LENS: &str = "general";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Finding {
    pub id: String,
    pub severity: RiskLevel,
    /// Who produced this finding — an expert id, or `general` / `professor` /
    /// `arbiter`. Set at load time by whichever merge path owns the sidecar, so
    /// a producer never writes it. See CONTEXT.md, "Lens". Empty on sidecars
    /// written before the field existed; backfilled from the finding id prefix.
    #[serde(default)]
    pub lens: String,
    /// What kind of defect this finding describes (`correctness`, …). Orthogonal
    /// to `lens` — the security lens can raise a correctness finding.
    #[serde(default)]
    pub category: String,
    /// Every lens that raised this claim. `lens` names the one it is filed
    /// under; this is the full set, so a claim several experts independently
    /// found reads as such. Empty on sidecars written before it existed, and
    /// treated as `[lens]` when read.
    #[serde(default)]
    pub raised_by: Vec<String>,
    /// Host-derived provider IDs. Producers cannot persist or author this field.
    #[serde(default, skip_serializing)]
    pub providers: Vec<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// 0-based index into the file's hunks
    #[serde(default, deserialize_with = "lenient_line_anchor")]
    pub hunk_index: Option<usize>,
    #[serde(default, deserialize_with = "lenient_line_anchor")]
    pub line_start: Option<usize>,
    #[serde(default, deserialize_with = "lenient_line_anchor")]
    pub line_end: Option<usize>,
    /// Text of the line this finding anchors to, as it was when the finding was
    /// written. Mirrors the comment types. Empty for file- and hunk-level
    /// findings, which cannot go stale individually.
    #[serde(default)]
    pub line_content: String,
    /// Runtime-only: the anchored line is no longer in the diff. Recomputed on
    /// each refresh from `line_content`; never persisted.
    #[serde(skip)]
    pub stale: bool,
    #[serde(default)]
    pub suggestion: String,
    #[serde(default)]
    pub related_files: Vec<String>,
    /// True when the AI flagged the referenced code as outside the diff hunks
    /// (i.e. the issue was not introduced by this PR). Downstream agents reading
    /// this file should treat such findings as informational — fix is optional
    /// and may belong in a separate change.
    #[serde(default)]
    pub outside_diff: bool,
    /// AI's confidence in this finding. Set by review (mostly confirmed/informational/tentative)
    /// and refined by validate (confirmed/informational/dropped).
    #[serde(default)]
    pub confidence: Confidence,
    /// One-line description of what would resolve a Tentative finding —
    /// what file/symbol/test the validate pass should read or grep.
    #[serde(default)]
    pub verification_plan: String,
    /// Files/ranges the AI actually read while evaluating this finding.
    #[serde(default)]
    pub evidence: Vec<EvidenceItem>,
    #[serde(default)]
    pub responses: Vec<AiResponse>,
    /// User has fixed (or otherwise addressed) this finding. Set by validate
    /// when the issue is no longer present in the current diff/file. Orthogonal
    /// to `confidence` — a Confirmed finding can become resolved.
    #[serde(default)]
    pub resolved: bool,
    /// One-line summary of what changed that resolved the finding (e.g.
    /// "line 42 now has the null check", "function deleted").
    #[serde(default)]
    pub resolved_note: String,
    /// ISO 8601 timestamp when validate marked this resolved. Empty otherwise.
    #[serde(default)]
    pub resolved_at: String,
    /// ID of the GitHub comment this finding was promoted to (if any).
    /// Populated from a sibling `.er/finding-promotions.json` mapping at load time
    /// to avoid writing into AI-owned `.er/review.json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub promoted_to: Option<String>,
}

impl Finding {
    /// Whether at least one producer assigned this finding to the security lens.
    pub fn is_security_stage(&self) -> bool {
        self.raisers().contains(&"security")
    }

    /// Whether this finding still demands the reviewer's attention. False
    /// when the user has fixed it (`resolved`) or the AI dismissed it as a
    /// false positive (`Confidence::Dropped`).
    pub const fn is_active(&self) -> bool {
        !self.resolved && !matches!(self.confidence, Confidence::Dropped)
    }

    /// Whether this finding clears the layers' confidence gate.
    ///
    /// `is_active()` runs first, so `Dropped` never reaches the comparison and a
    /// `min_trust` of `Dropped` cannot resurrect a removed finding.
    pub fn passes(&self, layers: &InlineLayers) -> bool {
        self.is_active() && self.confidence.trust_rank() <= layers.min_trust.trust_rank()
    }

    /// Recompute `stale` against the current diff.
    ///
    /// A finding whose target line can no longer be found is suspect; one whose
    /// line merely moved is not — the same distinction the comment path draws
    /// with `relocate_comment`. Findings are never re-anchored, so the position
    /// the reviewer wrote stays put and only the flag moves.
    ///
    /// `diff_file` is `None` when the file itself has left the diff, which is
    /// the comment path's `Missing → Lost` case: an anchored finding there is
    /// stale. A caller that merely hasn't parsed the file yet must not pass
    /// `None` — see `TabState::refresh_finding_staleness`.
    ///
    /// File- and hunk-level findings carry no `line_content` and stay fresh:
    /// `AiState::stale_files` already covers the file as a whole.
    pub fn refresh_stale(&mut self, diff_file: Option<&DiffFile>) {
        if self.line_content.is_empty() || self.line_start.is_none() {
            self.stale = false;
            return;
        }
        let Some(diff_file) = diff_file else {
            self.stale = true;
            return;
        };
        let anchor = CommentAnchor {
            hunk_index: self.hunk_index,
            line_start: self.line_start,
            line_content: self.line_content.clone(),
            context_before: Vec::new(),
            context_after: Vec::new(),
            old_line_start: None,
            hunk_header: String::new(),
        };
        self.stale = matches!(relocate_comment(&anchor, diff_file), RelocationResult::Lost);
    }

    /// The full set of raisers, falling back to `lens` for a finding written
    /// before `raised_by` existed. Never empty when the finding is attributed.
    pub fn raisers(&self) -> Vec<&str> {
        if self.raised_by.is_empty() {
            if self.lens.is_empty() {
                Vec::new()
            } else {
                vec![self.lens.as_str()]
            }
        } else {
            self.raised_by.iter().map(String::as_str).collect()
        }
    }

    /// The producers worth naming on a row: every raiser except the `general`
    /// fallback, which is noise on the general review's own findings.
    pub fn named_raisers(&self) -> Vec<&str> {
        self.raisers()
            .into_iter()
            .filter(|raiser| *raiser != GENERAL_LENS)
            .collect()
    }

    /// Compact `producers · category` tag for a finding row.
    ///
    /// Both are displayed (see CONTEXT.md), and either is skipped when empty. A
    /// `general` raiser is left out — it is the fallback producer, and naming it
    /// on every row of the general review is noise. So a general finding shows
    /// its defect kind, an expert finding shows `security · correctness`, and a
    /// claim two experts both found shows `reliability, security · correctness`.
    ///
    /// A repeated value collapses to one: sidecars written before the two fields
    /// were separated stored the producer in `category`, so an old
    /// `professor.json` would otherwise read `professor · professor`. The stored
    /// `category` is left as it was — nothing can tell such a value apart from a
    /// finding genuinely categorised `professor`, and dropping a real defect kind
    /// costs more than showing one twice.
    pub fn lens_category_tag(&self) -> String {
        // Every raiser, not just the one it is filed under: a claim three experts
        // independently found reads as such on the row, which is the whole point
        // of merging them.
        let producers = self.named_raisers().join(", ");
        let collapsed = if self.lens == self.category {
            ""
        } else {
            self.category.as_str()
        };
        match (producers.is_empty(), collapsed.is_empty()) {
            (false, false) => format!("{producers} · {collapsed}"),
            (false, true) => producers,
            (true, false) => collapsed.to_string(),
            (true, true) => String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AiResponse {
    pub id: String,
    #[serde(default)]
    pub in_reply_to: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub new_findings: Vec<String>,
}
