//! The backend tick: the duties the frontend's `poll` ran before each
//! snapshot, driven by the backend so each duty has one driver.
//!
//! A tick asks every loaded tab's actor to read its sidecars again, so an
//! outside write reaches the content with no frontend call, and has the task
//! host retire finished tasks and start queued ones. Then it runs the App's
//! duties: agent logs and commands and the active tab's sidecar check (the
//! only path that brings an outside write into the App's AI state). Each App
//! duty leaves as its data leaves the App. Then it lists the
//! active tab's worktrees for the sidebar's owner and has the owner publish,
//! which is how writes the owner does not make reach its resources.
//!
//! It wakes on each desktop revision the watcher emits, as the duties did when
//! every `er://revision` made the frontend poll, and on an interval as the
//! floor; `follow_revisions` joins the two for the app. This is the one module of the host that names the App; it is handed
//! the App beside the `DesktopState`.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use er_engine::app::{App, Notification};
use er_protocol::Notice;
use er_service::Command;

use crate::service::actor_key;
use crate::state::DesktopState;

/// The floor between ticks when nothing wakes one: the cadence the poll had
/// when it was the frontend's primary mechanism.
pub const INTERVAL: Duration = Duration::from_secs(2);

/// Runs a tick ahead of the interval. Dropping it stops the tick.
pub struct TickWake(Sender<()>);

impl TickWake {
    pub fn wake(&self) {
        let _ = self.0.send(());
    }
}

/// Starts the tick on its own thread, one tick per `interval` or per wake.
pub fn spawn(desktop: DesktopState, app: Arc<Mutex<App>>, interval: Duration) -> TickWake {
    let (tx, wakes) = mpsc::channel();
    // Unnamed, like every thread that reaches the storage root: a test's
    // process root lends itself only to unnamed threads.
    let _ = std::thread::Builder::new().spawn(move || loop {
        match wakes.recv_timeout(interval) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // A burst of wakes runs one tick.
        while wakes.try_recv().is_ok() {}
        run(&desktop, &app);
    });
    TickWake(tx)
}

/// Starts the tick and runs the revision watcher on this thread, waking the
/// tick after each revision the watcher hands `emit`. Returns once `stop` is
/// set, which stops the tick too.
pub fn follow_revisions(
    desktop: DesktopState,
    app: Arc<Mutex<App>>,
    interval: Duration,
    stop: &AtomicBool,
    emit: &dyn Fn(u64, u64),
) {
    let counter = Arc::clone(&desktop.desktop_revision);
    let wake = spawn(desktop, app, interval);
    crate::revision_watcher::follow(&counter, stop, emit, &|| wake.wake());
}

/// One tick. A duty that changed what the snapshot shows bumps the desktop
/// revision, since the frontend's poll no longer runs the duties and would
/// otherwise wait for the next event; a tick that changed nothing does not,
/// so the tick never wakes itself.
pub fn run(desktop: &DesktopState, app: &Mutex<App>) {
    reload_loaded_tabs(desktop);
    crate::tasks::poll(desktop);
    let moved = {
        let mut app = app.lock().unwrap_or_else(PoisonError::into_inner);
        let before = AppStamp::of(&app);
        app.drain_agent_log();
        app.check_commands();
        // Completion and failure lines land after the commands are checked.
        app.drain_agent_log();
        let ai_reloaded = app.tab_mut().check_ai_files_changed();
        if let Some(notice) = app_notice(app.notification.as_ref(), &desktop.session.app_notice_seq)
        {
            emit_notice(desktop, &notice);
        }
        ai_reloaded || AppStamp::of(&app) != before
    };
    if let Some((root, base)) = warm_target(desktop) {
        hold_worktrees(desktop, &root, &base);
    }
    desktop.app_owner.publish();
    if moved {
        crate::profile_log::bump_desktop_revision(&desktop.desktop_revision, "backend_tick");
    }
}

/// The App's notification as a notice, once: `sent` holds the engine's
/// sequence number of the last one sent, and the App keeps its last
/// notification set, so every later tick sees it again.
pub fn app_notice(notification: Option<&Notification>, sent: &AtomicU64) -> Option<Notice> {
    let notification = notification?;
    let before = sent.fetch_max(notification.seq, Ordering::AcqRel);
    (before < notification.seq)
        .then(|| super::notice::engine_notice(notification.message.clone(), notification.long))
}

fn emit_notice(desktop: &DesktopState, notice: &Notice) {
    let handle = desktop
        .tauri_app_handle
        .lock()
        .ok()
        .and_then(|handle| handle.clone());
    if let Some(handle) = handle {
        super::notice::emit(&handle, notice);
    }
}

/// The active tab's repository and base branch, as its loaded content holds
/// them; none until the active tab has loaded.
fn warm_target(desktop: &DesktopState) -> Option<(String, String)> {
    let tabs = desktop.service.app_registry().tabs_view();
    let active = tabs.active_entry()?;
    let content = desktop.service.content(active.key, &active.descriptor)?;
    Some((
        content.tab.repo_root.clone(),
        content.tab.base_branch.clone(),
    ))
}

/// Lists the active tab's worktrees for the owner, with the root and base
/// the snapshot lists them from. The listing fills the worktree metadata
/// cache the snapshot reads, as the warm it replaces did.
fn hold_worktrees(desktop: &DesktopState, root: &str, base: &str) {
    let (github, git) = (&*desktop.ports.github, &*desktop.ports.git);
    let rows = crate::snapshot::build_worktrees(github, git, root, base, root);
    let open: BTreeSet<String> = desktop
        .service
        .app_registry()
        .tabs_view()
        .tabs
        .iter()
        .map(|tab| tab.descriptor.repo_root.clone())
        .collect();
    let rows = crate::app_owner::worktree_rows(rows);
    desktop.app_owner.hold_worktrees(root, rows, &open);
}

/// Asks the actor of every loaded tab to read the tab's sidecars again.
/// Posted and never awaited, and only to actors already up, so it neither
/// waits on a queue nor loads a tab. Answers how many it asked.
pub fn reload_loaded_tabs(desktop: &DesktopState) -> usize {
    let tabs = desktop.service.app_registry().tabs_view();
    tabs.tabs
        .iter()
        .filter(|entry| {
            desktop
                .service
                .content(entry.key, &entry.descriptor)
                .is_some()
        })
        .filter(|entry| {
            let command = Command::ReloadSidecars { key: entry.key };
            desktop
                .service
                .post_if_started(&actor_key(&entry.descriptor), command)
        })
        .count()
}

/// What the App duties move that warrants a new snapshot: each tab's pending
/// commands. Agent log lines are left out:
/// a running agent writes them continuously, so a bump per line would wake
/// the tick and the poll every debounce for the whole run. They reach the
/// page with the next snapshot, as they did when the poll drained them.
#[derive(PartialEq, Eq)]
struct AppStamp {
    commands: Vec<usize>,
}

impl AppStamp {
    fn of(app: &App) -> Self {
        Self {
            commands: app.tabs.iter().map(|tab| tab.command_rx.len()).collect(),
        }
    }
}
