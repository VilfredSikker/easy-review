//! What `annotations(tab)` reads beside the AI state: the finding promotions, whether
//! the review file exists, and what the other view's bucket holds. The load and reload
//! effects read them under the AI section's stamp, so its revision covers them too.

use std::collections::HashMap;

use er_engine::ai;
use er_engine::file_rows::active_finding_count;
use er_engine::tab_identity::{DiffMode, ResolvedTab};

#[derive(Clone, Debug, Default)]
pub struct AiExtras {
    /// Finding id to the GitHub comment it was promoted to.
    pub promotions: HashMap<String, String>,
    pub has_review_json: bool,
    pub other_bucket: Option<OtherBucketFacts>,
}

/// What a local PR tab's other view holds, read from that view's bucket.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OtherBucketFacts {
    /// `pr` or `branch`.
    pub view: &'static str,
    pub triage_hash: Option<String>,
    pub review_hash: Option<String>,
    /// Its open findings, counted as its own list counts them.
    pub finding_count: usize,
    /// The diff hash, creation time and pillar count of its tour.
    pub tour: Option<(String, String, usize)>,
}

/// Reads the extras of a tab showing `er_dir`, whose GitHub comments live in
/// `comments_dir`. Runs on an effect thread.
pub(crate) fn read_extras(
    tab: &ResolvedTab,
    git: &dyn er_git::Git,
    er_dir: &str,
    comments_dir: &str,
) -> AiExtras {
    AiExtras {
        promotions: ai::load_finding_promotions(er_dir),
        has_review_json: ai::review_file_exists(er_dir),
        other_bucket: other_bucket_dir(tab, git, er_dir, comments_dir)
            .map(|(view, dir)| other_bucket_facts(view, &dir)),
    }
}

/// The other view's bucket of a local PR tab in its branch, tour or PR view.
/// The working-tree views have buckets of their own that describe another
/// diff, and a remote tab has only the PR bucket.
fn other_bucket_dir(
    tab: &ResolvedTab,
    git: &dyn er_git::Git,
    er_dir: &str,
    comments_dir: &str,
) -> Option<(&'static str, String)> {
    let id = tab.identity();
    let shows_a_pr_view = matches!(
        tab.mode,
        DiffMode::PrDiff | DiffMode::Branch | DiffMode::Tour
    );
    if id.is_remote() || tab.pr_number.is_none() || !shows_a_pr_view {
        return None;
    }
    let (view, dir) = if id.tour_context_is_pr() {
        ("branch", id.branch_bucket_er_dir(git)?)
    } else {
        ("pr", comments_dir.to_string())
    };
    (dir != er_dir).then_some((view, dir))
}

fn other_bucket_facts(view: &'static str, dir: &str) -> OtherBucketFacts {
    // Loaded as of the bucket's own generation, so the loader merges the
    // expert and professor sidecars its own view would.
    let overlay = ai::arbiter_overlay_stamp(dir);
    let state =
        ai::load_ai_state_with_arbiter(dir, &generation_hash(dir), None, overlay.as_deref());
    let tour = ai::load_tour_sidecar(dir, "tour.json");
    OtherBucketFacts {
        view,
        triage_hash: state.triage.as_ref().map(|t| t.diff_hash.clone()),
        review_hash: state.review.as_ref().map(|r| r.diff_hash.clone()),
        finding_count: active_finding_count(&state, None),
        tour: tour.map(|t| (t.diff_hash, t.created_at, t.pillars.len())),
    }
}

/// The diff hash a bucket's sidecars were generated against: `review.json`'s,
/// else the newest expert, professor or triage sidecar's.
fn generation_hash(dir: &str) -> String {
    let review = ai::load_review(dir).map(|review| review.diff_hash);
    if let Some(hash) = review.filter(|hash| !hash.is_empty()) {
        return hash;
    }
    let experts = ai::load_expert_reviews(dir)
        .into_iter()
        .map(|e| (e.created_at, e.diff_hash));
    let professor = ai::load_professor_review(dir).map(|p| (p.created_at, p.diff_hash));
    let triage = ai::load_triage_review(dir).map(|t| (t.created_at, t.diff_hash));
    experts
        .chain(professor)
        .chain(triage)
        .filter(|(_, hash)| !hash.is_empty())
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, hash)| hash)
        .unwrap_or_default()
}
