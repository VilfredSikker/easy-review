//! The working-tree writes (decision 8): stage, unstage, stage all, commit
//! and push. Each runs the `Git` port on an effect thread, then reloads the
//! tab in the view it shows, so the write and the diff it changed publish in
//! one revision. A watched file's snapshot and delete run there too, under
//! er-git's path checks.

use std::sync::mpsc::Sender;

use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;

use crate::actor::{refuse_unknown_tab, reload_input, spawn_effect, WorkspaceDeps};
use crate::content::{ContentPatch, TabContent};
use crate::effect::LoadInput;
use crate::handle::{ActorMsg, Effected, ReplySender, WorkspaceError};
use crate::protocol::Reply;
use crate::workspace::WorkspaceState;

/// One working-tree write, as its command names it.
pub(crate) enum Write {
    Stage(String),
    Unstage(String),
    StageAll,
    Commit(String),
    Push,
}

/// Runs `write` on `key`'s checkout off the actor, then reloads the tab over
/// the content it started from.
pub(crate) fn spawn(
    deps: &WorkspaceDeps,
    state: &WorkspaceState,
    tx: &Sender<ActorMsg>,
    reply: ReplySender,
    key: TabKey,
    write: Write,
) {
    let Some(content) = state.view().tabs.get(&key).cloned() else {
        return refuse_unknown_tab(reply);
    };
    let deps = deps.clone();
    spawn_effect(tx, reply, move || {
        let (notice, input) = run(&deps, &content, write)?;
        let loaded = crate::effect::load_tab(&deps, input)?;
        Ok(Effected::patched(
            Reply::WorktreeWritten { key, notice },
            ContentPatch::Loaded {
                key,
                since: Some(content.generation()),
                loaded: Box::new(loaded),
            },
        ))
    });
}

fn failed(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError::Failed(message.into())
}

/// Runs the port write and answers the status line's notice and the reload.
/// A remote PR, and a branch view checked out nowhere, have no working tree
/// to write in.
fn run(
    deps: &WorkspaceDeps,
    content: &TabContent,
    write: Write,
) -> Result<(String, LoadInput), WorkspaceError> {
    let tab = &content.tab;
    if tab.identity().is_remote()
        || (tab.local_branch_view.is_some() && tab.checkout_root.is_none())
    {
        return Err(failed("This tab has no working tree to write in"));
    }
    let git = &*deps.ports().git;
    let root = tab.checkout_root.as_deref().unwrap_or(&tab.repo_root);
    let port = |error: er_git::GitError| failed(error.to_string());
    let not_here = || failed("Staging not available in this mode");
    let mut input = reload_input(content, tab.mode);
    let notice = match write {
        Write::Stage(path) => {
            let verb = match tab.mode {
                DiffMode::Branch | DiffMode::Unstaged => "Staged",
                DiffMode::Conflicts => "Resolved",
                _ => return Err(not_here()),
            };
            git.stage_file(root, &path).map_err(port)?;
            format!("{verb}: {path}")
        }
        Write::Unstage(path) => {
            if tab.mode != DiffMode::Staged {
                return Err(not_here());
            }
            git.unstage_file(root, &path).map_err(port)?;
            format!("Unstaged: {path}")
        }
        Write::StageAll => {
            git.stage_all(root).map_err(port)?;
            "Staged all files".to_string()
        }
        Write::Commit(message) => {
            let message = message.trim();
            if message.is_empty() {
                return Err(failed("A commit needs a message"));
            }
            git.commit(root, message).map_err(port)?;
            input.committed_unpushed = true;
            "Committed! Ctrl+P to push".to_string()
        }
        Write::Push => {
            git.push(root)
                .map_err(|error| failed(format!("Push failed: {error}")))?;
            input.committed_unpushed = false;
            "Pushed!".to_string()
        }
    };
    Ok((notice, input))
}

/// A write to a watched file, which git does not track.
pub(crate) enum Watched {
    Snapshot(String),
    Delete(String),
}

/// Runs a watched-file write in the tab's repo off the actor. The diff
/// section does not show watched files, so nothing reloads.
pub(crate) fn spawn_watched(
    state: &WorkspaceState,
    tx: &Sender<ActorMsg>,
    reply: ReplySender,
    key: TabKey,
    write: Watched,
) {
    let Some(content) = state.view().tabs.get(&key).cloned() else {
        return refuse_unknown_tab(reply);
    };
    spawn_effect(tx, reply, move || {
        crate::guard::assert_off_actor();
        let tab = &content.tab;
        if tab.identity().is_remote() {
            return Err(failed("This tab has no working tree to write in"));
        }
        let root = &tab.repo_root;
        let notice = match write {
            Watched::Snapshot(path) => {
                let snapshots = format!("{}/snapshots", content.context.er_dir);
                er_git::save_snapshot(root, &path, &snapshots)
                    .map_err(|error| failed(format!("Snapshot error: {error}")))?;
                "Snapshot updated".to_string()
            }
            Watched::Delete(path) => {
                er_git::delete_watched_file(root, &path)
                    .map_err(|error| failed(format!("Delete failed: {error}")))?;
                format!("Deleted: {path}")
            }
        };
        Ok(Reply::WorktreeWritten { key, notice })
    });
}
