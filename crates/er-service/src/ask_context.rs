//! The diff an ask puts in its prompt, read from a tab's published content
//! by the rule the App reads its own tab by, so an ask never waits on the App
//! or on the tab's actor.

use er_engine::ports::Ports;
use er_engine::review_flow_driver::cached_diff_matches_scope;
use er_engine::tab_identity::DiffMode;

use crate::content::TabContent;

/// The review scope an ask on a view reads: the view's own for the three
/// git views, the branch for every other.
pub fn ask_scope(mode: DiffMode) -> &'static str {
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => mode.git_mode(),
        _ => "branch",
    }
}

/// The raw diff an ask on `content`'s tab reads: the diff the content holds
/// when it is the one the ask's scope reviews and it was hashed, else a fresh
/// fetch of that scope. History, Conflicts and Hidden always fetch, since the
/// content keeps no diff of the view it left: History and Conflicts answer
/// none, which the fetch refuses, and Hidden an empty diff. Called off the
/// actor, since the fetch runs git.
pub fn ask_raw_diff(ports: &Ports, content: &TabContent) -> Option<String> {
    let id = content.tab.identity();
    let scope = ask_scope(id.mode);
    let meta = &content.diff.meta;
    let shows_a_diff = !matches!(
        id.mode,
        DiffMode::History | DiffMode::Conflicts | DiffMode::Hidden
    );
    if shows_a_diff && !meta.branch_diff_hash.is_empty() && cached_diff_matches_scope(&id, scope)
    {
        return Some(meta.raw.clone());
    }
    er_engine::diff_load::fetch_raw(ports, &id, scope, false).ok()
}
