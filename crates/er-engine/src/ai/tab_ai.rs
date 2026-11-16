//! The steps that turn one tab's loaded sidecars into its AI state: the
//! PR-scoped GitHub comments, the guided tour for the active view and the
//! staleness carried across a reload.
//!
//! They depend only on the tab's identity and its diff hashes, so `TabState`
//! and the headless service assemble the same state from the same sidecars.

use super::{AiState, ErGithubComments, ErTour, GithubComment};
use crate::tab_identity::{ReviewBucket, TabIdentity};
use std::collections::HashSet;

/// What one tab's AI state is assembled against.
#[derive(Clone, Copy)]
pub struct TabAiLoad<'a> {
    pub identity: TabIdentity<'a>,
    pub git: &'a dyn er_git::Git,
    /// The active bucket, which the sidecars were loaded from.
    pub er_dir: &'a str,
    pub diff_hash: &'a str,
    pub branch_diff_hash: &'a str,
}

/// The in-memory post-load steps of a tab's AI reload, applied to state just
/// loaded from `load.er_dir`.
///
/// `prev_tour_stale` and `prev_stale_files` are what the previous state held:
/// a fast-hash refresh keeps the tour's staleness, and per-file staleness is
/// recomputed by the diff refresh rather than by a reload.
pub fn assemble_tab_ai(
    ai: &mut AiState,
    load: &TabAiLoad<'_>,
    prev_tour_stale: bool,
    prev_stale_files: HashSet<String>,
) {
    let identity = &load.identity;
    // GitHub PR comments are PR-scoped: stored in the shared PR bucket and shown
    // only in the Local branch (Branch) and PR Diff views — hidden in Unstaged/
    // Staged/History, where PR-line comments don't apply.
    if identity.pr_number.is_some() {
        if matches!(
            identity.review_bucket(),
            ReviewBucket::Branch | ReviewBucket::Pr
        ) {
            let gh_dir = identity
                .pr_bucket_er_dir(load.git)
                .unwrap_or_else(|| load.er_dir.to_string());
            if gh_dir != load.er_dir {
                // `load_ai_state(er_dir)` may have loaded github comments from the
                // active (branch) bucket — these are orphans written while this tab
                // still had no `pr_number` (github_comments_dir() fell back to
                // er_dir then). The PR bucket is now authoritative, so migrate any
                // such local comments into it instead of discarding them.
                let orphaned = ai.github_comments.take();
                ai.github_comments = adopt_pr_bucket_comments(orphaned, &gh_dir, load.er_dir);
                ai.rebuild_comment_index();
            }
        } else if ai.github_comments.take().is_some() {
            ai.rebuild_comment_index();
        }
    }

    // Guided tour (per-view buckets): the Local branch tour lives in the branch
    // bucket, the PR Diff tour in the PR bucket. The active context (branch vs PR —
    // including which view the Guide tab was opened from, via `tour_context_is_pr`)
    // selects the bucket, and a tour whose diff_hash matches the active diff is
    // reused across the two when the branch and PR diffs are identical. Staleness is
    // computed per-context with `tour_stale_for`.
    let want_pr = identity.tour_context_is_pr();
    ai.tour = resolve_view_tour(
        identity.tour_bucket_er_dir(load.git).as_deref(),
        identity.cross_tour_bucket_er_dir(load.git).as_deref(),
        want_pr,
        load.diff_hash,
        load.branch_diff_hash,
    );
    ai.tour_stale = match ai.tour.as_ref() {
        None => false,
        Some(t) => tour_stale_for(
            &t.diff_hash,
            want_pr,
            load.diff_hash,
            load.branch_diff_hash,
            prev_tour_stale,
        ),
    };
    // Preserve per-file staleness across .er-* file reloads (recomputed in refresh_diff)
    if ai.is_stale {
        ai.stale_files = prev_stale_files;
    }
}

/// Decide whether a loaded guided tour is stale relative to the diff it is
/// scoped to.
///
/// - Branch-scoped tours (`want_pr == false`) baseline against `branch_diff_hash`,
///   which is always a SHA-256 of the branch diff and is valid in every
///   working-tree mode. In Unstaged/Staged/History `diff_hash` is a *different*
///   scope's diff, so using it would mark a fresh branch tour stale.
/// - PR-scoped tours baseline against the current `diff_hash` (the PR diff's
///   SHA-256 after a full refresh). On a fast-hash refresh (`diff_hash` is the
///   16-char non-cryptographic hash) keep the previous result rather than
///   compare against an incompatible hash.
pub fn tour_stale_for(
    tour_hash: &str,
    want_pr: bool,
    diff_hash: &str,
    branch_diff_hash: &str,
    prev: bool,
) -> bool {
    if !want_pr {
        return tour_hash != branch_diff_hash;
    }
    if diff_hash.len() == 64 {
        return tour_hash != diff_hash;
    }
    prev
}

/// Parse the per-view `tour.json` from a bucket directory, if present and valid.
fn load_tour_from_dir(dir: &str) -> Option<ErTour> {
    super::load_tour_sidecar(dir, "tour.json")
}

/// Pick the tour to show in the active view (option C).
///
/// The Local branch tour lives in the branch bucket, the PR Diff tour in the PR
/// bucket. The active *context* — branch vs PR, including which view the Guide tab
/// was opened from (`tour_context_is_pr`) — selects the view's own bucket. Prefer a
/// tour whose `diff_hash` matches the active diff (own bucket first, then the other
/// bucket, so an identical-diff tour is reused across the two without copying). When
/// neither is fresh, keep a stale tour (own bucket first) so the Regenerate
/// affordance shows. Returns `None` only when no tour exists in either bucket.
pub fn resolve_view_tour(
    own_dir: Option<&str>,
    cross_dir: Option<&str>,
    want_pr: bool,
    diff_hash: &str,
    branch_diff_hash: &str,
) -> Option<ErTour> {
    let own = own_dir.and_then(load_tour_from_dir);
    // Avoid re-reading the same file when both buckets resolve to one dir.
    let cross = match (own_dir, cross_dir) {
        (Some(o), Some(c)) if o == c => None,
        _ => cross_dir.and_then(load_tour_from_dir),
    };
    // "Fresh for the active view" reuses the same per-context staleness rule, so a
    // tour generated against the matching diff is reused even from the other bucket.
    let is_fresh =
        |t: &ErTour| !tour_stale_for(&t.diff_hash, want_pr, diff_hash, branch_diff_hash, false);
    let own_fresh = own.as_ref().is_some_and(is_fresh);
    let cross_fresh = cross.as_ref().is_some_and(is_fresh);
    if own_fresh {
        own
    } else if cross_fresh {
        cross
    } else {
        own.or(cross)
    }
}

/// Merge orphaned local GitHub comments (written to the branch bucket while a
/// tab had no `pr_number`) into the now-authoritative PR-bucket comments.
///
/// Only `local` comments absent from the PR bucket (by id) are carried over;
/// `github`-sourced entries re-sync from GitHub and are never migrated.
/// Returns `true` when at least one comment was migrated, signalling the
/// caller to persist `pr_comments` and remove the orphan file.
pub fn migrate_orphaned_github_comments(
    orphaned: Option<ErGithubComments>,
    pr_comments: &mut Option<ErGithubComments>,
) -> bool {
    let orphaned = match orphaned {
        Some(o) => o,
        None => return false,
    };
    let to_migrate: Vec<GithubComment> = orphaned
        .comments
        .into_iter()
        .filter(|c| c.source == "local")
        .collect();
    if to_migrate.is_empty() {
        return false;
    }

    let target = pr_comments.get_or_insert_with(|| ErGithubComments {
        version: 1,
        diff_hash: orphaned.diff_hash.clone(),
        github: None,
        comments: Vec::new(),
    });

    let mut migrated = false;
    for comment in to_migrate {
        if target.comments.iter().any(|c| c.id == comment.id) {
            continue;
        }
        target.comments.push(comment);
        migrated = true;
    }
    migrated
}

/// Adopt the PR bucket's github comments after a tab gains its PR number.
///
/// `orphaned` came from the active bucket (written while the tab had no
/// PR). Local comments in it are merged into the PR bucket's `current`
/// under the store lock, so a comment written there since the tab opened
/// is kept; the orphan file is then deleted. Without local orphans this
/// only reads the PR bucket.
pub fn adopt_pr_bucket_comments(
    orphaned: Option<ErGithubComments>,
    gh_dir: &str,
    er_dir: &str,
) -> Option<ErGithubComments> {
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(gh_dir).ok()?;
    let has_local_orphans = orphaned
        .as_ref()
        .is_some_and(|gc| gc.comments.iter().any(|c| c.source == "local"));
    let mut migrated = false;
    let mut adopted = None;
    if has_local_orphans {
        if let Ok(updated) =
            super::annotation_store::update_github_comments(&store, &scope, |current| {
                let mut pr_comments = current;
                migrated = migrate_orphaned_github_comments(orphaned, &mut pr_comments);
                Ok::<_, std::convert::Infallible>(pr_comments.unwrap_or_else(|| ErGithubComments {
                    version: 1,
                    diff_hash: String::new(),
                    github: None,
                    comments: Vec::new(),
                }))
            })
        {
            adopted = Some(updated.value);
        }
        if adopted.is_some() && migrated {
            // Drop the orphan file only after the merged write succeeded.
            if let Ok(orphan_scope) = er_store::Scope::dir(er_dir) {
                let _ = super::annotation_store::delete::<ErGithubComments>(&store, &orphan_scope);
            }
        }
    }
    adopted.or_else(
        || match super::annotation_store::load_github_comments(&store, &scope) {
            Ok(er_store::Loaded::Found(stored)) => Some(stored.value),
            _ => None,
        },
    )
}
