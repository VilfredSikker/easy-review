//! The working-tree writes (decision 8): stage, unstage, stage all, commit
//! and push. Each runs the `Git` port on an effect thread, then reloads the
//! tab in the view it shows.
//!
//! A git write cannot be taken back, so once it has run the command answers
//! with its notice whatever the reload meets. A reload refused because the
//! tab moved meanwhile starts again from the content that moved it, with the
//! write's committed view applied, so the tab still shows the write.
//!
//! A watched file's snapshot and delete also run off the actor, under
//! er-git's path checks; git does not track those files, so nothing reloads.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use er_engine::tab_identity::DiffMode;
use er_engine::TabKey;

use crate::actor::{broadcast, publish, refuse_unknown_tab, reload_input, WorkspaceDeps};
use crate::content::{ContentPatch, LoadedTab, PatchRefused, TabContent};
use crate::handle::{ActorMsg, ReplySender, WorkspaceError};
use crate::protocol::{Event, Reply};
use crate::workspace::{WorkspaceState, WorkspaceView};

/// How many times a reload refused by a moved tab starts again.
const RELOAD_TRIES: u8 = 3;

/// One working-tree write, as its command names it.
pub(crate) enum Write {
    Stage(String),
    Unstage(String),
    StageAll,
    Commit(String),
    Push,
}

/// A write git has done, on its way to the tab's reload.
pub(crate) struct Written {
    key: TabKey,
    notice: String,
    /// The committed view a commit sets and a push clears; `None` keeps it.
    committed: Option<bool>,
    reply: ReplySender,
    tries: u8,
}

/// What the effect threads hand back to the actor.
pub(crate) enum Step {
    /// Git wrote; reload over the content the write started from.
    Written(Written, Arc<TabContent>),
    Loaded {
        written: Written,
        since: u64,
        loaded: Result<Box<LoadedTab>, WorkspaceError>,
    },
}

/// Runs `write` on `key`'s checkout off the actor. A refused write answers
/// with the port's message; a done one goes on to the reload.
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
    let (deps, tx) = (deps.clone(), tx.clone());
    // Unnamed, as `spawn_effect`'s threads are, for the storage guard.
    let _ = thread::Builder::new().spawn(move || {
        let message = match run(&deps, &content, write) {
            Ok((notice, committed)) => {
                let written = Written {
                    key,
                    notice,
                    committed,
                    reply,
                    tries: 0,
                };
                ActorMsg::Worktree(Box::new(Step::Written(written, content)))
            }
            Err(error) => ActorMsg::EffectDone {
                result: Err(error),
                reply,
            },
        };
        let _ = tx.send(message);
    });
}

/// Applies one step on the actor thread.
pub(crate) fn step(
    deps: &WorkspaceDeps,
    state: &mut WorkspaceState,
    tx: &Sender<ActorMsg>,
    step: Step,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<WorkspaceView>>,
) {
    let (written, since, loaded) = match step {
        Step::Written(written, base) => return reload(deps, tx, written, &base),
        Step::Loaded {
            written,
            since,
            loaded,
        } => (written, since, loaded),
    };
    match loaded {
        Ok(loaded) => {
            let key = written.key;
            let since = Some(since);
            match state.apply_patch(ContentPatch::loaded(key, since, loaded)) {
                Ok((revision, _)) => {
                    publish(view, state);
                    broadcast(subscribers, Event::ViewPublished { revision });
                }
                Err(PatchRefused::Moved) if written.tries < RELOAD_TRIES => {
                    published(view, state, subscribers);
                    let Some(current) = state.view().tabs.get(&key).cloned() else {
                        return answer(written);
                    };
                    let tries = written.tries + 1;
                    return reload(deps, tx, Written { tries, ..written }, &current);
                }
                Err(PatchRefused::Moved) => published(view, state, subscribers),
            }
        }
        Err(error) => {
            let message = error.to_string();
            broadcast(subscribers, Event::CommandFailed { message });
        }
    }
    answer(written);
}

/// Publishes the view as it stands: a refused load may still have landed
/// the marks its clear saved.
fn published(
    view: &RwLock<Arc<WorkspaceView>>,
    state: &WorkspaceState,
    subscribers: &Mutex<Vec<Sender<Event>>>,
) {
    let revision = state.view().revision;
    publish(view, state);
    broadcast(subscribers, Event::ViewPublished { revision });
}

fn answer(written: Written) {
    let Written { key, notice, .. } = written;
    let _ = written
        .reply
        .send(Ok(Reply::WorktreeWritten { key, notice }));
}

/// Reloads the tab from `base` off the actor, with the write's committed view.
fn reload(deps: &WorkspaceDeps, tx: &Sender<ActorMsg>, written: Written, base: &TabContent) {
    let mut input = reload_input(base, base.tab.mode);
    if let Some(committed) = written.committed {
        input.committed_unpushed = committed;
    }
    let since = base.generation();
    let (deps, tx) = (deps.clone(), tx.clone());
    let _ = thread::Builder::new().spawn(move || {
        let loaded = crate::effect::load_tab(&deps, input).map(Box::new);
        let step = Step::Loaded {
            written,
            since,
            loaded,
        };
        let _ = tx.send(ActorMsg::Worktree(Box::new(step)));
    });
}

fn failed(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError::Failed(message.into())
}

/// Runs the port write and answers the status line's notice and the
/// committed view it leaves. A remote PR, and a branch view checked out
/// nowhere, have no working tree to write in; a branch view writes in the
/// worktree its branch is checked out in.
pub(crate) fn run(
    deps: &WorkspaceDeps,
    content: &TabContent,
    write: Write,
) -> Result<(String, Option<bool>), WorkspaceError> {
    let tab = &content.tab;
    let root = match (&tab.local_branch_view, &tab.checkout_root) {
        _ if tab.identity().is_remote() => None,
        (Some(_), checkout) => checkout.as_deref(),
        (None, _) => Some(tab.repo_root.as_str()),
    };
    let Some(root) = root else {
        return Err(failed("This tab has no working tree to write in"));
    };
    let git = &*deps.ports().git;
    let port = |error: er_git::GitError| failed(error.to_string());
    let not_here = || failed("Staging not available in this mode");
    Ok(match write {
        Write::Stage(path) => {
            let verb = match tab.mode {
                DiffMode::Branch | DiffMode::Unstaged => "Staged",
                DiffMode::Conflicts => "Resolved",
                _ => return Err(not_here()),
            };
            git.stage_file(root, &path).map_err(port)?;
            (format!("{verb}: {path}"), None)
        }
        Write::Unstage(path) => {
            if tab.mode != DiffMode::Staged {
                return Err(not_here());
            }
            git.unstage_file(root, &path).map_err(port)?;
            (format!("Unstaged: {path}"), None)
        }
        Write::StageAll => {
            git.stage_all(root).map_err(port)?;
            ("Staged all files".to_string(), None)
        }
        Write::Commit(message) => {
            let message = message.trim();
            if message.is_empty() {
                return Err(failed("A commit needs a message"));
            }
            git.commit(root, message).map_err(port)?;
            // A branch view's diff never shows the last commit.
            let committed = tab.local_branch_view.is_none();
            ("Committed! Ctrl+P to push".to_string(), Some(committed))
        }
        Write::Push => {
            git.push(root)
                .map_err(|error| failed(format!("Push failed: {error}")))?;
            ("Pushed!".to_string(), Some(false))
        }
    })
}

/// A write to a watched file, which git does not track.
pub(crate) enum Watched {
    Snapshot(String),
    Delete(String),
}

/// Runs a watched-file write in the tab's repo off the actor. The diff
/// section does not show watched files, so nothing reloads.
///
/// Each write waits for the one sent before it, as the App ran them in
/// turn: a first snapshot sent just before a delete of the same file would
/// otherwise run after it and fail on the missing file.
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
    let previous = state.chain_watched_write(None);
    let tx = tx.clone();
    // Unnamed, as `spawn_effect`'s threads are, for the storage guard.
    let spawned = thread::Builder::new().spawn(move || {
        if let Some(previous) = previous {
            let _ = previous.join();
        }
        let result = write_watched(&content, key, write).map(Into::into);
        let _ = tx.send(ActorMsg::EffectDone { result, reply });
    });
    state.chain_watched_write(spawned.ok());
}

fn write_watched(
    content: &TabContent,
    key: TabKey,
    write: Watched,
) -> Result<Reply, WorkspaceError> {
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
}

/// The repo's worktrees, as git lists them; run on an effect thread.
pub(crate) fn list(
    deps: &WorkspaceDeps,
    repo_root: &str,
) -> Result<Vec<er_git::Worktree>, WorkspaceError> {
    let listed = deps.ports().git.list_worktrees(repo_root);
    listed.map_err(|error| WorkspaceError::Failed(error.to_string()))
}
