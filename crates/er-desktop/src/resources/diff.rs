//! `diff(tab)` on the flag path: a listed tab's file list and diff summary,
//! answered from the content its workspace last published.
//!
//! A fetch never waits on an actor (ADR 0067). A loaded key answers its
//! content; a listed key nothing loaded posts `LoadTab` and answers an empty
//! diff at version 0, below any published section, so the push after the
//! load brings the rows; a key whose first load failed answers why.

use std::sync::Arc;

use er_engine::config::ErConfig;
use er_engine::TabKey;
use er_protocol::{ErError, ErErrorKind, Resource, TabDiff};
use er_service::{
    diff_load_error, diff_resource, diff_version, resolve_tab_key, DiffInputs, TabContent,
    TabEntry,
};

use crate::service::actor_key;
use crate::state::DesktopState;

/// What a tab's workspace has published for it.
enum Published {
    Content(Arc<TabContent>),
    Failed { revision: u64, message: String },
    Nothing,
}

/// `wire`'s diff at the version its sections were published at.
pub fn diff(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<(u64, Resource), ErError> {
    let entry = listed(desktop, wire)?;
    let (version, diff) = match published(desktop, &entry) {
        Published::Content(content) => {
            let config = config(desktop);
            let inputs = inputs(&content, config.as_deref());
            let version = diff_version(&content) + settings_revision(desktop);
            (version, diff_resource(&content, &inputs))
        }
        Published::Failed { revision, message } => (revision, diff_load_error(&message)),
        Published::Nothing => {
            desktop.service.request_load(entry.key, &entry.descriptor);
            (0, TabDiff::default())
        }
    };
    Ok((version, Resource::Diff(Box::new(diff))))
}

/// `wire`'s diff version, read without building it or loading anything.
pub fn version(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<u64, ErError> {
    let entry = listed(desktop, wire)?;
    Ok(published_version(desktop, entry.key, &entry.descriptor))
}

/// The version `diff(key)` answers at, for the relay. Zero while nothing
/// loaded or failed.
pub fn published_version(desktop: &DesktopState, key: TabKey, tab: &er_engine::tabs_record::TabDescriptor) -> u64 {
    let entry = TabEntry {
        key,
        descriptor: tab.clone(),
    };
    match published(desktop, &entry) {
        Published::Content(content) => diff_version(&content) + settings_revision(desktop),
        Published::Failed { revision, .. } => revision,
        Published::Nothing => 0,
    }
}

fn listed(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<TabEntry, ErError> {
    let tabs = desktop.service.app_registry().tabs_view();
    let key = resolve_tab_key(&tabs, wire)?;
    tabs.tabs
        .iter()
        .find(|entry| entry.key == key)
        .cloned()
        .ok_or_else(|| ErError {
            kind: ErErrorKind::NotFound,
            message: "no open tab has that key".into(),
        })
}

fn published(desktop: &DesktopState, entry: &TabEntry) -> Published {
    let Some(view) = desktop
        .service
        .published_view(&actor_key(&entry.descriptor))
    else {
        return Published::Nothing;
    };
    if let Some(content) = view.tab(entry.key) {
        return Published::Content(Arc::clone(content));
    }
    match view.load_errors.get(&entry.key) {
        Some(error) => Published::Failed {
            revision: error.revision,
            message: error.message.clone(),
        },
        None => Published::Nothing,
    }
}

/// The rule tables come from the settings actor's config, when it is up; a
/// diff read never starts it. Its revision is part of the version, so a rule
/// edit moves every tab's diff.
fn config(desktop: &DesktopState) -> Option<Arc<er_service::SettingsView>> {
    desktop.service.settings_if_started().map(|s| s.view())
}

fn settings_revision(desktop: &DesktopState) -> u64 {
    config(desktop).map_or(0, |view| view.revision)
}

fn inputs<'a>(content: &TabContent, config: Option<&er_service::SettingsView>) -> DiffInputs<'a> {
    let meta = &content.diff.meta;
    let fallback = ErConfig::default();
    let config = config.map_or(&fallback, |view| &view.config);
    DiffInputs {
        file_kinds: config
            .file_kinds
            .repo(&meta.rules_key)
            .cloned()
            .unwrap_or_default(),
        risk_rules: config.risk_rules_merged(&meta.rules_key, meta.risk_artifact.clone()),
        ..DiffInputs::default()
    }
}
