//! `arbiter.json` — an arbiter's verdicts over findings, merged into the review
//! at load time.
//!
//! A verdict is an opinion *about* a finding, so it is stored apart from one
//! (see CONTEXT.md). The arbiter writes this file and nothing else; the review
//! keeps its existing writers, and the overlay below is what makes the grades
//! visible. That also makes re-running the arbiter idempotent: the same
//! verdicts applied twice leave the same review.

use super::review::{AiResponse, Confidence, ErReview, Finding};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// `.er/arbiter.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArbiterRuling {
    Kept,
    Escalated,
    /// Folded into another finding, which carries the claim from here on.
    Merged,
    /// The arbiter could not substantiate it.
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

const MAX_SIDECAR_BYTES: u64 = 10_000_000;

/// Read `<er_dir>/arbiter.json`. Absent or malformed reads as "no verdicts".
pub fn load_arbiter_review(er_dir: &str) -> Option<ArbiterReview> {
    let path = std::path::Path::new(er_dir).join("arbiter.json");
    let metadata = std::fs::metadata(&path).ok()?;
    if metadata.len() > MAX_SIDECAR_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Serialize verdicts for `<er_dir>/arbiter.json`, atomically.
pub fn write_arbiter_review(er_dir: &str, review: &ArbiterReview) -> anyhow::Result<()> {
    let path = std::path::Path::new(er_dir).join("arbiter.json");
    let json = serde_json::to_string_pretty(review)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
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
    /// Verdicts that matched no finding. Non-zero means the file and the review
    /// disagree — a stale hash, or keys that moved — and the pass did nothing.
    /// Without this, a silent no-op looks exactly like a clean result.
    pub unmatched: usize,
}

impl ArbiterEffect {
    pub const fn hidden(&self) -> usize {
        self.dropped + self.merged
    }
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

    let mut matched: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (path, file_review) in review.files.iter_mut() {
        for finding in &mut file_review.findings {
            let id = crate::arena::finding_key(path, finding.line_start, &finding.title);
            let Some(verdict) = by_id.get(id.as_str()) else {
                continue;
            };
            matched.insert(verdict.id.as_str());
            apply_verdict(finding, verdict, &mut effect);
        }
    }
    // A verdict that matched nothing is the tell that the two sides disagree
    // about the diff or the keys — the pass did nothing, and silence would let
    // that read as "everything was fine".
    effect.unmatched = by_id.len() - matched.len();
    effect
}

fn apply_verdict(finding: &mut Finding, verdict: &ArbiterVerdict, effect: &mut ArbiterEffect) {
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

    // The producer's original grade goes into the response trail rather than
    // being overwritten, so the disagreement between the two stays readable.
    if let Some(confidence) = verdict.confidence {
        if confidence != finding.confidence {
            finding
                .responses
                .push(regrade_response(finding, confidence, verdict));
            finding.confidence = confidence;
            effect.regraded += 1;
        }
    }

    // `Confidence::Dropped` is what `is_active()` reads, so a ruling the
    // reviewer should stop acting on has to land there whichever it was.
    match verdict.verdict {
        ArbiterRuling::Dropped => {
            finding.confidence = Confidence::Dropped;
            effect.dropped += 1;
        }
        ArbiterRuling::Merged => {
            finding.confidence = Confidence::Dropped;
            effect.merged += 1;
        }
        ArbiterRuling::Kept | ArbiterRuling::Escalated => {}
    }
}

fn regrade_response(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{ErFileReview, RiskLevel};
    use crate::arena::finding_key;
    use std::collections::HashMap;

    const HASH: &str = "diff-hash";
    const TITLE: &str = "unchecked user input";

    /// Anchored at line 10, which `verdict()` keys on — the anchor is part of
    /// the key, so a fixture without one would never match a verdict.
    fn finding(confidence: Confidence) -> Finding {
        serde_json::from_value(serde_json::json!({
            "id": "f-1",
            "severity": "high",
            "title": TITLE,
            "line_start": 10,
            "confidence": confidence_name(confidence),
        }))
        .expect("a minimal finding deserializes")
    }

    fn review_with(findings: Vec<Finding>) -> ErReview {
        ErReview {
            version: 1,
            diff_hash: HASH.to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: HashMap::from([(
                "src/a.rs".to_string(),
                ErFileReview {
                    risk: RiskLevel::High,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings,
                },
            )]),
            file_hashes: HashMap::new(),
        }
    }

    fn verdict(ruling: ArbiterRuling, confidence: Option<Confidence>) -> ArbiterVerdict {
        ArbiterVerdict {
            id: finding_key("src/a.rs", Some(10), TITLE),
            file: "src/a.rs".to_string(),
            verdict: ruling,
            confidence,
            merged_into: None,
            rationale: "Reproduced against the diff.".to_string(),
            raised_by: Vec::new(),
        }
    }

    fn arbiter(diff_hash: &str, verdicts: Vec<ArbiterVerdict>) -> ArbiterReview {
        ArbiterReview {
            version: 1,
            diff_hash: diff_hash.to_string(),
            created_at: String::new(),
            run_id: "arena-1".to_string(),
            verdicts,
        }
    }

    #[test]
    fn a_regrade_replaces_the_self_report_and_keeps_the_original() {
        let mut review = review_with(vec![finding(Confidence::Confirmed)]);
        let before = review.files["src/a.rs"].findings[0].confidence;

        let effect = merge_arbiter_into_review(
            &mut review,
            &arbiter(
                HASH,
                vec![verdict(ArbiterRuling::Kept, Some(Confidence::Tentative))],
            ),
        );

        let f = &review.files["src/a.rs"].findings[0];
        assert_eq!(f.confidence, Confidence::Tentative);
        assert_eq!(effect.regraded, 1);
        assert!(f.is_active(), "a regrade is not a drop");
        assert_eq!(f.responses.len(), 1, "the producer's grade is recoverable");
        assert!(f.responses[0].text.contains(confidence_name(before)));
        assert!(f.responses[0].text.contains("Reproduced against the diff."));
    }

    #[test]
    fn agreeing_verdicts_leave_the_finding_alone() {
        let mut review = review_with(vec![finding(Confidence::Confirmed)]);

        let effect = merge_arbiter_into_review(
            &mut review,
            &arbiter(
                HASH,
                vec![verdict(ArbiterRuling::Kept, Some(Confidence::Confirmed))],
            ),
        );

        let f = &review.files["src/a.rs"].findings[0];
        assert!(f.responses.is_empty(), "no regrade, no trail");
        assert_eq!(effect, ArbiterEffect::default());
    }

    #[test]
    fn dropped_and_merged_findings_go_inactive_and_are_counted() {
        for (ruling, expected) in [
            (
                ArbiterRuling::Dropped,
                ArbiterEffect {
                    dropped: 1,
                    ..Default::default()
                },
            ),
            (
                ArbiterRuling::Merged,
                ArbiterEffect {
                    merged: 1,
                    ..Default::default()
                },
            ),
        ] {
            let mut review = review_with(vec![finding(Confidence::Confirmed)]);

            let effect =
                merge_arbiter_into_review(&mut review, &arbiter(HASH, vec![verdict(ruling, None)]));

            let f = &review.files["src/a.rs"].findings[0];
            assert!(
                !f.is_active(),
                "{ruling:?} must hide it — `is_active` is what every consumer reads"
            );
            assert_eq!(effect, expected);
            assert_eq!(
                effect.hidden(),
                1,
                "the UI can say how many it is not showing"
            );
        }
    }

    /// The arbiter is what saw every producer's version of a claim, so its
    /// raiser set is the authoritative one.
    #[test]
    fn a_verdict_carries_the_full_raiser_set_onto_the_finding() {
        let mut review = review_with(vec![finding(Confidence::Confirmed)]);
        review.files.get_mut("src/a.rs").unwrap().findings[0].lens = "security".to_string();

        let mut v = verdict(ArbiterRuling::Kept, None);
        v.raised_by = vec!["reliability".to_string(), "security".to_string()];
        merge_arbiter_into_review(&mut review, &arbiter(HASH, vec![v]));

        let f = &review.files["src/a.rs"].findings[0];
        assert_eq!(f.raised_by, vec!["reliability", "security"]);
        assert_eq!(
            f.lens, "security",
            "the lens stays the one it is filed under"
        );
    }

    /// A verdict from before `raised_by` existed leaves the finding's own
    /// attribution alone rather than blanking it.
    #[test]
    fn a_verdict_without_raisers_does_not_clear_the_findings_own() {
        let mut review = review_with(vec![finding(Confidence::Confirmed)]);
        review.files.get_mut("src/a.rs").unwrap().findings[0].raised_by =
            vec!["security".to_string()];

        merge_arbiter_into_review(
            &mut review,
            &arbiter(HASH, vec![verdict(ArbiterRuling::Kept, None)]),
        );

        assert_eq!(
            review.files["src/a.rs"].findings[0].raised_by,
            vec!["security"]
        );
    }

    /// Verdicts are graded against a diff. Applying them to a review written
    /// against a different one would put a second opinion on the wrong code.
    #[test]
    fn verdicts_for_another_diff_are_ignored() {
        let mut review = review_with(vec![finding(Confidence::Confirmed)]);

        let effect = merge_arbiter_into_review(
            &mut review,
            &arbiter(
                "some-other-hash",
                vec![verdict(ArbiterRuling::Dropped, None)],
            ),
        );

        assert_eq!(
            effect.unmatched, 1,
            "the verdict exists but describes another diff — not a silent zero"
        );
        assert!(review.files["src/a.rs"].findings[0].is_active());
    }

    /// An arbiter file whose keys match nothing is reported, not ignored: the
    /// pass did nothing and that looks identical to a clean result otherwise.
    #[test]
    fn verdicts_that_match_no_finding_are_counted() {
        let mut review = review_with(vec![finding(Confidence::Confirmed)]);
        let mut orphan = verdict(ArbiterRuling::Dropped, None);
        orphan.id = finding_key("src/elsewhere.rs", Some(1), "a different claim");

        let effect = merge_arbiter_into_review(&mut review, &arbiter(HASH, vec![orphan]));

        assert_eq!(effect.unmatched, 1);
        assert_eq!(effect.hidden(), 0);
        assert!(review.files["src/a.rs"].findings[0].is_active());
    }

    /// Content-addressing is what keeps a verdict off a different claim wearing
    /// a recycled positional id: reword the claim and the verdict is orphaned.
    #[test]
    fn a_reworded_claim_does_not_inherit_the_verdict() {
        let mut review = review_with(vec![finding(Confidence::Confirmed)]);
        review.files.get_mut("src/a.rs").unwrap().findings[0].title =
            "unchecked user input reaches the cache key".to_string();

        let effect = merge_arbiter_into_review(
            &mut review,
            &arbiter(HASH, vec![verdict(ArbiterRuling::Dropped, None)]),
        );

        assert_eq!(
            effect.unmatched, 1,
            "the verdict is orphaned by the new key, and says so"
        );
        assert!(review.files["src/a.rs"].findings[0].is_active());
    }

    /// Applying the same verdicts twice leaves the same review — re-running the
    /// arbiter must not stack response trail entries.
    #[test]
    fn the_overlay_is_not_accumulative_across_runs() {
        let load = || review_with(vec![finding(Confidence::Confirmed)]);
        let verdicts = || {
            arbiter(
                HASH,
                vec![verdict(ArbiterRuling::Kept, Some(Confidence::Tentative))],
            )
        };

        let mut once = load();
        merge_arbiter_into_review(&mut once, &verdicts());

        let mut twice = load();
        merge_arbiter_into_review(&mut twice, &verdicts());
        merge_arbiter_into_review(&mut twice, &verdicts());

        assert_eq!(
            once.files["src/a.rs"].findings[0].responses.len(),
            twice.files["src/a.rs"].findings[0].responses.len(),
            "the second pass sees an already-graded finding and adds nothing"
        );
    }
}
