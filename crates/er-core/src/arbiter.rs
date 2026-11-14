use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::artifact::Artifact;
use crate::review::{AiResponse, Confidence, ErReview, Finding};

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

/// Whether an `AiResponse` is an arbiter ruling rather than a validation reply.
///
/// `regrade_response` assigns the `arbiter-` prefix, and the desktop reads that
/// prefix to tag a finding's response trail, so the format is a contract
/// between the crates. `a_regrade_is_tagged_as_the_arbiters_own` pins it, and
/// it is a predicate rather than a sentence match so no reader has to parse
/// what the arbiter happened to write.
pub fn is_arbiter_ruling(response: &AiResponse) -> bool {
    response.id.starts_with("arbiter-")
}

/// Apply `arbiter` to `review` in place.
///
/// Matching runs on the same key `arena::seeded` grouped on —
/// `sha1(file + line + canonical(title))`. An unchanged claim at an unchanged
/// anchor keeps its verdict across re-runs; a reworded claim, or one whose
/// anchor moved, gets a new key and its old verdict is orphaned rather than
/// applied to something the arbiter never read.
///
/// A verdict is only applied when the arbiter's `diff_hash` is the diff the
/// review was written against — grading a diff that has since moved would be
/// the same theatre as gating on an unverified self-report.
pub fn merge_arbiter_into_review(review: &mut ErReview, arbiter: &ArbiterReview) -> ArbiterEffect {
    let mut effect = ArbiterEffect::default();
    if arbiter.diff_hash != review.diff_hash {
        // Not silently: every verdict is reported as unapplied, because the
        // grades exist but describe a diff this review is not.
        effect.unmatched = arbiter.verdicts.len();
        return effect;
    }

    let by_id: HashMap<&str, &ArbiterVerdict> = arbiter
        .verdicts
        .iter()
        .map(|v| (v.id.as_str(), v))
        .collect();

    let mut matched: HashSet<&str> = HashSet::new();
    for (path, file_review) in review.files.iter_mut() {
        for finding in &mut file_review.findings {
            let id = super::identity::finding_key(path, finding.line_start, &finding.title);
            let Some(verdict) = by_id.get(id.as_str()) else {
                continue;
            };
            // Two rows can share a key — the expert merge does not dedupe, so a
            // claim two experts raised is two review rows. The verdict applies to
            // both so the display stays consistent, but it is *one* ruling, and
            // the counters say so rather than reporting it twice.
            let first_application = matched.insert(verdict.id.as_str());
            apply_verdict(finding, verdict, &mut effect, first_application);
        }
    }
    // A verdict that matched nothing is the tell that the two sides disagree
    // about the diff or the keys — the pass did nothing, and silence would let
    // that read as "everything was fine". Counted over the verdicts as written,
    // so a duplicated id in the file cannot flatter the total.
    effect.unmatched = arbiter
        .verdicts
        .iter()
        .filter(|v| !matched.contains(v.id.as_str()))
        .count();
    effect
}

fn apply_verdict(
    finding: &mut Finding,
    verdict: &ArbiterVerdict,
    effect: &mut ArbiterEffect,
    first_application: bool,
) {
    // The raiser set is the arbiter's, because it is the thing that saw every
    // producer's version of the claim and merged them.
    if !verdict.raised_by.is_empty() {
        finding.raised_by = verdict.raised_by.clone();
        if let Some(primary) = verdict.raised_by.first() {
            if !verdict.raised_by.contains(&finding.lens) {
                finding.lens = primary.clone();
            }
        }
    }
    if !verdict.providers.is_empty() {
        finding.providers = verdict.providers.clone();
    }

    // A ruling that hides the finding settles its confidence outright, so the
    // regrade is skipped: recording "regraded from tentative to confirmed" on a
    // finding the arbiter then dropped would describe a grade nothing reads.
    // `Confidence::Dropped` is also what `is_active()` reads, so a ruling the
    // reviewer should stop acting on has to land there whichever it was.
    match verdict.verdict {
        ArbiterRuling::Dropped => {
            finding.confidence = Confidence::Dropped;
            if first_application {
                effect.dropped += 1;
            }
            return;
        }
        ArbiterRuling::Merged => {
            finding.confidence = Confidence::Dropped;
            if first_application {
                effect.merged += 1;
            }
            return;
        }
        ArbiterRuling::Kept | ArbiterRuling::Escalated => {
            if first_application {
                effect.kept += 1;
            }
        }
    }

    // The producer's original grade goes into the response trail rather than
    // being overwritten, so the disagreement between the two stays readable.
    if let Some(confidence) = verdict.confidence {
        if confidence != finding.confidence {
            finding
                .responses
                .push(regrade_response(finding, confidence, verdict));
            finding.confidence = confidence;
            if first_application {
                effect.regraded += 1;
            }
        }
    }
}

/// The response row a regrade appends. Its `arbiter-` prefix is the tag the
/// desktop reads to recognise the arbiter's own reply, so the format is a
/// cross-crate contract rather than a private detail.
pub fn regrade_response(
    finding: &Finding,
    confidence: Confidence,
    verdict: &ArbiterVerdict,
) -> AiResponse {
    let mut text = format!(
        "Arbiter regraded this from {} to {}",
        confidence_name(finding.confidence),
        confidence_name(confidence)
    );
    if !verdict.rationale.trim().is_empty() {
        text.push_str(": ");
        text.push_str(verdict.rationale.trim());
    }
    AiResponse {
        id: format!("arbiter-{}", finding.responses.len() + 1),
        in_reply_to: String::new(),
        timestamp: String::new(),
        text,
        new_findings: Vec::new(),
    }
}

const fn confidence_name(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Confirmed => "confirmed",
        Confidence::Tentative => "tentative",
        Confidence::Informational => "informational",
        Confidence::Dropped => "dropped",
    }
}
