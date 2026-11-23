//! The review-flow commands on the flag path. Each names a tab, or takes
//! the registry's active one, and builds its context from that tab's
//! published content and the settings actor's view, then sends the command to
//! the workspace actor that runs the flow. A run read takes the published view
//! and the store and never waits on the actor (ADR 0067). The App is out of
//! reach here: every function takes `DesktopState`.
//!
//! Only compiled with `v1-service`.

use std::sync::atomic::Ordering;

use er_engine::arena::ArenaRunSnapshot;
use er_engine::ports::Ports;
use er_protocol::{ErError, ErErrorKind};
use er_service::{AcceptanceDiff, Command, FlowCommand, FlowContext, Reply, TabEntry, TabsView};

use crate::arena_service::{self, ArenaTab};
use crate::service::{actor_key, ServiceState};
use crate::state::DesktopState;

/// The actor a listed tab's flows run on.
fn tab_key(entry: &TabEntry) -> String {
    actor_key(&entry.descriptor)
}

fn error(kind: ErErrorKind, message: impl Into<String>) -> ErError {
    ErError {
        kind,
        message: message.into(),
    }
}

fn failed(error: impl std::fmt::Display) -> ErError {
    self::error(ErErrorKind::Refused, error.to_string())
}

/// The listed tab `wire` names, else the registry's active one, loaded or
/// not: a run is read and accepted by its bucket, not by the tab's content.
pub fn entry(
    desktop: &DesktopState,
    wire: Option<er_protocol::TabKey>,
) -> Result<TabEntry, ErError> {
    let tabs = desktop.service.app_registry().tabs_view();
    let key = match wire {
        Some(wire) => er_service::resolve_tab_key(&tabs, wire)?,
        None => tabs
            .active
            .ok_or_else(|| error(ErErrorKind::NotFound, "no tab is open"))?,
    };
    tabs.tabs
        .iter()
        .find(|entry| entry.key == key)
        .cloned()
        .ok_or_else(|| error(ErErrorKind::NotFound, "no open tab has that key"))
}

/// The settings actor's config and the saved selection's effort (A18), and
/// the view `scope` names on `tab` when the command reviews one, read from
/// the tab's content by the App's own rules.
pub fn context(desktop: &DesktopState, tab: &ArenaTab, scope: Option<&str>) -> FlowContext {
    let config = arena_service::config(desktop);
    let effort = config
        .ai_hub
        .resolve_default_selection(&config.agent)
        .effort;
    let view = match scope {
        Some(scope) => er_service::review_flow_inputs(&desktop.ports, &tab.content, &config, scope)
            .map_err(|error| error.to_string()),
        None => Err("this command reviews no view".into()),
    };
    FlowContext {
        config,
        effort,
        view,
    }
}

/// The actor that runs the flow on `er_dir`: the started actor whose view
/// holds it, else the one for the listed tab on the bucket, else
/// `fallback`'s. A run is read and accepted after the user may have switched
/// tabs, and tabs of different projects map to different actors, so the
/// active tab can name the wrong one.
pub fn flow_key(desktop: &DesktopState, er_dir: &str, fallback: &TabEntry) -> String {
    desktop
        .service
        .flow_key(er_dir)
        .or_else(|| {
            let tabs = desktop.service.app_registry().tabs_view();
            tabs.tabs
                .iter()
                .find(|entry| {
                    arena_service::listed_bucket(desktop, entry).as_deref() == Some(er_dir)
                })
                .map(tab_key)
        })
        .unwrap_or_else(|| tab_key(fallback))
}

/// Sends one flow command and bumps the desktop revision, as the flag-off
/// commands do, for the readers still on the snapshot.
fn send(desktop: &DesktopState, key: &str, command: FlowCommand) -> Result<Reply, ErError> {
    let reply = desktop
        .service
        .send(key, Command::Flow(command))
        .map_err(failed)?;
    desktop.desktop_revision.fetch_add(1, Ordering::Relaxed);
    Ok(reply)
}

/// A command that controls `tab`'s flow, refused when the page painted
/// another bucket than the tab's content has now.
fn control(
    desktop: &DesktopState,
    tab: &ArenaTab,
    er_dir: &str,
    command: FlowCommand,
) -> Result<(), ErError> {
    if tab.content.context.er_dir != er_dir {
        return Err(error(
            ErErrorKind::Refused,
            "The active review bucket changed. Refresh before controlling this review.",
        ));
    }
    send(desktop, &tab_key(&tab.entry), command).map(drop)
}

/// The flow command a control names. Acceptance carries more and goes
/// through [`accept`].
pub enum Control {
    /// `scope` as the page names it: `current`, `pr` or a git scope.
    Start(String),
    Confirm,
    Decline,
    /// A re-run reviews the branch view, as the driver's resume does.
    Rerun,
    Cancel,
    RunStandard,
}

pub fn run(
    desktop: &DesktopState,
    tab: &ArenaTab,
    er_dir: String,
    control: Control,
) -> Result<(), ErError> {
    let command = match control {
        Control::Start(scope) => {
            let scope = crate::commands::resolve_review_scope_in(&scope, tab.content.tab.mode)
                .map_err(failed)?;
            FlowCommand::Start {
                context: context(desktop, tab, Some(&scope)),
                scope,
            }
        }
        Control::Confirm => FlowCommand::Confirm {
            context: context(desktop, tab, None),
            er_dir: er_dir.clone(),
        },
        Control::Decline => FlowCommand::Decline {
            er_dir: er_dir.clone(),
        },
        Control::Rerun => FlowCommand::Rerun {
            context: context(desktop, tab, Some("branch")),
            er_dir: er_dir.clone(),
        },
        Control::Cancel => FlowCommand::Cancel {
            er_dir: er_dir.clone(),
        },
        Control::RunStandard => FlowCommand::RunStandard {
            context: context(desktop, tab, None),
            er_dir: er_dir.clone(),
        },
    };
    self::control(desktop, tab, &er_dir, command)
}

/// Acceptance targets the bucket the run captured, in the actor that runs its
/// flow, whichever tab is active. The actor checks the run against the diff
/// the listed tab that shows the run's target has now, fetched fresh here by
/// the App's own rules. Answers how many findings it took.
pub fn accept(
    desktop: &DesktopState,
    fallback: &TabEntry,
    er_dir: String,
    run_id: String,
    finding_ids: Vec<String>,
) -> Result<usize, ErError> {
    let key = flow_key(desktop, &er_dir, fallback);
    let view = desktop.service.view(&key);
    let current = view
        .review_flow(&er_dir)
        .and_then(|live| live.run_targets.iter().find(|(id, _)| *id == run_id))
        .map(|(_, target)| {
            let tabs = desktop.service.app_registry().tabs_view();
            let contents: Vec<_> = tabs
                .tabs
                .iter()
                .filter_map(|entry| desktop.service.content(entry.key, &entry.descriptor))
                .collect();
            AcceptanceDiff {
                target: target.clone(),
                diff: er_service::current_review_diff(
                    &desktop.ports,
                    contents.iter().map(AsRef::as_ref),
                    target,
                )
                .map_err(|error| format!("{error:#}")),
            }
        });
    let command = FlowCommand::AcceptFindings {
        er_dir,
        run_id,
        finding_ids,
        current,
    };
    match send(desktop, &key, command)? {
        Reply::FlowAccepted { accepted } => Ok(accepted),
        other => Err(error(
            ErErrorKind::Internal,
            format!("unexpected answer from the workspace: {other:?}"),
        )),
    }
}

/// A flow run from its captured bucket, checked against the flow and target
/// the actor that runs the flow published, whichever tab is active.
pub fn get_run(
    desktop: &DesktopState,
    fallback: &TabEntry,
    er_dir: &str,
    run_id: &str,
) -> Result<ArenaRunSnapshot, ErError> {
    let key = flow_key(desktop, er_dir, fallback);
    let config = arena_service::config(desktop);
    er_service::read_flow_run(&desktop.service.view(&key), &config, er_dir, run_id).map_err(failed)
}

/// Recovers every listed tab's flow bucket in its workspace actor at
/// startup, before any tab has loaded: each bucket is the one its descriptor
/// resolves to, read with the git calls a restore makes. The commands are
/// sent without waiting: recovery marks records and starts nothing, and the
/// published view shows what it found.
pub fn reconcile_on_load(tabs: &TabsView, ports: &Ports, service: &ServiceState) {
    let mut buckets: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for entry in &tabs.tabs {
        let Ok(bucket) = er_service::descriptor_bucket(&*ports.git, &entry.descriptor) else {
            continue;
        };
        let listed = buckets.entry(tab_key(entry)).or_default();
        if !listed.contains(&bucket) {
            listed.push(bucket);
        }
    }
    for (key, buckets) in buckets {
        let _ = service
            .handle_for(&key)
            .send(Command::Reconcile { buckets });
    }
}
