//! How far origin has moved past what a tab shows, against the tips the
//! desktop's probes post: a branch view's base behind origin's tip, and a
//! checkout ahead of its PR's head. Counting never fetches, so a commit not
//! yet in the local object store leaves its count unknown.

use er_engine::git::Git;
use er_engine::tab_identity::{DiffMode, ResolvedTab};

/// What origin reported last, as the desktop's probes post it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteTips {
    /// The tip of the base branch on origin, for a branch-vs-base view.
    pub base_tip: Option<String>,
    /// The PR's head in the PR list, for a checkout that has a PR.
    pub pr_head: Option<String>,
}

/// A branch view's base, built from one commit while origin's tip is another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseBehind {
    pub built_from_oid: String,
    pub latest_oid: String,
    /// Commits the tip has that the base lacks.
    pub commits_behind: Option<usize>,
}

/// A checkout against its PR's head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Divergence {
    pub pr_head_oid: String,
    /// Commits on the checkout's HEAD that the PR head lacks.
    pub ahead: Option<usize>,
    /// Files with unstaged changes in the checkout.
    pub unstaged_files: usize,
}

/// The tips a tab was last told of and what they show against it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Freshness {
    pub tips: RemoteTips,
    pub base: Option<BaseBehind>,
    pub divergence: Option<Divergence>,
}

/// `tab` against `tips`, by the snapshot's rules. `unstaged_files` is the
/// load's count at the repo root, which a checkout elsewhere counts again.
pub(crate) fn measure(
    git: &dyn Git,
    tab: &ResolvedTab,
    tips: RemoteTips,
    unstaged_files: usize,
) -> Freshness {
    let remote = tab.identity().is_remote();
    let base = tips
        .base_tip
        .as_deref()
        .filter(|_| !remote && tab.pr_number.is_none() && tab.mode == DiffMode::Branch)
        .and_then(|latest| {
            let used = er_engine::github::rev_parse_oid(git, &tab.repo_root, &tab.base_branch)?;
            (!latest.is_empty() && used != latest).then(|| BaseBehind {
                commits_behind: count(git, &tab.repo_root, &used, latest),
                built_from_oid: used,
                latest_oid: latest.to_string(),
            })
        });
    let divergence = tips
        .pr_head
        .as_deref()
        .filter(|head| {
            !head.is_empty() && !remote && !matches!(tab.mode, DiffMode::PrDiff | DiffMode::Tour)
        })
        .and_then(|pr_head| {
            let root = match (&tab.local_branch_view, &tab.checkout_root) {
                (None, _) => tab.repo_root.as_str(),
                (Some(_), Some(checkout)) => checkout.as_str(),
                (Some(_), None) => return None,
            };
            let unstaged_files = if root == tab.repo_root {
                unstaged_files
            } else {
                git.diff_shortstat(root, false).0
            };
            let ahead = er_engine::github::rev_parse_oid(git, root, "HEAD")
                .and_then(|head| count(git, root, pr_head, &head));
            Some(Divergence {
                pr_head_oid: pr_head.to_string(),
                ahead,
                unstaged_files,
            })
        });
    Freshness {
        tips,
        base,
        divergence,
    }
}

/// `content` measured again against `tips`, off the actor.
pub(crate) fn remeasure(
    deps: &crate::actor::WorkspaceDeps,
    content: &crate::content::TabContent,
    tips: RemoteTips,
) -> Freshness {
    let unstaged = content.diff.meta.scope_stats.unstaged.0;
    measure(&*deps.ports().git, &content.tab, tips, unstaged)
}

/// Commits reachable from `to` and not from `from`.
fn count(git: &dyn Git, root: &str, from: &str, to: &str) -> Option<usize> {
    let range = format!("{from}..{to}");
    git.rev_list_count(root, &range).ok()?.trim().parse().ok()
}
