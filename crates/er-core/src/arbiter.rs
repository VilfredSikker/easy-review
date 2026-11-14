use serde::{Deserialize, Serialize};

use crate::artifact::Artifact;
use crate::review::Confidence;

/// `.er/arbiter.json`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ArbiterReview {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub created_at: String,
    /// The arena run these verdicts came from, when there was one.
    #[serde(default)]
    pub run_id: String,
    #[serde(default, deserialize_with = "super::review::lenient_verdicts")]
    pub verdicts: Vec<ArbiterVerdict>,
}

/// What the arbiter decided about one finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ArbiterRuling {
    Kept,
    Escalated,
    /// Folded into another finding, which carries the claim from here on.
    Merged,
    /// The arbiter could not substantiate it.
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ArbiterVerdict {
    /// Content-addressed, so a verdict cannot attach to a different claim
    /// wearing a recycled positional id.
    pub id: String,
    #[serde(default)]
    pub file: String,
    pub verdict: ArbiterRuling,
    /// The arbiter's own grade, replacing the producer's self-report.
    #[serde(default)]
    pub confidence: Option<Confidence>,
    /// The surviving finding, when this one was merged away.
    #[serde(default)]
    pub merged_into: Option<String>,
    #[serde(default)]
    pub rationale: String,
    /// Every lens that raised this claim, so a finding several experts found
    /// reads as one row from all of them rather than one row from the first.
    #[serde(default)]
    pub raised_by: Vec<String>,
    /// Host-derived providers responsible for the surviving finding.
    #[serde(default)]
    pub providers: Vec<String>,
}

impl Artifact for ArbiterReview {
    const KIND: &'static str = "arbiter";
    const VERSION: u32 = 1;
    const FILE: &'static str = "arbiter.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

/// What applying the overlay did, so the UI can say so rather than silently
/// showing fewer findings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArbiterEffect {
    /// Findings the arbiter ruled out, now hidden.
    pub dropped: usize,
    /// Findings folded into another, now hidden.
    pub merged: usize,
    /// Findings whose confidence the arbiter regraded.
    pub regraded: usize,
    /// Findings the arbiter looked at and let stand.
    ///
    /// Counted even though nothing about those findings changed: it is the only
    /// evidence that a pass ran at all when the arbiter agreed with every
    /// producer, and the confidence gate tightens on whether the diff has been
    /// graded rather than on how many findings moved.
    pub kept: usize,
    /// Verdicts that matched no finding. Non-zero means the file and the review
    /// disagree — a stale hash, or keys that moved — and the pass did nothing.
    /// Without this, a silent no-op looks exactly like a clean result.
    pub unmatched: usize,
}

impl ArbiterEffect {
    pub const fn hidden(&self) -> usize {
        self.dropped + self.merged
    }

    /// Whether an arbiter has graded anything on this diff.
    ///
    /// `kept` counts: a pass that affirmed every finding still read the code,
    /// which is the difference the confidence gate is about. One definition, so
    /// the gate and anything that explains the gate cannot drift apart.
    pub const fn graded(&self) -> bool {
        self.dropped + self.merged + self.regraded + self.kept > 0
    }
}
