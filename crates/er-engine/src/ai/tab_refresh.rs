//! The steps a full diff refresh runs over a tab's AI state once the new diff
//! is parsed: comments follow moved code, findings and files are marked stale.
//!
//! They read only the parsed files and the sidecars, so `TabState` and the
//! headless service run the same steps and write the same sidecars.

use super::{self as ai, AiState};
use crate::git::{self, DiffFile};
use anyhow::Context;

/// Relocate all comments to their new positions after a diff change, and write
/// the moved anchors back to their sidecars.
///
/// `files` is the parsed diff, `lazy` whether its unparsed files are stubs, and
/// `current_hash` the diff hash the anchors are stamped with. Questions and
/// notes live in `er_dir`; `comments_dir` is asked for only when a GitHub
/// comment moved, since resolving it can run git.
pub fn relocate_all_comments(
    state: &mut AiState,
    files: &[DiffFile],
    lazy: bool,
    current_hash: &str,
    er_dir: &str,
    comments_dir: impl FnOnce() -> String,
) {
    let current_hash = current_hash.to_string();

    // The per-id anchor changes to re-apply to `current` when writing back.
    #[derive(Clone)]
    struct AnchorUpdate {
        hunk_index: Option<usize>,
        line_start: Option<usize>,
        anchor_status: String,
        relocated_at_hash: String,
        stale: bool,
    }
    let mut question_updates: std::collections::HashMap<String, AnchorUpdate> =
        std::collections::HashMap::new();
    let mut note_updates: std::collections::HashMap<String, AnchorUpdate> =
        std::collections::HashMap::new();
    let mut comment_updates: std::collections::HashMap<String, AnchorUpdate> =
        std::collections::HashMap::new();
    macro_rules! record {
        ($map:ident, $item:expr) => {
            $map.insert(
                $item.id.clone(),
                AnchorUpdate {
                    hunk_index: $item.hunk_index,
                    line_start: $item.line_start,
                    anchor_status: $item.anchor_status.clone(),
                    relocated_at_hash: $item.relocated_at_hash.clone(),
                    stale: $item.stale,
                },
            );
        };
    }
    fn apply_updates<T>(
        items: &mut [T],
        updates: &std::collections::HashMap<String, AnchorUpdate>,
        id_of: impl Fn(&T) -> &str,
        apply: impl Fn(&mut T, &AnchorUpdate),
    ) {
        for item in items.iter_mut() {
            if let Some(update) = updates.get(id_of(item)) {
                apply(item, update);
            }
        }
    }

    // Build rename map: old path → new path
    let rename_map: std::collections::HashMap<String, String> = files
        .iter()
        .filter_map(|f| {
            if let git::FileStatus::Renamed(ref old_path) = f.status {
                Some((old_path.clone(), f.path.clone()))
            } else {
                None
            }
        })
        .collect();

    let is_lazy = lazy;

    // Ready / unparsed-stub / missing. Unparsed lazy stubs must not be
    // treated as Missing — that stamped every comment `stale` and the
    // Comments panel hid them behind Hide outdated.
    enum RelocateLookup {
        Ready(usize),
        UnparsedStub,
        Missing,
    }
    let lookup_file = |path: &str| -> RelocateLookup {
        let find_idx = |p: &str| -> Option<usize> { files.iter().position(|f| f.path == p) };
        let Some(idx) = find_idx(path).or_else(|| rename_map.get(path).and_then(|np| find_idx(np)))
        else {
            return RelocateLookup::Missing;
        };
        if is_lazy {
            let f = &files[idx];
            if f.hunks.is_empty() && !f.compacted {
                return RelocateLookup::UnparsedStub;
            }
        }
        RelocateLookup::Ready(idx)
    };

    // Process questions
    let questions_changed = if let Some(ref mut qs) = state.questions {
        let mut changed = false;
        for q in &mut qs.questions {
            if q.relocated_at_hash == current_hash {
                continue;
            }
            // File-level and document-line questions have no hunk anchor to relocate — skip
            if q.is_file_level() || q.is_document_anchor() {
                q.relocated_at_hash = current_hash.clone();
                continue;
            }
            let result = match lookup_file(&q.file) {
                RelocateLookup::UnparsedStub => continue,
                RelocateLookup::Ready(idx) => {
                    let anchor = ai::CommentAnchor {
                        hunk_index: q.hunk_index,
                        line_start: q.line_start,
                        line_content: q.line_content.clone(),
                        context_before: q.context_before.clone(),
                        context_after: q.context_after.clone(),
                        old_line_start: q.old_line_start,
                        hunk_header: q.hunk_header.clone(),
                    };
                    ai::relocate_comment(&anchor, &files[idx])
                }
                RelocateLookup::Missing => ai::RelocationResult::Lost,
            };
            match result {
                ai::RelocationResult::Unchanged => {
                    q.anchor_status = "original".to_string();
                    q.relocated_at_hash = current_hash.clone();
                    q.stale = false;
                    changed = true;
                    record!(question_updates, q);
                }
                ai::RelocationResult::Relocated {
                    new_hunk_index,
                    new_line_start,
                } => {
                    q.hunk_index = Some(new_hunk_index);
                    q.line_start = Some(new_line_start);
                    q.anchor_status = "relocated".to_string();
                    q.relocated_at_hash = current_hash.clone();
                    q.stale = false;
                    changed = true;
                    record!(question_updates, q);
                }
                ai::RelocationResult::Lost => {
                    q.anchor_status = "lost".to_string();
                    q.stale = true;
                    q.relocated_at_hash = current_hash.clone();
                    changed = true;
                    record!(question_updates, q);
                }
            }
        }
        changed
    } else {
        false
    };

    // Process notes (same anchoring model as questions)
    let notes_changed = if let Some(ref mut ns) = state.notes {
        let mut changed = false;
        for n in &mut ns.notes {
            if n.relocated_at_hash == current_hash {
                continue;
            }
            if n.is_file_level() || n.is_document_anchor() {
                n.relocated_at_hash = current_hash.clone();
                continue;
            }
            let result = match lookup_file(&n.file) {
                RelocateLookup::UnparsedStub => continue,
                RelocateLookup::Ready(idx) => {
                    let anchor = ai::CommentAnchor {
                        hunk_index: n.hunk_index,
                        line_start: n.line_start,
                        line_content: n.line_content.clone(),
                        context_before: n.context_before.clone(),
                        context_after: n.context_after.clone(),
                        old_line_start: n.old_line_start,
                        hunk_header: n.hunk_header.clone(),
                    };
                    ai::relocate_comment(&anchor, &files[idx])
                }
                RelocateLookup::Missing => ai::RelocationResult::Lost,
            };
            match result {
                ai::RelocationResult::Unchanged => {
                    n.anchor_status = "original".to_string();
                    n.relocated_at_hash = current_hash.clone();
                    n.stale = false;
                    changed = true;
                    record!(note_updates, n);
                }
                ai::RelocationResult::Relocated {
                    new_hunk_index,
                    new_line_start,
                } => {
                    n.hunk_index = Some(new_hunk_index);
                    n.line_start = Some(new_line_start);
                    n.anchor_status = "relocated".to_string();
                    n.relocated_at_hash = current_hash.clone();
                    n.stale = false;
                    changed = true;
                    record!(note_updates, n);
                }
                ai::RelocationResult::Lost => {
                    n.anchor_status = "lost".to_string();
                    n.stale = true;
                    n.relocated_at_hash = current_hash.clone();
                    changed = true;
                    record!(note_updates, n);
                }
            }
        }
        changed
    } else {
        false
    };

    // Process GitHub comments (top-level only; replies follow their parent)
    let comments_changed = if let Some(ref mut gc) = state.github_comments {
        let mut changed = false;
        for c in &mut gc.comments {
            if c.relocated_at_hash == current_hash {
                continue;
            }
            if c.in_reply_to.is_some() {
                continue;
            }
            // File-level comments have no anchor to relocate — skip
            if c.hunk_index.is_none() && c.line_start.is_none() && c.hunk_header.is_empty() {
                c.relocated_at_hash = current_hash.clone();
                continue;
            }
            let result = match lookup_file(&c.file) {
                RelocateLookup::UnparsedStub => continue,
                RelocateLookup::Ready(idx) => {
                    let anchor = ai::CommentAnchor {
                        hunk_index: c.hunk_index,
                        line_start: c.line_start,
                        line_content: c.line_content.clone(),
                        context_before: c.context_before.clone(),
                        context_after: c.context_after.clone(),
                        old_line_start: c.old_line_start,
                        hunk_header: c.hunk_header.clone(),
                    };
                    ai::relocate_comment(&anchor, &files[idx])
                }
                RelocateLookup::Missing => ai::RelocationResult::Lost,
            };
            match result {
                ai::RelocationResult::Unchanged => {
                    c.anchor_status = "original".to_string();
                    c.relocated_at_hash = current_hash.clone();
                    c.stale = false;
                    changed = true;
                    record!(comment_updates, c);
                }
                ai::RelocationResult::Relocated {
                    new_hunk_index,
                    new_line_start,
                } => {
                    c.hunk_index = Some(new_hunk_index);
                    c.line_start = Some(new_line_start);
                    c.anchor_status = "relocated".to_string();
                    c.relocated_at_hash = current_hash.clone();
                    c.stale = false;
                    changed = true;
                    record!(comment_updates, c);
                }
                ai::RelocationResult::Lost => {
                    c.anchor_status = "lost".to_string();
                    c.stale = true;
                    c.relocated_at_hash = current_hash.clone();
                    changed = true;
                    record!(comment_updates, c);
                }
            }
        }
        changed
    } else {
        false
    };

    // Write back to disk if anything changed. Write failures are ignored — the
    // next refresh re-relocates the same comments from scratch. Each write
    // re-applies the id's anchor change to `current`, so a comment added
    // since the in-memory pass is not lost.
    if questions_changed {
        if let Ok(scope) = er_store::Scope::dir(er_dir) {
            let store = crate::store_registry::store();
            let _ = ai::annotation_store::update_questions(&store, &scope, move |current| {
                let mut qs = current.context("the questions file is missing or unreadable")?;
                apply_updates(
                    &mut qs.questions,
                    &question_updates,
                    |q| q.id.as_str(),
                    |q, update| {
                        q.hunk_index = update.hunk_index;
                        q.line_start = update.line_start;
                        q.anchor_status = update.anchor_status.clone();
                        q.relocated_at_hash = update.relocated_at_hash.clone();
                        q.stale = update.stale;
                    },
                );
                Ok::<_, anyhow::Error>(qs)
            });
        }
    }
    if notes_changed {
        if let Ok(scope) = er_store::Scope::dir(er_dir) {
            let store = crate::store_registry::store();
            let _ = ai::annotation_store::update_notes(&store, &scope, move |current| {
                let mut ns = current.context("the notes file is missing or unreadable")?;
                apply_updates(
                    &mut ns.notes,
                    &note_updates,
                    |n| n.id.as_str(),
                    |n, update| {
                        n.hunk_index = update.hunk_index;
                        n.line_start = update.line_start;
                        n.anchor_status = update.anchor_status.clone();
                        n.relocated_at_hash = update.relocated_at_hash.clone();
                        n.stale = update.stale;
                    },
                );
                Ok::<_, anyhow::Error>(ns)
            });
        }
    }
    if comments_changed {
        if let Ok(scope) = er_store::Scope::dir(comments_dir()) {
            let store = crate::store_registry::store();
            let _ = ai::annotation_store::update_github_comments(&store, &scope, move |current| {
                let mut gc =
                    current.context("the GitHub comments file is missing or unreadable")?;
                apply_updates(
                    &mut gc.comments,
                    &comment_updates,
                    |c| c.id.as_str(),
                    |c, update| {
                        c.hunk_index = update.hunk_index;
                        c.line_start = update.line_start;
                        c.anchor_status = update.anchor_status.clone();
                        c.relocated_at_hash = update.relocated_at_hash.clone();
                        c.stale = update.stale;
                    },
                );
                Ok::<_, anyhow::Error>(gc)
            });
        }
    }

    // Relocation mutates hunk_index / line_start in memory; rebuild the lazy
    // comment index so inline lookups stay in sync (file-level counts alone
    // would still look correct with a stale index).
    if questions_changed || notes_changed || comments_changed {
        state.rebuild_comment_index();
    }
}

/// Recompute per-finding staleness against the current diff.
///
/// Findings are never re-anchored, so this only sets the flag on each one:
/// a finding whose target line is gone is suspect, while a sibling in the
/// same file whose line merely moved is not. `stale_files` (file-level) is
/// computed separately and still drives the file-tree indicator.
///
/// Three outcomes per reviewed file, matching `relocate_all_comments`:
/// present and parsed → compare each finding against it; an unparsed lazy
/// stub → skip, because an empty `hunks` means "not loaded yet", not "the
/// code is gone"; absent from the diff entirely → stale, the same verdict
/// the comment path reaches for a missing file.
pub fn refresh_finding_staleness(state: &mut AiState, files: &[DiffFile], lazy: bool) {
    let is_lazy = lazy;
    let rename_map: std::collections::HashMap<String, String> = files
        .iter()
        .filter_map(|f| match &f.status {
            git::FileStatus::Renamed(old_path) => Some((old_path.clone(), f.path.clone())),
            _ => None,
        })
        .collect();

    let Some(ref mut review) = state.review else {
        return;
    };
    // Indexed once: this runs on every refresh and a linear scan per review
    // file is quadratic on a large diff.
    let index_by_path: std::collections::HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(idx, f)| (f.path.as_str(), idx))
        .collect();
    for (path, file_review) in review.files.iter_mut() {
        let idx = index_by_path
            .get(path.as_str())
            .or_else(|| {
                rename_map
                    .get(path)
                    .and_then(|p| index_by_path.get(p.as_str()))
            })
            .copied();
        let diff_file = match idx {
            Some(idx) => {
                let file = &files[idx];
                // Not loaded yet is not the same as gone — the trap
                // `relocate_all_comments` documents. Leave the flags alone.
                if is_lazy && file.hunks.is_empty() && !file.compacted {
                    continue;
                }
                Some(file)
            }
            // The file left the diff entirely, so an anchored finding points
            // at nothing. The comment path reaches the same verdict
            // (`Missing → Lost → stale`).
            None => None,
        };
        for finding in &mut file_review.findings {
            finding.refresh_stale(diff_file);
        }
    }
}

/// Compute which files have changed since the review, populating stale_files
pub fn compute_stale_files(state: &mut AiState, branch_raw_diff: &str) {
    if let Some(ref review) = state.review {
        if review.file_hashes.is_empty() {
            return;
        }
        let current_hashes = ai::compute_per_file_hashes(branch_raw_diff);
        let mut stale = std::collections::HashSet::new();
        for (file, review_hash) in &review.file_hashes {
            match current_hashes.get(file) {
                Some(current_hash) if current_hash == review_hash => {}
                _ => {
                    stale.insert(file.clone());
                }
            }
        }
        // Files in current diff but not in review are new (not stale)
        state.stale_files = stale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,2 @@
 a-one
-a-old
+a-new
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,2 +1,2 @@
 b-one
-b-old
+b-new
";

    type HashMapOf = std::collections::HashMap<String, String>;

    fn reviewed_against(file_hashes: HashMapOf) -> AiState {
        let mut state = AiState::default();
        state.review = Some(ai::ErReview {
            version: 1,
            diff_hash: "old".to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: std::collections::HashMap::new(),
            file_hashes,
        });
        state
    }

    #[test]
    fn a_file_whose_hash_moved_or_left_the_diff_is_stale() {
        let current = ai::compute_per_file_hashes(RAW);
        let mut file_hashes = HashMapOf::new();
        file_hashes.insert("src/a.rs".to_string(), current["src/a.rs"].clone());
        file_hashes.insert("src/b.rs".to_string(), "moved".to_string());
        file_hashes.insert("src/gone.rs".to_string(), "gone".to_string());
        let mut state = reviewed_against(file_hashes);

        compute_stale_files(&mut state, RAW);

        let mut stale: Vec<&str> = state.stale_files.iter().map(String::as_str).collect();
        stale.sort_unstable();
        assert_eq!(stale, vec!["src/b.rs", "src/gone.rs"]);
    }

    #[test]
    fn a_review_without_file_hashes_leaves_the_stale_set_alone() {
        let mut state = reviewed_against(HashMapOf::new());
        state.stale_files.insert("kept.rs".to_string());

        compute_stale_files(&mut state, RAW);

        assert!(state.stale_files.contains("kept.rs"));
    }
}
