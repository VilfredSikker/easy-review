//! Sidecar reloads: an external writer (er-mcp, an agent) reaches a loaded
//! tab's AI section.
//!
//! The desktop's poll tick asks every couple of seconds, so a reload must cost
//! nothing when nothing moved: the read runs off the actor, compares the
//! sidecars' stamp with the one the section was read under, and publishes only
//! when they differ. One reload per tab runs at a time; a tick that arrives
//! while one runs is answered at once.

use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use er_engine::TabKey;

use crate::actor::{broadcast, publish, WorkspaceDeps};
use crate::content::{ContentPatch, ReloadedAi, TabContent};
use crate::handle::{ActorMsg, ReplySender, WorkspaceError};
use crate::protocol::{Event, Reply};
use crate::workspace::{WorkspaceState, WorkspaceView};

/// What a reload read, for the actor to apply.
pub(crate) struct Reloaded {
    key: TabKey,
    /// The AI revision the read started from.
    since: u64,
    /// `None` when the sidecars had not moved.
    ai: Option<ReloadedAi>,
    reply: ReplySender,
}

/// The tabs whose reload is running. Lives on the actor thread.
#[derive(Default)]
pub(crate) struct SidecarReloads {
    running: HashSet<TabKey>,
}

impl SidecarReloads {
    pub(crate) fn start(
        &mut self,
        deps: &WorkspaceDeps,
        state: &WorkspaceState,
        tx: &Sender<ActorMsg>,
        key: TabKey,
        reply: ReplySender,
    ) {
        let Some(content) = state.view().tabs.get(&key).cloned() else {
            let _ = reply.send(Err(WorkspaceError::Failed(
                "no loaded tab has that key".into(),
            )));
            return;
        };
        if !self.running.insert(key) {
            let _ = reply.send(Ok(Reply::SidecarsReloaded {
                key,
                reloaded: false,
            }));
            return;
        }
        let (deps, tx) = (deps.clone(), tx.clone());
        // Unnamed, like every effect thread (see `actor::spawn_effect`).
        let _ = thread::Builder::new().spawn(move || {
            let ai = crate::effect::reload_ai(&deps, &content, false);
            let since = content.ai.revision;
            let _ = tx.send(ActorMsg::Reloaded(Box::new(Reloaded {
                key,
                since,
                ai,
                reply,
            })));
        });
    }

    /// Applies a finished read. A read that found nothing new, or whose AI
    /// section moved meanwhile, publishes nothing: the tick reads again.
    pub(crate) fn finish(
        &mut self,
        state: &mut WorkspaceState,
        done: Box<Reloaded>,
        subscribers: &Mutex<Vec<Sender<Event>>>,
        view: &RwLock<Arc<WorkspaceView>>,
    ) {
        let Reloaded {
            key,
            since,
            ai,
            reply,
        } = *done;
        self.running.remove(&key);
        let current = state.view().tabs.get(&key).map(|c| c.ai.revision);
        let reloaded = match ai {
            Some(ai) if current == Some(since) => {
                let patch = ContentPatch::Ai {
                    key,
                    since,
                    ai: Box::new(ai),
                };
                match state.apply_patch(patch) {
                    Ok((revision, _)) => {
                        publish(view, state);
                        broadcast(subscribers, Event::ViewPublished { revision });
                        true
                    }
                    Err(_) => false,
                }
            }
            _ => false,
        };
        let _ = reply.send(Ok(Reply::SidecarsReloaded { key, reloaded }));
    }
}

/// The loaded tabs that read the sidecars in `er_dir`.
pub(crate) fn readers(state: &WorkspaceState, er_dir: &str) -> Vec<Arc<TabContent>> {
    state
        .view()
        .tabs
        .values()
        .filter(|content| content.context.er_dir == er_dir)
        .cloned()
        .collect()
}

/// `patch` with every reader's AI state read again first. A write that edits
/// sidecars a patch cannot carry (a finding leaving the review, which merges
/// several files) reaches the tabs this way.
pub(crate) fn with_reloaded_ai(
    deps: &WorkspaceDeps,
    readers: &[Arc<TabContent>],
    patch: ContentPatch,
) -> ContentPatch {
    let mut patches: Vec<ContentPatch> = readers
        .iter()
        .filter_map(|content| {
            let ai = crate::effect::reload_ai(deps, content, true)?;
            Some(ContentPatch::Ai {
                key: content.key,
                since: content.ai.revision,
                ai: Box::new(ai),
            })
        })
        .collect();
    patches.push(patch);
    ContentPatch::Several(patches)
}
