use super::diagrams::{
    ErDiagram, DIAGRAM_KIND_FLOWS, DIAGRAM_KIND_MENTAL_MODEL, DIAGRAM_KIND_SUBSYSTEMS,
};
use super::experts::{expert_by_id, load_expert_reviews};
use super::professor::load_professor_review;
use super::review::*;
use super::triage::load_triage_review;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Compute SHA-256 hash of raw diff output (for staleness detection).
/// Used for .er-review.json compatibility where the hash is persisted.
pub fn compute_diff_hash(raw_diff: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_diff.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Compute a fast (non-cryptographic) hash for internal change detection.
///
/// Much faster than SHA-256 — used for detecting if the diff has changed
/// between ticks without the overhead of a full cryptographic hash.
// `DefaultHasher` has no stability guarantee across Rust releases or program runs,
// so this hash is for in-process change detection only — never persist or compare it across processes.
pub fn compute_diff_hash_fast(raw_diff: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    raw_diff.hash(&mut hasher);
    hasher.finish()
}

/// Split a combined diff into per-file sections and hash each one.
/// Returns a map of file path → SHA-256 hash of that file's diff section.
pub fn compute_per_file_hashes(raw_diff: &str) -> HashMap<String, String> {
    section_hashes(raw_diff, None)
}

/// Per-file hashes for the named paths only.
///
/// Same sectioning and path extraction as [`compute_per_file_hashes`], but a
/// section whose file is not wanted is skipped without being accumulated or
/// hashed. That is the point: a watch-event refresh only needs hashes for the
/// files whose review state something consults without a user action, and
/// hashing the rest is the cost this removes.
///
/// Hashes for a wanted path are identical to [`compute_per_file_hashes`].
pub fn compute_per_file_hashes_for(
    raw_diff: &str,
    wanted: &HashSet<String>,
) -> HashMap<String, String> {
    section_hashes(raw_diff, Some(wanted))
}

/// Hash of a single file's diff section, or `None` when the diff has no such
/// file. Used to resolve a file's hash at the moment it is marked reviewed,
/// after the watch path has stopped caching hashes for every file.
pub fn compute_per_file_hash(raw_diff: &str, path: &str) -> Option<String> {
    let mut wanted = HashSet::new();
    wanted.insert(path.to_string());
    section_hashes(raw_diff, Some(&wanted)).remove(path)
}

/// One pass over the diff. `None` hashes every section; `Some(set)` hashes only
/// those paths, leaving the others unaccumulated as well as unhashed.
fn section_hashes(raw_diff: &str, wanted: Option<&HashSet<String>>) -> HashMap<String, String> {
    let mut hashes = HashMap::new();
    if matches!(wanted, Some(w) if w.is_empty()) {
        return hashes;
    }
    let mut current_file: Option<String> = None;
    let mut current_section = String::new();
    let mut current_wanted = false;

    for line in raw_diff.lines() {
        if line.starts_with("diff --git a/") {
            // Flush previous section
            if let Some(file) = current_file.take() {
                if current_wanted {
                    hashes.insert(file, compute_diff_hash(&current_section));
                }
            }
            // Parse file path from "diff --git a/path b/path".
            // For renames ("diff --git a/old.rs b/new.rs") this extracts the old
            // path, so per-file staleness lookups keyed by the new path miss
            // renamed files.
            let path = line
                .strip_prefix("diff --git a/")
                .and_then(|rest| rest.split(" b/").next())
                .unwrap_or("")
                .to_string();
            current_wanted = wanted.is_none_or(|w| w.contains(&path));
            current_file = Some(path);
            current_section.clear();
            if current_wanted {
                current_section.push_str(line);
                current_section.push('\n');
            }
        } else if current_file.is_some() && current_wanted {
            current_section.push_str(line);
            current_section.push('\n');
        }
    }

    // Flush last section
    if let Some(file) = current_file {
        if current_wanted {
            hashes.insert(file, compute_diff_hash(&current_section));
        }
    }

    hashes
}

/// True when `stored` names the same branch as `expected` (exact or slug match).
///
/// The slug carries a hash of the raw name, so two branches that only slugify
/// alike (`feat/x` and `feat-x`) are different folders and do not match.
pub fn storage_branches_match(expected: &str, stored: &str) -> bool {
    if expected == stored {
        return true;
    }
    crate::storage::slug_branch(expected) == crate::storage::slug_branch(stored)
}

/// Branch name from AI-generated `summary.md` titles (`# Branch Review: …`).
pub fn summary_declares_branch(summary: &str) -> Option<String> {
    for line in summary.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("# Branch Review:")
            .or_else(|| trimmed.strip_prefix("## Branch Review:"))
            .map(str::trim);
        if let Some(branch) = rest {
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }
    }
    None
}

/// True when a branch declaration carries real information. Review agents
/// that can't determine the branch (prepared-diff reviews of remote PRs)
/// write "unknown" or leave the prompt template's `<head branch if known>`
/// placeholder, and a PR tab whose head is still unknown labels itself
/// `pr/<N>` — none of these identifies another branch, so none may
/// disqualify a sidecar, whether it appears as the stored `head_branch` or
/// as the tab's scope.
fn branch_declaration_is_real(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.eq_ignore_ascii_case("unknown")
        && !branch.starts_with('<')
        && !crate::storage::is_pr_placeholder_branch(branch)
}

/// Read `summary.md` through the store's plain-text path.
///
/// The kind carries no envelope: the store adds the path, the size guard and
/// the atomic write, and the bytes stay what the agent wrote.
fn load_summary(er_dir: &str) -> Option<String> {
    if er_dir.is_empty() {
        return None;
    }
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(er_dir).ok()?;
    let file = er_store::RelPath::new("summary.md").ok()?;
    match store.read_text(&scope, &file).ok()? {
        er_store::Loaded::Found(stored) => Some(stored.value),
        _ => None,
    }
}

/// True when sidecars in `er_dir` clearly belong to another branch than `scope`.
pub fn artifacts_branch_mismatch(er_dir: &Path, scope: &str) -> bool {
    if !branch_declaration_is_real(scope) {
        return false;
    }
    if let Some(review) = er_dir
        .to_str()
        .and_then(|dir| crate::store_registry::read_found::<ErReview>(dir, ()))
    {
        if branch_declaration_is_real(&review.head_branch)
            && !storage_branches_match(scope, &review.head_branch)
        {
            return true;
        }
    }
    if let Some(content) = er_dir.to_str().and_then(load_summary) {
        if let Some(declared) = summary_declares_branch(&content) {
            if branch_declaration_is_real(&declared) && !storage_branches_match(scope, &declared) {
                return true;
            }
        }
    }
    false
}

/// Load all .er-* files from the er_dir and check staleness against current diff hash.
///
/// When `branch_scope` is set (viewed branch or checkout), `review.json` with a
/// non-empty `head_branch` that does not match is ignored — including `summary.md`
/// and other review artifacts from the same directory. This avoids showing a prior
/// branch's review after mistaken migration into the wrong managed folder.
pub fn load_ai_state(er_dir: &str, current_diff_hash: &str, branch_scope: Option<&str>) -> AiState {
    let overlay = super::arbiter::arbiter_overlay_stamp(er_dir);
    load_ai_state_with_arbiter(er_dir, current_diff_hash, branch_scope, overlay.as_deref())
}

/// Load the AI state with a captured overlay, tying the loaded value to its stamp.
pub fn load_ai_state_with_arbiter(
    er_dir: &str,
    current_diff_hash: &str,
    branch_scope: Option<&str>,
    overlay: Option<&[u8]>,
) -> AiState {
    let mut state = AiState::default();
    let er_path = Path::new(er_dir);

    if let Some(scope) = branch_scope {
        if artifacts_branch_mismatch(er_path, scope) {
            return state;
        }
    }

    // Load .er/review.json. A sidecar that cannot be used reads as absent.
    // The projection below owns the lens backfill and the expert/professor
    // merges; the review's staleness contribution comes from the projection.
    let mut general: Option<ErReview> = None;
    if let Some(review) = crate::store_registry::read_found::<ErReview>(er_dir, ()) {
        general = Some(review);
    }
    // A completed scoped run overlays only its paths; the base identity it
    // stamped decides freshness.
    if let Some(overlay) =
        crate::store_registry::read_found::<super::scoped_overlay::ScopedReview>(er_dir, ())
    {
        super::scoped_overlay::apply_scoped_review(&mut general, &overlay);
    }

    // A validation matching the current diff refreshes a stale review in place.
    // Moving the hash here, before the expert/professor filters below, is what
    // drops old-generation sidecars exactly as the old rewrite did.
    let validation =
        crate::store_registry::read_found::<super::validation::ValidationReview>(er_dir, ());
    if let (Some(general), Some(validation)) = (general.as_mut(), validation.as_ref()) {
        if validation.diff_hash == current_diff_hash
            && validation.review_created_at == general.created_at
            && general.diff_hash != current_diff_hash
        {
            general.diff_hash = current_diff_hash.to_string();
        }
    }

    // The store-backed sidecars share one scope; an invalid directory skips them.
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(er_dir).ok();

    // Load order.json through the store.
    if let Some(scope) = &scope {
        if let Ok(er_store::Loaded::Found(stored)) =
            super::checklist_store::load_order(&store, scope)
        {
            let order = stored.value;
            // Check staleness against review hash or independently
            if !state.is_stale && order.diff_hash != current_diff_hash {
                state.is_stale = true;
            }
            state.order = Some(order);
        }
    }

    // The guided tour (`tour.json`, one per view bucket) is context-scoped (branch
    // vs PR) and is loaded by `TabState::reload_ai_state` → `resolve_view_tour`,
    // which knows the active view's bucket. It is intentionally not loaded here.

    // Load .er/summary.md through the store's plain-text path.
    if let Some(content) = load_summary(er_dir) {
        if !content.trim().is_empty() {
            state.summary = Some(content);
        }
    }

    // Load checklist.json through the store.
    if let Some(scope) = &scope {
        if let Ok(er_store::Loaded::Found(stored)) =
            super::checklist_store::load_checklist(&store, scope)
        {
            let checklist = stored.value;
            if !state.is_stale && checklist.diff_hash != current_diff_hash {
                state.is_stale = true;
            }
            state.checklist = Some(checklist);
        }
    }

    // Load the three annotation sidecars through their single writer.
    if let Some(scope) = &scope {
        if let Ok(er_store::Loaded::Found(stored)) =
            super::annotation_store::load_questions(&store, scope)
        {
            state.questions = Some(stored.value);
        }
        if let Ok(er_store::Loaded::Found(stored)) =
            super::annotation_store::load_notes(&store, scope)
        {
            state.notes = Some(stored.value);
        }
        if let Ok(er_store::Loaded::Found(stored)) =
            super::annotation_store::load_github_comments(&store, scope)
        {
            state.github_comments = Some(stored.value);
        }
    }

    // Expert sidecars from the same generation as a (possibly stale) general
    // review ride along with it — see `expert_hash_accepted`. Their findings
    // are appended by the projection, not here. A completed scoped run
    // overlays only its paths.
    let mut experts = load_expert_reviews(er_dir);
    for overlay in load_scoped_expert_overlays(er_dir) {
        super::scoped_overlay::apply_scoped_expert(&mut experts, &overlay);
    }
    let review_hash = general
        .as_ref()
        .map(|review| review.diff_hash.as_str())
        .unwrap_or("");
    for expert in &experts {
        if !super::experts::expert_hash_accepted(expert, review_hash, current_diff_hash) {
            continue;
        }
        let summary = expert.summary.trim();
        if summary.is_empty() {
            continue;
        }
        if let Some(def) = expert_by_id(&expert.expert_id) {
            state
                .agent_summaries
                .insert(def.label.to_string(), summary.to_string());
        }
    }

    if let Some(triage) = load_triage_review(er_dir) {
        if triage.diff_hash != current_diff_hash {
            state.is_stale = true;
        }
        state.triage = Some(triage);
    }

    let mut professor = load_professor_review(er_dir);
    if let Some(overlay) = crate::store_registry::read_found::<
        super::scoped_overlay::ScopedProfessorReview,
    >(er_dir, ())
    {
        super::scoped_overlay::apply_scoped_professor(&mut professor, &overlay);
    }
    if let Some(prof) = professor.as_ref() {
        let summary = prof.summary.trim();
        if prof.diff_hash == current_diff_hash && !summary.is_empty() {
            state.agent_summaries.insert(
                super::professor::PROFESSOR_LABEL.to_string(),
                summary.to_string(),
            );
        }
    }

    // The arbiter overlay is the captured generation the caller stamped, and
    // the store's parser decides whether it is usable, so a stamp taken from a
    // newer version is not applied even if the file changes under it. The
    // projection applies the parsed value last.
    let arbiter = overlay.and_then(|bytes| {
        let value = serde_json::from_slice(bytes).ok()?;
        match er_store::envelope::parse::<super::arbiter::ArbiterReview>(value) {
            er_store::Loaded::Found(stored) => Some(stored.value),
            _ => None,
        }
    });

    let projection =
        super::projection::build_findings_projection(super::projection::ProjectionInputs {
            general,
            experts: &experts,
            professor: professor.as_ref(),
            arbiter: arbiter.as_ref(),
            validation: validation.as_ref(),
            current_diff_hash,
            accepted_provenance: &|review_hash, rows| {
                super::accepted_provenance::accepted_provenance_for_rows(
                    Path::new(er_dir),
                    review_hash,
                    rows,
                )
            },
        });
    for diagnostic in projection.provenance_diagnostics {
        crate::dev_log::arena_line(diagnostic);
    }
    // The review's staleness comes from the projected hash; order, checklist
    // and triage may already have marked the state stale.
    let review_stale = projection
        .review
        .as_ref()
        .is_some_and(|review| review.diff_hash != current_diff_hash);
    state.is_stale |= review_stale;
    state.review = projection.review;
    state.arbiter_effect = projection.arbiter_effect;
    state.arbiter_graded_keys = projection.arbiter_graded_keys;
    state.arbiter_verdicts = projection.arbiter_verdicts;
    state.validation_effect = projection.validation_effect;

    // Load .er/diagrams/*.json (mermaid diagrams — one file per generated diagram)
    state.diagrams = load_diagrams(er_dir, current_diff_hash);

    state
}

/// Every completed scoped expert overlay in the bucket, in slot order.
fn load_scoped_expert_overlays(er_dir: &str) -> Vec<super::scoped_overlay::ScopedExpertReview> {
    if er_dir.is_empty() {
        return Vec::new();
    }
    let store = crate::store_registry::store();
    let Ok(scope) = er_store::Scope::dir(er_dir) else {
        return Vec::new();
    };
    let Ok(slots) = store.list::<super::scoped_overlay::ScopedExpertReview>(&scope) else {
        return Vec::new();
    };
    slots
        .into_iter()
        .filter_map(|slot| {
            crate::store_registry::read_found::<super::scoped_overlay::ScopedExpertReview>(
                er_dir, slot,
            )
        })
        .collect()
}

/// Read and parse the guided-tour sidecar from a directory.
///
/// The store owns `tour.json`; any other name reads as absent. The name
/// argument stays until its callers in the desktop and sidecar summaries are
/// ready to drop it.
pub fn load_tour_sidecar(dir: &str, name: &str) -> Option<ErTour> {
    if name != "tour.json" {
        return None;
    }
    crate::store_registry::read_found::<ErTour>(dir, ())
}

/// Load all mermaid diagrams from `{er_dir}/diagrams/*.json`.
///
/// Each diagram is a standalone file (written one-per-run by the host), so
/// unreadable or malformed files are skipped rather than failing the whole
/// set. The `id` is the store slot and `stale` is computed against the current
/// diff hash. A file whose stem is not a valid diagram id is not a diagram and
/// never loads. Ordering: presets in their fixed display order, then customs
/// newest-first.
pub fn load_diagrams(er_dir: &str, current_diff_hash: &str) -> Vec<ErDiagram> {
    if er_dir.is_empty() {
        return Vec::new();
    }
    let store = crate::store_registry::store();
    let Ok(scope) = er_store::Scope::dir(er_dir) else {
        return Vec::new();
    };
    let Ok(slots) = store.list::<ErDiagram>(&scope) else {
        return Vec::new();
    };
    let mut out: Vec<ErDiagram> = Vec::new();
    for slot in slots {
        let Some(mut diagram) =
            crate::store_registry::read_found::<ErDiagram>(er_dir, slot.clone())
        else {
            continue;
        };
        if diagram.mermaid.trim().is_empty() {
            continue;
        }
        diagram.id = slot.as_str().to_string();
        diagram.stale = diagram.diff_hash != current_diff_hash;
        out.push(diagram);
    }
    out.sort_by(|a, b| {
        let rank = |d: &ErDiagram| match d.kind.as_str() {
            k if k == DIAGRAM_KIND_MENTAL_MODEL => 0,
            k if k == DIAGRAM_KIND_SUBSYSTEMS => 1,
            k if k == DIAGRAM_KIND_FLOWS => 2,
            _ => 3,
        };
        rank(a)
            .cmp(&rank(b))
            // Newest first within a rank (created_at is ISO 8601).
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{write_sidecar_json, write_sidecar_json_slot};
    use serde_json::json;

    #[test]
    fn load_ai_state_ignores_feedback_json() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            std::fs::write(
            dir.path().join("feedback.json"),
            r#"{"version":1,"diff_hash":"h","comments":[{"id":"fb-1","file":"a.rs","hunk_index":0,"comment":"old"}]}"#,
        )
        .unwrap();
            let state = load_ai_state(er_dir, "h", None);
            assert!(state.find_comment("fb-1").is_none());
            assert!(state.comments_for_hunk("a.rs", 0).is_empty());
        });
    }

    #[test]
    fn producer_provider_metadata_is_cleared_and_accepted_run_restores_it() {
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();
        fn project(review: ErReview, er_dir: &str) -> ErReview {
            let mut projection = crate::ai::projection::build_findings_projection(
                crate::ai::projection::ProjectionInputs {
                    general: Some(review),
                    experts: &[],
                    professor: None,
                    arbiter: None,
                    validation: None,
                    current_diff_hash: "hash",
                    accepted_provenance: &|review_hash, rows| {
                        crate::ai::accepted_provenance::accepted_provenance_for_rows(
                            std::path::Path::new(er_dir),
                            review_hash,
                            rows,
                        )
                    },
                },
            );
            projection.review.take().expect("a projected review")
        }
        let review: ErReview = serde_json::from_value(json!({
            "version": 1,
            "diff_hash": "hash",
            "files": { "src/a.rs": { "risk": "high", "findings": [{
                "id": "arena-run-1-f1", "severity": "high", "title": "claim",
                "line_start": 9, "lens": "general", "raised_by": ["general"],
                "providers": ["producer-forgery"]
            }] } }
        }))
        .unwrap();
        let run_dir = dir.path().join("arena/run-1");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("run.json"),
            serde_json::to_vec(&json!({
                "id": "run-1", "diff_hash": "hash",
                "reviewers": [{ "id": "reviewer-security", "provider_id": "provider-a", "agent_kind": "expert:security" }],
                "findings": [{ "id": "f1", "file": "src/a.rs", "line": 9, "raised_by": ["reviewer-security"], "accepted_at": "now" }],
                "accepted_finding_ids": ["f1"]
            }))
            .unwrap(),
        )
        .unwrap();

        let review = project(review, er_dir);
        let finding = &review.files["src/a.rs"].findings[0];
        assert_eq!(finding.providers, ["provider-a"]);
        assert_eq!(finding.raised_by, ["security"]);
        assert_eq!(finding.lens, "security");

        std::fs::remove_dir_all(&run_dir).unwrap();
        let producer_only: ErReview = serde_json::from_value(json!({
            "version": 1, "diff_hash": "hash",
            "files": { "src/a.rs": { "risk": "high", "findings": [{
                "id": "plain", "severity": "high", "title": "claim", "providers": ["forged"]
            }] } }
        }))
        .unwrap();
        let producer_only = project(producer_only, er_dir);
        assert!(producer_only.files["src/a.rs"].findings[0]
            .providers
            .is_empty());
    }

    #[test]
    fn arbiter_verdicts_follow_matching_review_identity_without_line_anchor() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            let key = super::super::identity::finding_key("src/a.rs", None, "claim");
            write_sidecar_json::<ErReview>(
                path,
                json!({
                    "version": 1, "diff_hash": "hash",
                    "files": { "src/a.rs": { "risk": "medium", "findings": [
                        { "id": "f-1", "severity": "medium", "title": "claim", "line_start": null },
                        { "id": "f-2", "severity": "medium", "title": "other", "line_start": null }
                    ] } }
                }),
            );
            let write_arbiter = |hash| {
                write_sidecar_json::<super::super::arbiter::ArbiterReview>(
                    path,
                    json!({
                        "version": 1, "diff_hash": hash, "verdicts": [
                            { "id": key, "verdict": "escalated", "confidence": "informational" },
                            { "id": "orphan", "verdict": "kept" }
                        ]
                    }),
                );
            };
            write_arbiter("hash");
            let state = load_ai_state(path.to_str().unwrap(), "hash", None);
            assert_eq!(state.arbiter_verdicts.len(), 1);
            assert_eq!(
                state
                    .arbiter_verdicts
                    .get(&key)
                    .map(|(verdict, _)| *verdict),
                Some(super::super::arbiter::ArbiterRuling::Escalated)
            );
            write_arbiter("old-hash");
            let stale = load_ai_state(path.to_str().unwrap(), "hash", None);
            assert!(stale.arbiter_verdicts.is_empty());
        });
    }

    #[test]
    fn captured_arbiter_bytes_are_the_bytes_loaded() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_sidecar_json::<ErReview>(
                path,
                json!({"version":1,"diff_hash":"hash","files":{"a.rs":{"risk":"medium","findings":[{"id":"f","severity":"medium","title":"claim"}]}}}),
            );
            let key = super::super::identity::finding_key("a.rs", None, "claim");
            let overlay = |rationale| {
                write_sidecar_json::<super::super::arbiter::ArbiterReview>(
                    path,
                    json!({"version":1,"diff_hash":"hash","verdicts":[{"id": key,"verdict":"escalated","rationale":rationale}]}),
                );
            };
            overlay("A");
            let captured =
                super::super::arbiter::arbiter_overlay_stamp(path.to_str().unwrap()).unwrap();
            overlay("B");
            let state =
                load_ai_state_with_arbiter(path.to_str().unwrap(), "hash", None, Some(&captured));
            assert_eq!(state.arbiter_verdicts.get(&key).unwrap().1, "A");
        });
    }

    #[test]
    fn an_arbiter_from_a_newer_version_is_not_overlaid() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_sidecar_json::<ErReview>(
                path,
                json!({"version":1,"diff_hash":"hash","files":{"a.rs":{"risk":"medium","findings":[{"id":"f","severity":"medium","title":"claim"}]}}}),
            );
            let key = super::super::identity::finding_key("a.rs", None, "claim");
            // A newer version than the kind has, so it cannot go through the store.
            std::fs::write(
            path.join("arbiter.json"),
            serde_json::json!({"version":2,"diff_hash":"hash","verdicts":[{"id": key,"verdict":"dropped","rationale":"no"}]}).to_string(),
        )
        .unwrap();
            let captured =
                super::super::arbiter::arbiter_overlay_stamp(path.to_str().unwrap()).unwrap();

            let state =
                load_ai_state_with_arbiter(path.to_str().unwrap(), "hash", None, Some(&captured));

            assert_eq!(
                state.arbiter_effect,
                crate::ai::ArbiterEffect::default(),
                "a newer arbiter is ignored"
            );
            assert_eq!(
                state.review.expect("the review").files["a.rs"]
                    .findings
                    .len(),
                1
            );
        });
    }

    #[test]
    fn captured_newer_bytes_are_not_overlaid_after_a_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_sidecar_json::<ErReview>(
                path,
                json!({"version":1,"diff_hash":"hash","files":{"a.rs":{"risk":"medium","findings":[{"id":"f","severity":"medium","title":"claim"}]}}}),
            );
            let key = super::super::identity::finding_key("a.rs", None, "claim");
            let arbiter = |version: u32, rationale: &str| serde_json::json!({"version":version,"diff_hash":"hash","verdicts":[{"id": key,"verdict":"dropped","rationale":rationale}]});
            // Version 2 is newer than the kind, so it stays a raw write.
            std::fs::write(
                path.join("arbiter.json"),
                serde_json::to_vec(&arbiter(2, "from-v2")).unwrap(),
            )
            .unwrap();
            let captured =
                super::super::arbiter::arbiter_overlay_stamp(path.to_str().unwrap()).unwrap();
            // The file on disk becomes a valid version 1 before the load runs.
            // The raw write is forced: the store refuses to replace a newer file.
            std::fs::write(
                path.join("arbiter.json"),
                serde_json::to_vec(&arbiter(1, "from-v1")).unwrap(),
            )
            .unwrap();

            let state =
                load_ai_state_with_arbiter(path.to_str().unwrap(), "hash", None, Some(&captured));

            let applied: Vec<&str> = state
                .arbiter_verdicts
                .values()
                .map(|(_, rationale)| rationale.as_str())
                .collect();
            assert!(
                !applied.contains(&"from-v2"),
                "the captured version-2 bytes were overlaid: {applied:?}"
            );
        });
    }

    #[test]
    fn canonical_rows_collapse_only_for_keys_with_matching_arbiter_verdicts() {
        fn load_case(
            arbiter_hash: Option<&str>,
            verdict_target: &str,
        ) -> (tempfile::TempDir, AiState) {
            let dir = tempfile::tempdir().unwrap();
            let state = crate::storage::with_test_storage_root(dir.path(), || {
                let er_dir = dir.path();
                write_sidecar_json::<ErReview>(
                    er_dir,
                    json!({
                        "version": 1,
                        "diff_hash": "hash",
                        "files": { "src/a.rs": { "risk": "high", "findings": [
                            { "id": "general-row", "severity": "high", "title": "same claim", "line_start": 9, "lens": "general", "raised_by": ["general"] },
                            { "id": "security-row", "severity": "high", "title": "same claim", "line_start": 9, "lens": "security", "raised_by": ["security"] },
                            { "id": "unrelated-a", "severity": "high", "title": "unrelated claim", "line_start": 4, "lens": "general", "raised_by": ["general"] }
                        ] } }
                    }),
                );
                if let Some(diff_hash) = arbiter_hash {
                    let (line, title, raised_by, providers) = if verdict_target == "duplicate-b" {
                        (
                            9,
                            "same claim",
                            json!(["general", "security"]),
                            json!(["provider-a", "provider-b"]),
                        )
                    } else {
                        (
                            4,
                            "unrelated claim",
                            json!(["general"]),
                            json!(["provider-a"]),
                        )
                    };
                    let key = super::super::identity::finding_key("src/a.rs", Some(line), title);
                    write_sidecar_json::<super::super::arbiter::ArbiterReview>(
                        er_dir,
                        json!({
                            "version": 1, "diff_hash": diff_hash, "run_id": "run-1",
                            "verdicts": [{ "id": key, "file": "src/a.rs", "verdict": "kept",
                                "confidence": "confirmed", "raised_by": raised_by, "providers": providers }]
                        }),
                    );
                }
                load_ai_state(er_dir.to_str().unwrap(), "hash", None)
            });
            (dir, state)
        }

        let (_valid_dir, valid) = load_case(Some("hash"), "duplicate-b");
        let rows = valid.review.as_ref().unwrap().files["src/a.rs"]
            .findings
            .iter()
            .map(|f| ("src/a.rs", f));
        let visible =
            super::super::review::project_visible_findings(rows, &valid.arbiter_graded_keys);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].1.id, "general-row");
        assert!(visible[0].1.is_security_stage());
        assert_eq!(visible[0].1.raisers(), ["general", "security"]);
        assert_eq!(visible[0].1.providers, ["provider-a", "provider-b"]);

        let (_missing_dir, missing) = load_case(None, "duplicate-b");
        let rows = missing.review.as_ref().unwrap().files["src/a.rs"]
            .findings
            .iter()
            .map(|f| ("src/a.rs", f));
        assert_eq!(
            super::super::review::project_visible_findings(rows, &missing.arbiter_graded_keys)
                .len(),
            3
        );

        let (_stale_dir, stale) = load_case(Some("old-hash"), "duplicate-b");
        let rows = stale.review.as_ref().unwrap().files["src/a.rs"]
            .findings
            .iter()
            .map(|f| ("src/a.rs", f));
        assert_eq!(
            super::super::review::project_visible_findings(rows, &stale.arbiter_graded_keys).len(),
            3
        );

        let (_partial_dir, partial) = load_case(Some("hash"), "unrelated-a");
        assert!(partial.arbiter_effect.graded());
        assert_eq!(partial.arbiter_graded_keys.len(), 1);
        let rows = partial.review.as_ref().unwrap().files["src/a.rs"]
            .findings
            .iter()
            .map(|f| ("src/a.rs", f));
        let visible =
            super::super::review::project_visible_findings(rows, &partial.arbiter_graded_keys);
        assert_eq!(
            visible.len(),
            3,
            "grading unrelated A must not collapse duplicate B"
        );
        assert!(visible
            .iter()
            .any(|(_, finding)| finding.id == "general-row"));
        assert!(visible
            .iter()
            .any(|(_, finding)| finding.id == "security-row"));
    }

    #[test]
    fn storage_branches_match_compares_slugs() {
        assert!(storage_branches_match("main", "main"));
        assert!(!storage_branches_match(
            "claude/dev-5067-remove-async-media-export",
            "dependabot/npm_and_yarn/foo",
        ));
    }

    #[test]
    fn storage_branches_match_keeps_slug_collisions_apart() {
        assert!(!storage_branches_match("feat/x", "feat-x"));
    }

    #[test]
    fn summary_declares_branch_parses_heading() {
        let md = "# Branch Review: claude/dev-5067\n\nBody.";
        assert_eq!(
            summary_declares_branch(md).as_deref(),
            Some("claude/dev-5067")
        );
    }

    #[test]
    fn load_ai_state_ignores_summary_only_wrong_branch() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            std::fs::write(
                dir.path().join("summary.md"),
                "# Branch Review: claude/dev-5067\n\nOld summary.",
            )
            .unwrap();
            let state = load_ai_state(er_dir, "abc", Some("dependabot/npm_and_yarn/foo"));
            assert!(state.review.is_none());
            assert!(state.summary.is_none());
        });
    }

    #[test]
    fn load_ai_state_keeps_review_with_unknown_head_branch() {
        // Regression: a prepared-diff review agent that can't determine the
        // branch writes head_branch "unknown"; the branch-mismatch guard must
        // not discard the review over it.
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let review = serde_json::json!({
                "version": 1,
                "diff_hash": "abc",
                "head_branch": "unknown",
                "files": {
                    "a.rs": {
                        "risk": "low",
                        "findings": [{
                            "id": "f1",
                            "title": "t",
                            "description": "d",
                            "severity": "low",
                            "category": "logic",
                            "hunk_index": 0
                        }]
                    }
                }
            });
            write_sidecar_json::<ErReview>(dir.path(), review);

            let state = load_ai_state(er_dir, "abc", Some("mikkelam/dev-5713-add-outbox"));
            let review = state
                .review
                .expect("review must survive unknown head_branch");
            assert_eq!(review.files["a.rs"].findings.len(), 1);
            assert!(!state.is_stale);
        });
    }

    #[test]
    fn load_ai_state_keeps_review_with_negative_line_anchor() {
        // Real-world failure: the annotated diff tags deleted lines as
        // `[h<N> L-<old>]` and a model copied the negative number into
        // `line_start`. That must not fail deserialization of the whole review,
        // which showed "No findings written" despite a completed run.
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let review = serde_json::json!({
                "version": 1,
                "diff_hash": "abc",
                "files": {
                    "a.rs": {
                        "risk": "medium",
                        "findings": [{
                            "id": "f-1",
                            "title": "deleted-line finding",
                            "description": "d",
                            "severity": "medium",
                            "category": "testing",
                            "hunk_index": 3,
                            "line_start": -494
                        }]
                    }
                }
            });
            write_sidecar_json::<ErReview>(dir.path(), review);

            let state = load_ai_state(er_dir, "abc", None);
            let review = state
                .review
                .expect("review must survive a negative line anchor");
            let finding = &review.files["a.rs"].findings[0];
            // Anchor degrades to hunk level instead of poisoning the parse.
            assert_eq!(finding.line_start, None);
            assert_eq!(finding.hunk_index, Some(3));
            assert!(!state.is_stale);
        });
    }

    #[test]
    fn load_ai_state_skips_malformed_finding_keeps_rest() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let review = serde_json::json!({
                "version": 1,
                "diff_hash": "abc",
                "files": {
                    "a.rs": {
                        "risk": "low",
                        "findings": [
                            {
                                "id": "f-bad",
                                "title": "t",
                                "severity": "catastrophic",
                                "hunk_index": 0
                            },
                            {
                                "id": "f-good",
                                "title": "t",
                                "description": "d",
                                "severity": "low",
                                "category": "logic",
                                "hunk_index": 1,
                                "line_start": 12
                            }
                        ]
                    }
                }
            });
            write_sidecar_json::<ErReview>(dir.path(), review);

            let state = load_ai_state(er_dir, "abc", None);
            let review = state.review.expect("review must survive one bad finding");
            let findings = &review.files["a.rs"].findings;
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].id, "f-good");
            assert_eq!(findings[0].line_start, Some(12));
        });
    }

    #[test]
    fn load_ai_state_keeps_review_with_placeholder_head_branch() {
        // An agent that copies the prompt template literally leaves
        // "<head branch if known>" — also not a real declaration.
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let review = serde_json::json!({
                "version": 1,
                "diff_hash": "abc",
                "head_branch": "<head branch if known>",
                "files": {}
            });
            write_sidecar_json::<ErReview>(dir.path(), review);

            let state = load_ai_state(er_dir, "abc", Some("feature/x"));
            let r = state.review.expect("placeholder-head review should load");
            assert_eq!(r.diff_hash, "abc");
            assert!(r.files.is_empty());
            assert!(!state.is_stale);
        });
    }

    fn write_review_for_head(dir: &std::path::Path, head_branch: &str) {
        let review = serde_json::json!({
            "version": 1,
            "diff_hash": "abc",
            "head_branch": head_branch,
            "files": {
                "tests/test_bots.py": {
                    "risk": "low",
                    "findings": [{
                        "id": "f-1",
                        "title": "Add coverage for superuser branch",
                        "description": "d",
                        "severity": "low",
                        "category": "testing",
                        "hunk_index": 0
                    }]
                }
            }
        });
        write_sidecar_json::<ErReview>(dir, review);
        std::fs::write(dir.join("summary.md"), "Adds the managing group.\n").unwrap();
    }

    #[test]
    fn load_ai_state_keeps_review_when_scope_is_pr_placeholder() {
        // pr-1560: the tab was re-opened from the inbox without a head-branch
        // hint, so its scope was the `pr/1560` label while review.json named
        // the real head. The label is not a branch — it must not disqualify
        // the review (nor the summary, which the early return also dropped).
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            write_review_for_head(dir.path(), "fix/superviewer-manager");

            let state = load_ai_state(dir.path().to_str().unwrap(), "abc", Some("pr/1560"));
            let review = state
                .review
                .expect("placeholder scope must not discard the review");
            assert_eq!(review.files["tests/test_bots.py"].findings.len(), 1);
            assert_eq!(state.summary.as_deref(), Some("Adds the managing group.\n"));
            assert!(!state.is_stale);
        });
    }

    #[test]
    fn load_ai_state_keeps_review_whose_stored_head_is_pr_placeholder() {
        // Mirror case: a review spawned from a placeholder-labelled tab baked
        // `pr/1560` into head_branch; a tab that later knows the real head
        // must still load it.
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            write_review_for_head(dir.path(), "pr/1560");

            let state = load_ai_state(
                dir.path().to_str().unwrap(),
                "abc",
                Some("fix/superviewer-manager"),
            );
            assert!(
                state.review.is_some(),
                "stored pr/<N> label is not a branch"
            );
            assert!(state.summary.is_some());
        });
    }

    #[test]
    fn load_ai_state_merges_experts_sharing_the_stale_review_hash() {
        // Selected-files run (pr-582): review.json + experts/*.json hash the
        // filtered diff, the tab hashes the full diff. The review loads as
        // stale — its experts' findings and summaries must load with it,
        // while an expert from an older generation stays out.
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let review = serde_json::json!({
                "version": 1,
                "diff_hash": "scoped",
                "head_branch": "",
                "files": {}
            });
            write_sidecar_json::<ErReview>(dir.path(), review);
            let expert = |id: &str, hash: &str, summary: &str| {
                serde_json::json!({
                    "version": 1,
                    "expert_id": id,
                    "diff_hash": hash,
                    "summary": summary,
                    "files": {
                        "m.sql": {
                            "findings": [{
                                "id": "1",
                                "title": "t",
                                "description": "d",
                                "severity": "medium",
                                "category": "correctness",
                                "hunk_index": 0
                            }]
                        }
                    }
                })
            };
            write_sidecar_json_slot::<crate::ai::experts::ExpertReview>(
                dir.path(),
                er_store::ExpertId::new("reliability").unwrap(),
                expert("reliability", "scoped", "Rel summary"),
            );
            write_sidecar_json_slot::<crate::ai::experts::ExpertReview>(
                dir.path(),
                er_store::ExpertId::new("security").unwrap(),
                expert("security", "older", "Old summary"),
            );

            let state = load_ai_state(dir.path().to_str().unwrap(), "full", None);
            assert!(
                state.is_stale,
                "filtered-diff hash never matches the full tab diff"
            );
            let review = state.review.expect("review loads even when stale");
            let ids: Vec<&str> = review.files["m.sql"]
                .findings
                .iter()
                .map(|f| f.id.as_str())
                .collect();
            assert_eq!(
                ids,
                vec!["rel-1"],
                "same-generation expert merged, older one skipped"
            );
            let summaries: Vec<&str> = state.agent_summaries.values().map(String::as_str).collect();
            assert!(summaries.contains(&"Rel summary"));
            assert!(!summaries.contains(&"Old summary"));
        });
    }

    #[test]
    fn a_review_json_with_another_diff_hash_loads_as_stale() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let review = serde_json::json!({
                "version": 1,
                "diff_hash": "old",
                "files": {
                    "a.rs": {
                        "risk": "medium",
                        "findings": [{
                            "id": "1",
                            "title": "claim",
                            "severity": "medium",
                            "hunk_index": 0
                        }]
                    }
                }
            });
            write_sidecar_json::<ErReview>(dir.path(), review);

            let state = load_ai_state(dir.path().to_str().unwrap(), "new", None);

            assert!(state.is_stale);
            let review = state.review.expect("a stale review still loads");
            assert_eq!(review.diff_hash, "old");
            assert_eq!(review.files["a.rs"].findings.len(), 1);
        });
    }

    #[test]
    fn a_review_json_with_an_unknown_version_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            std::fs::write(
                dir.path().join("review.json"),
                r#"{"version":2,"diff_hash":"new","files":{}}"#,
            )
            .unwrap();

            let state = load_ai_state(dir.path().to_str().unwrap(), "new", None);

            assert!(
                state.review.is_none(),
                "a newer schema is ignored, never guessed at"
            );
        });
    }

    #[test]
    fn a_malformed_review_json_loads_no_review_and_the_other_kinds_still_load() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            std::fs::write(dir.path().join("review.json"), "{ bad").unwrap();
            write_sidecar_json::<crate::ai::triage::TriageReview>(
                dir.path(),
                json!({"version":1,"diff_hash":"hash","verdict":{"primary":"general"}}),
            );
            write_sidecar_json::<crate::ai::professor::ProfessorReview>(
                dir.path(),
                json!({"version":1,"diff_hash":"hash","summary":"the lesson"}),
            );

            let state = load_ai_state(dir.path().to_str().unwrap(), "hash", None);

            assert!(state.review.is_none(), "a malformed review reads as absent");
            assert!(state.triage.is_some(), "triage still loads");
            assert_eq!(
                state
                    .agent_summaries
                    .get(crate::ai::professor::PROFESSOR_LABEL)
                    .map(String::as_str),
                Some("the lesson"),
                "the professor summary still loads"
            );
        });
    }

    #[test]
    fn a_review_json_over_the_size_limit_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let pad = "x".repeat(er_store::MAX_ARTIFACT_BYTES as usize + 1);
            std::fs::write(
                dir.path().join("review.json"),
                format!(r#"{{"version":1,"diff_hash":"hash","pad":"{pad}"}}"#),
            )
            .unwrap();

            let state = load_ai_state(dir.path().to_str().unwrap(), "hash", None);

            assert!(state.review.is_none(), "an oversized review is ignored");
        });
    }

    #[test]
    fn review_file_written_by_an_agent_loads() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            write_sidecar_json::<ErReview>(
                dir.path(),
                json!({"version":1,"diff_hash":"hash","head_branch":"","files":{"a.rs":{"risk":"medium","findings":[{"id":"1","title":"claim","severity":"medium","hunk_index":0}]}}}),
            );

            let state = load_ai_state(dir.path().to_str().unwrap(), "hash", None);

            let review = state.review.expect("the agent's review loads");
            assert_eq!(review.files["a.rs"].findings.len(), 1);
            assert!(!state.is_stale);
        });
    }

    #[test]
    fn a_malformed_order_json_is_ignored_and_the_other_kinds_still_load() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            std::fs::write(dir.path().join("order.json"), "{ bad").unwrap();
            write_sidecar_json::<ErReview>(
                dir.path(),
                json!({"version": 1, "diff_hash": "abc", "files": {}}),
            );

            let state = load_ai_state(er_dir, "abc", None);
            assert!(state.order.is_none());
            assert!(state.review.is_some());
        });
    }

    #[test]
    fn a_malformed_checklist_json_is_ignored_and_the_other_kinds_still_load() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            std::fs::write(dir.path().join("checklist.json"), "{ bad").unwrap();
            write_sidecar_json::<ErReview>(
                dir.path(),
                json!({"version": 1, "diff_hash": "abc", "files": {}}),
            );

            let state = load_ai_state(er_dir, "abc", None);
            assert!(state.checklist.is_none());
            assert!(state.review.is_some());
        });
    }

    #[test]
    fn load_ai_state_ignores_review_for_wrong_branch_scope() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let review = serde_json::json!({
                "version": 1,
                "diff_hash": "abc",
                "head_branch": "claude/dev-5067",
                "files": {
                    "a.rs": {
                        "risk": "low",
                        "findings": [{
                            "id": "f1",
                            "title": "t",
                            "description": "d",
                            "severity": "low",
                            "category": "logic",
                            "hunk_index": 0
                        }]
                    }
                }
            });
            write_sidecar_json::<ErReview>(dir.path(), review);
            std::fs::write(
                dir.path().join("summary.md"),
                "# Branch Review: claude/dev-5067\n\nOld summary.",
            )
            .unwrap();

            let state = load_ai_state(er_dir, "abc", Some("dependabot/npm_and_yarn/foo"));
            assert!(state.review.is_none());
            assert!(state.summary.is_none());
        });
    }

    /// The spec's "loads with `lens == \"security\"`" bullet, through the read
    /// path rather than a direct call: a sidecar written before `lens` existed
    /// is attributed from the finding id prefix on load.
    #[test]
    fn load_ai_state_backfills_lens_from_the_id_prefix() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let review = serde_json::json!({
                "version": 1,
                "diff_hash": "abc",
                "files": {
                    "a.rs": {
                        "risk": "low",
                        "findings": [
                            { "id": "sec-1", "title": "t", "severity": "high" },
                            { "id": "f-2", "title": "t", "severity": "low" }
                        ]
                    }
                }
            });
            write_sidecar_json::<ErReview>(dir.path(), review);

            let state = load_ai_state(er_dir, "abc", None);

            let findings = &state.review.expect("review loads").files["a.rs"].findings;
            assert_eq!(findings[0].lens, "security", "expert prefix attributes");
            assert_eq!(findings[1].lens, "general", "no prefix is the general pass");
        });
    }

    #[test]
    fn load_tour_sidecar_parses_pillars_and_load_ai_state_ignores_tour() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let tour = serde_json::json!({
                "version": 1,
                "diff_hash": "abc",
                "title": "Tour: foo",
                "overview": "intro",
                "pillars": [{
                    "id": "p-1",
                    "title": "Foundation",
                    "description": "start here",
                    "order": 0,
                    "importance": 90,
                    "foundation": true,
                    "files": [{"path": "src/a.rs", "reason": "core", "finding_ids": ["f-1"]}]
                }]
            });
            write_sidecar_json::<ErTour>(dir.path(), tour);

            // The tour is context-scoped and loaded by TabState (via the size-guarded
            // `load_tour_sidecar`), not by `load_ai_state`.
            let loaded = load_tour_sidecar(er_dir, "tour.json").unwrap();
            assert_eq!(loaded.pillars.len(), 1);
            assert_eq!(loaded.file_pillar("src/a.rs").unwrap().id, "p-1");
            assert_eq!(loaded.diff_hash, "abc");

            // load_ai_state does not load the tour and leaves staleness defaulted.
            let state = load_ai_state(er_dir, "abc", None);
            assert!(!state.has_tour());
            assert!(!state.is_stale);
            assert!(!state.tour_stale);
        });
    }

    fn write_diagram_envelope(dir: &Path, name: &str, hash: &str) {
        let payload = serde_json::json!({
            "version": 1,
            "diff_hash": hash,
            "kind": "flows",
            "title": "title",
            "mermaid": "flowchart TD\nA-->B",
        });
        let envelope = serde_json::json!({
            "kind": "diagram",
            "version": 1,
            "diff_hash": hash,
            "producer": "host",
            "written_at": "2026-11-10T00:00:00Z",
            "payload": payload,
        });
        let diagrams = dir.join("diagrams");
        std::fs::create_dir_all(&diagrams).unwrap();
        std::fs::write(
            diagrams.join(name),
            serde_json::to_string(&envelope).unwrap(),
        )
        .unwrap();
    }

    fn write_tour(dir: &Path, name: &str, value: serde_json::Value) {
        std::fs::write(dir.join(name), serde_json::to_string(&value).unwrap()).unwrap();
    }

    fn a_tour() -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "diff_hash": "abc",
            "title": "Tour: foo",
            "pillars": [{"id": "p-1", "title": "Foundation", "files": []}]
        })
    }

    #[test]
    fn load_tour_sidecar_ignores_any_name_but_tour_json() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            write_sidecar_json::<ErTour>(dir.path(), a_tour());
            // A name that is not the kind's file stays a raw write.
            write_tour(dir.path(), "tour.pr.json", a_tour());

            assert!(load_tour_sidecar(er_dir, "tour.json").is_some());
            assert!(load_tour_sidecar(er_dir, "tour.pr.json").is_none());
        });
    }

    #[test]
    fn a_malformed_tour_json_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            std::fs::write(dir.path().join("tour.json"), "{ not valid json").unwrap();

            assert!(load_tour_sidecar(dir.path().to_str().unwrap(), "tour.json").is_none());
        });
    }

    #[test]
    fn a_tour_json_with_an_unknown_version_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let mut tour = a_tour();
            tour["version"] = serde_json::json!(2);
            write_tour(dir.path(), "tour.json", tour);

            assert!(load_tour_sidecar(dir.path().to_str().unwrap(), "tour.json").is_none());
        });
    }

    #[test]
    fn an_empty_or_whitespace_summary_is_no_summary() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            for body in ["", "   \n\t\n"] {
                std::fs::write(dir.path().join("summary.md"), body).unwrap();
                assert!(
                    load_ai_state(er_dir, "abc", None).summary.is_none(),
                    "{body:?}"
                );
            }
        });
    }

    #[test]
    fn a_summary_over_the_size_limit_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let file = std::fs::File::create(dir.path().join("summary.md")).unwrap();
            file.set_len(er_store::MAX_ARTIFACT_BYTES + 1).unwrap();

            assert!(load_ai_state(dir.path().to_str().unwrap(), "abc", None)
                .summary
                .is_none());
        });
    }

    #[test]
    fn a_malformed_tour_does_not_hide_the_review_or_the_diagrams() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            std::fs::write(dir.path().join("tour.json"), "{ not valid json").unwrap();
            write_sidecar_json::<ErReview>(
                dir.path(),
                json!({"version":1,"diff_hash":"abc","files":{"a.rs":{"risk":"medium","findings":[{"id":"1","title":"claim","severity":"medium","hunk_index":0}]}}}),
            );
            write_diagram_envelope(dir.path(), "flows.json", "abc");

            let state = load_ai_state(er_dir, "abc", None);

            assert_eq!(
                state.review.expect("the review").files["a.rs"]
                    .findings
                    .len(),
                1
            );
            assert_eq!(load_diagrams(er_dir, "abc").len(), 1);
        });
    }

    #[test]
    fn diagram_files_with_an_unsafe_name_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            write_diagram_envelope(dir.path(), "flows.json", "abc");
            write_diagram_envelope(dir.path(), "my diagram.json", "abc");

            let diagrams = load_diagrams(dir.path().to_str().unwrap(), "abc");

            let ids: Vec<&str> = diagrams.iter().map(|d| d.id.as_str()).collect();
            assert_eq!(ids, vec!["flows"]);
        });
    }

    #[test]
    fn er_tour_ordered_pillars_sorts_foundation_then_importance() {
        let tour: ErTour = serde_json::from_value(serde_json::json!({
            "version": 1,
            "diff_hash": "h",
            "pillars": [
                {"id": "b", "title": "B", "order": 0, "importance": 10, "foundation": false},
                {"id": "a", "title": "A", "order": 0, "importance": 50, "foundation": true},
                {"id": "c", "title": "C", "order": 1, "importance": 99, "foundation": true}
            ]
        }))
        .unwrap();
        let ordered: Vec<&str> = tour
            .ordered_pillars()
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        // order 0 foundation first, then order 0 non-foundation, then order 1.
        assert_eq!(ordered, vec!["a", "b", "c"]);
    }

    #[test]
    fn load_diagrams_reads_dir_sets_id_and_stale() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let diagrams_dir = dir.path().join("diagrams");
            std::fs::create_dir_all(&diagrams_dir).unwrap();

            let write = |name: &str, kind: &str, hash: &str, created: &str, mermaid: &str| {
                let payload = serde_json::json!({
                    "version": 1,
                    "diff_hash": hash,
                    "created_at": created,
                    "kind": kind,
                    "title": format!("title-{name}"),
                    "prompt": "",
                    "mermaid": mermaid,
                });
                let envelope = serde_json::json!({
                    "kind": "diagram",
                    "version": 1,
                    "diff_hash": hash,
                    "producer": "host",
                    "written_at": created,
                    "payload": payload,
                });
                std::fs::write(
                    diagrams_dir.join(name),
                    serde_json::to_string(&envelope).unwrap(),
                )
                .unwrap();
            };

            write(
                "flows.json",
                "flows",
                "abc",
                "2026-08-11T10:00:00Z",
                "flowchart TD\nA-->B",
            );
            write(
                "mental-model.json",
                "mental-model",
                "stale-hash",
                "2026-08-11T09:00:00Z",
                "flowchart TD\nC-->D",
            );
            write(
                "custom-1.json",
                "custom",
                "abc",
                "2026-08-11T12:00:00Z",
                "sequenceDiagram\nA->>B: hi",
            );
            // Empty mermaid source and malformed JSON are skipped, not fatal.
            write("empty.json", "custom", "abc", "2026-08-11T13:00:00Z", "  ");
            std::fs::write(diagrams_dir.join("broken.json"), "{not json").unwrap();

            let diagrams = load_diagrams(er_dir, "abc");
            let ids: Vec<&str> = diagrams.iter().map(|d| d.id.as_str()).collect();
            // Presets in fixed display order, then customs; broken/empty skipped.
            assert_eq!(ids, vec!["mental-model", "flows", "custom-1"]);
            assert!(!diagrams[1].stale); // flows matches the current diff hash
            assert!(diagrams[0].stale); // mental-model was generated for another diff
            assert_eq!(load_ai_state(er_dir, "abc", None).diagrams.len(), 3);
        });
    }

    #[test]
    fn compute_diff_hash_empty_string_returns_known_sha256() {
        let hash = compute_diff_hash("");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn compute_diff_hash_is_deterministic() {
        let input = "diff --git a/foo.rs b/foo.rs\n+let x = 1;";
        let first = compute_diff_hash(input);
        let second = compute_diff_hash(input);
        assert_eq!(first, second);
    }

    #[test]
    fn compute_diff_hash_different_inputs_produce_different_hashes() {
        let hash_a = compute_diff_hash("input a");
        let hash_b = compute_diff_hash("input b");
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn compute_diff_hash_non_empty_produces_64_char_hex() {
        let hash = compute_diff_hash("some diff content");
        // Exact value pins the algorithm; a wrong but 64-char hex hash fails.
        assert_eq!(
            hash,
            "b5949b4e084b58626df729a9e6df634b3dbe207d8234f62cec3f368b3b6b99f6"
        );
    }

    // ── compute_per_file_hashes ──

    #[test]
    fn per_file_hashes_empty_diff_returns_empty() {
        let hashes = compute_per_file_hashes("");
        assert!(hashes.is_empty());
    }

    #[test]
    fn per_file_hashes_single_file() {
        let diff = "diff --git a/src/main.rs b/src/main.rs\nindex abc..def 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n+use std::io;\n fn main() {\n }\n";
        let hashes = compute_per_file_hashes(diff);
        assert_eq!(hashes.len(), 1);
        // Exact hash of the single-file diff section; pins the hashing algorithm.
        assert_eq!(
            hashes["src/main.rs"],
            "9792f1716d22c5c9c79af1d08e346e85ebd1970883114f21021ce834019ddf79"
        );
    }

    #[test]
    fn per_file_hashes_multiple_files() {
        let diff = "diff --git a/foo.rs b/foo.rs\n+line1\ndiff --git a/bar.rs b/bar.rs\n+line2\n";
        let hashes = compute_per_file_hashes(diff);
        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains_key("foo.rs"));
        assert!(hashes.contains_key("bar.rs"));
        assert_ne!(hashes["foo.rs"], hashes["bar.rs"]);
    }

    #[test]
    fn per_file_hashes_deterministic() {
        let diff = "diff --git a/x.rs b/x.rs\n+hello\n";
        let first = compute_per_file_hashes(diff);
        let second = compute_per_file_hashes(diff);
        assert_eq!(first, second);
    }

    #[test]
    fn per_file_hashes_changed_content_changes_hash() {
        let diff_v1 = "diff --git a/x.rs b/x.rs\n+version1\n";
        let diff_v2 = "diff --git a/x.rs b/x.rs\n+version2\n";
        let h1 = compute_per_file_hashes(diff_v1);
        let h2 = compute_per_file_hashes(diff_v2);
        assert_ne!(h1["x.rs"], h2["x.rs"]);
    }

    // ── compute_per_file_hashes_for ──

    const MULTI_DIFF: &str = concat!(
        "diff --git a/foo.rs b/foo.rs\n",
        "index abc..def 100644\n",
        "--- a/foo.rs\n",
        "+++ b/foo.rs\n",
        "@@ -1,2 +1,3 @@\n",
        "+line1\n",
        " context\n",
        "diff --git a/bar.rs b/bar.rs\n",
        "index 111..222 100644\n",
        "+line2\n",
        "diff --git a/baz.rs b/baz.rs\n",
        "+line3\n",
    );

    #[test]
    fn targeted_hashes_match_the_full_pass_exactly() {
        // The targeted pass must not drift from the full one — same hash for
        // the same file, with the same section boundaries. Every subset is
        // checked because a wanted file's section must include its own header
        // lines whether or not the files before it were skipped.
        let full = compute_per_file_hashes(MULTI_DIFF);

        for wanted in [
            vec!["foo.rs"],
            vec!["bar.rs"],
            vec!["baz.rs"],
            vec!["foo.rs", "baz.rs"],
            vec!["bar.rs", "baz.rs"],
            vec!["foo.rs", "bar.rs", "baz.rs"],
        ] {
            let set: std::collections::HashSet<String> =
                wanted.iter().map(|s| s.to_string()).collect();
            let targeted = compute_per_file_hashes_for(MULTI_DIFF, &set);
            assert_eq!(
                targeted.len(),
                wanted.len(),
                "only wanted files are returned for {wanted:?}"
            );
            for path in &wanted {
                assert_eq!(
                    targeted[*path], full[*path],
                    "hash for {path} must match the full pass"
                );
            }
        }
    }

    #[test]
    fn targeted_hashes_omit_unwanted_files() {
        let wanted: std::collections::HashSet<String> =
            ["foo.rs".to_string()].into_iter().collect();
        let hashes = compute_per_file_hashes_for(MULTI_DIFF, &wanted);
        assert_eq!(hashes.len(), 1);
        assert!(hashes.contains_key("foo.rs"));
        assert!(!hashes.contains_key("bar.rs"));
        assert!(!hashes.contains_key("baz.rs"));
    }

    #[test]
    fn targeted_hashes_empty_wanted_is_empty() {
        let hashes = compute_per_file_hashes_for(MULTI_DIFF, &std::collections::HashSet::new());
        assert!(hashes.is_empty());
    }

    #[test]
    fn single_file_hash_matches_the_full_pass() {
        let full = compute_per_file_hashes(MULTI_DIFF);
        assert_eq!(
            compute_per_file_hash(MULTI_DIFF, "bar.rs").unwrap(),
            full["bar.rs"]
        );
        assert!(
            compute_per_file_hash(MULTI_DIFF, "absent.rs").is_none(),
            "a path absent from the diff has no hash"
        );
    }

    fn write_review(path: &Path, hash: &str, created_at: &str) {
        write_sidecar_json::<ErReview>(
            path,
            json!({
                "version": 1, "diff_hash": hash, "created_at": created_at,
                "files": {
                    "src/a.rs": { "risk": "medium", "findings": [
                        { "id": "a1", "severity": "medium", "title": "Old A", "line_start": 1 }
                    ] },
                    "src/b.rs": { "risk": "medium", "findings": [
                        { "id": "b1", "severity": "medium", "title": "Old B", "line_start": 2 }
                    ] }
                }
            }),
        );
    }

    fn write_scoped_general(path: &Path, scope: &str, base_created_at: &str, title: &str) {
        std::fs::create_dir_all(path.join("scoped/experts")).unwrap();
        write_sidecar_json::<super::super::scoped_overlay::ScopedReview>(
            path,
            json!({
                "version": 1, "diff_hash": "hash", "created_at": "t1",
                "files": { scope: { "risk": "medium", "findings": [
                    { "id": "n1", "severity": "medium", "title": title, "line_start": 5 }
                ] } },
                "scope_paths": [scope],
                "base_diff_hash": "hash",
                "base_created_at": base_created_at,
                "host_stamped": true
            }),
        );
    }

    #[test]
    fn scoped_overlay_applies_through_the_loader() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_review(path, "hash", "t0");
            write_scoped_general(path, "src/a.rs", "t0", "New A");

            let state = load_ai_state(path.to_str().unwrap(), "hash", None);

            let review = state.review.expect("a projected review");
            assert_eq!(review.files["src/a.rs"].findings[0].title, "New A");
            assert_eq!(review.files["src/b.rs"].findings[0].title, "Old B");
            assert_eq!(review.diff_hash, "hash", "the base identity stays put");
            assert!(!state.is_stale);
        });
    }

    #[test]
    fn scoped_overlay_with_a_changed_base_is_not_applied_by_the_loader() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_review(path, "hash", "t0");
            write_scoped_general(path, "src/a.rs", "t-moved", "New A");

            let state = load_ai_state(path.to_str().unwrap(), "hash", None);

            let review = state.review.expect("a projected review");
            assert_eq!(review.files["src/a.rs"].findings[0].title, "Old A");
        });
    }

    /// A validation matching the current diff refreshes the review's hash, so
    /// experts from the old generation no longer ride along.
    #[test]
    fn validation_refreshes_a_stale_review_and_drops_stale_experts() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_review(path, "old", "t0");
            write_sidecar_json_slot::<crate::ai::experts::ExpertReview>(
                path,
                er_store::ExpertId::new("security").unwrap(),
                json!({
                    "version": 1, "expert_id": "security", "diff_hash": "old",
                    "created_at": "t0", "summary": "security summary",
                    "files": { "src/a.rs": { "findings": [
                        { "id": "sec-1", "severity": "high", "title": "Stale expert row" }
                    ] } }
                }),
            );
            write_sidecar_json::<crate::ai::validation::ValidationReview>(
                path,
                json!({
                    "version": 1, "diff_hash": "new", "review_created_at": "t0",
                    "created_at": "t1", "outcomes": []
                }),
            );

            let state = load_ai_state(path.to_str().unwrap(), "new", None);

            let review = state.review.expect("a projected review");
            assert_eq!(review.diff_hash, "new", "validation refreshed the review");
            assert!(!state.is_stale);
            assert!(
                !review.files["src/a.rs"]
                    .findings
                    .iter()
                    .any(|finding| finding.title == "Stale expert row"),
                "an expert from the old generation must not ride along"
            );
            assert!(!state.agent_summaries.contains_key("Security"));
        });
    }

    /// A file a newer build wrote carries semantics this build cannot know; the
    /// store refuses it, and the loader must not apply it.
    #[test]
    fn a_newer_version_validation_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_review(path, "hash", "t0");
            let key = crate::ai::identity::finding_key("src/a.rs", Some(1), "Old A");
            std::fs::write(
                path.join("validation.json"),
                serde_json::to_vec(&json!({
                    "version": 99, "diff_hash": "hash", "review_created_at": "t0",
                    "created_at": "t1",
                    "outcomes": [{ "key": key, "file": "src/a.rs", "result": "resolved" }]
                }))
                .unwrap(),
            )
            .unwrap();

            let state = load_ai_state(path.to_str().unwrap(), "hash", None);

            let review = state.review.expect("a projected review");
            assert!(
                review.files["src/a.rs"].findings[0].is_active(),
                "a newer validation must not resolve the row"
            );
            assert_eq!(state.validation_effect.applied, 0);
            assert_eq!(state.validation_effect.unmatched, 0);
            assert!(!state.validation_effect.ignored);
        });
    }

    /// The scoped general rerun keeps the base's identity, so a validation that
    /// matched the base still applies after the rerun.
    #[test]
    fn scoped_general_rerun_keeps_validation_outcomes() {
        use crate::ai::review::Confidence;
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_sidecar_json::<ErReview>(
                path,
                json!({
                    "version": 1, "diff_hash": "hash", "created_at": "t0",
                    "files": {
                        "src/a.rs": { "risk": "medium", "findings": [
                            { "id": "a1", "severity": "medium", "title": "Old A", "line_start": 1 }
                        ] },
                        "src/b.rs": { "risk": "medium", "findings": [
                            { "id": "b1", "severity": "medium", "title": "Resolved B", "line_start": 2 }
                        ] }
                    }
                }),
            );
            write_scoped_general(path, "src/a.rs", "t0", "New A");
            let key = crate::ai::identity::finding_key("src/b.rs", Some(2), "Resolved B");
            write_sidecar_json::<crate::ai::validation::ValidationReview>(
                path,
                json!({
                    "version": 1, "diff_hash": "hash", "review_created_at": "t0",
                    "created_at": "t1",
                    "outcomes": [{ "key": key, "file": "src/b.rs", "result": "resolved" }]
                }),
            );

            let state = load_ai_state(path.to_str().unwrap(), "hash", None);

            let review = state.review.expect("a projected review");
            assert_eq!(review.files["src/a.rs"].findings[0].title, "New A");
            assert_eq!(
                review.files["src/b.rs"].findings[0].confidence,
                Confidence::Dropped,
                "the validation survived the scoped rerun"
            );
            assert_eq!(state.validation_effect.applied, 1);
        });
    }

    /// Loading never clears: clearing happens on run success, so a scoped
    /// overlay still applies while the full review is on disk.
    #[test]
    fn loading_never_clears_the_overlay() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let path = dir.path();
            write_review(path, "hash", "t0");
            write_scoped_general(path, "src/a.rs", "t0", "New A");
            let state = load_ai_state(path.to_str().unwrap(), "hash", None);
            let review = state.review.expect("a projected review");
            assert_eq!(review.files["src/a.rs"].findings[0].title, "New A");
        });
    }
}
