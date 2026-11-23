//! `reviewed(tab)` on the flag path: a listed tab's reviewed marks and
//! counts, answered from the content its workspace last published, and the
//! reviewed writes, each keyed by the tab the page painted (decision A).
//!
//! A fetch follows `diff(tab)`'s rule: a loaded key answers its content, a
//! listed key nothing loaded posts `LoadTab` and answers empty at version
//! 0, and a key whose first load failed answers empty at that revision.

use er_engine::TabKey;
use er_protocol::{
    Ack, Command, ErError, ErErrorKind, Resource, ResourceChanged, ResourceKey, ResourceVersion,
    TabReviewed,
};
use er_service::{resolve_tab_key, reviewed_resource, reviewed_version, wire_tab_key, TabEntry};

use super::content_routes;
use super::diff::{listed, published, Published};
use super::tabs::Routed;
use crate::state::DesktopState;

/// `wire`'s marks at the version their sections were published at.
pub fn reviewed(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<(u64, Resource), ErError> {
    let entry = listed(desktop, wire)?;
    let (version, reviewed) = match published(desktop, &entry) {
        Published::Content(content) => (reviewed_version(&content), reviewed_resource(&content)),
        Published::Failed { revision, .. } => (revision, TabReviewed::default()),
        Published::Nothing => {
            desktop.service.request_load(entry.key, &entry.descriptor);
            (0, TabReviewed::default())
        }
    };
    Ok((version, Resource::Reviewed(reviewed)))
}

/// The version `reviewed(key)` answers at, for the relay. Zero while nothing
/// loaded or failed.
pub fn published_version(desktop: &DesktopState, entry: &TabEntry) -> u64 {
    match published(desktop, entry) {
        Published::Content(content) => reviewed_version(&content),
        Published::Failed { revision, .. } => revision,
        Published::Nothing => 0,
    }
}

/// Applies `command` when it is a reviewed write.
pub fn apply(desktop: &DesktopState, command: Command) -> Routed {
    let (tab, view, build): (_, _, Box<dyn FnOnce(_) -> _>) = match command {
        Command::ToggleReviewed { tab, path } => (
            tab,
            None,
            Box::new(|context| er_service::Command::ToggleReviewed { context, path }),
        ),
        Command::MarkReviewed {
            tab,
            path,
            reviewed,
        } => (
            tab,
            None,
            Box::new(move |context| er_service::Command::MarkReviewed {
                context,
                path,
                reviewed,
            }),
        ),
        Command::BulkReviewPillar {
            tab,
            view,
            pillar_id,
            reviewed,
        } => (
            tab,
            Some(view),
            Box::new(move |context| er_service::Command::BulkReviewPillar {
                context,
                pillar_id,
                reviewed,
            }),
        ),
        other => return Routed::Other(other),
    };
    Routed::Applied(write(desktop, tab, view.as_ref(), build))
}

fn write(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    view: Option<&er_protocol::PaintedView>,
    build: Box<dyn FnOnce(er_service::TabContext) -> er_service::Command + '_>,
) -> Result<Ack, ErError> {
    let key = resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)?;
    content_routes::write_keyed(desktop, key, view, build).map_err(|message| ErError {
        kind: ErErrorKind::Refused,
        message,
    })?;
    Ok(moved(desktop, key))
}

/// The `Ack` of a reviewed write on `key`: its marks, and its diff, whose
/// rows carry each file's mark.
fn moved(desktop: &DesktopState, key: TabKey) -> Ack {
    let view = desktop.service.app_registry().tabs_view();
    let changed = view
        .tabs
        .iter()
        .filter(|entry| entry.key == key)
        .flat_map(|entry| {
            let tab = wire_tab_key(entry.key);
            [
                ResourceChanged {
                    key: ResourceKey::Reviewed { tab },
                    version: ResourceVersion(published_version(desktop, entry)),
                },
                ResourceChanged {
                    key: ResourceKey::Diff { tab },
                    version: ResourceVersion(super::diff::published_version(
                        desktop,
                        entry.key,
                        &entry.descriptor,
                    )),
                },
            ]
        })
        .collect();
    Ack {
        changed,
        created: None,
    }
}
