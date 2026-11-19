//! The dispatch from each key to its owner, and the versions the relay
//! compares. Only the service build has owners to ask.

use std::collections::BTreeMap;

use er_engine::TabKey;
use er_protocol::{ErError, Resource, ResourceKey, ResourceVersion, Versioned};

use crate::app_owner::SidebarOwner;
use crate::state::DesktopState;

/// `key`'s content at the version its owner last published.
pub fn fetch(desktop: &DesktopState, key: ResourceKey) -> Result<Versioned<Resource>, ErError> {
    let (version, value) = match &key {
        ResourceKey::Tabs => tabs(desktop),
        ResourceKey::Projects => {
            let (version, projects) = sidebar(desktop).projects();
            (version, Resource::Projects(projects))
        }
        ResourceKey::Branches { project } => {
            let (version, branches) = sidebar(desktop).branches(project);
            (version, Resource::Branches(branches))
        }
    };
    Ok(Versioned {
        key,
        version: ResourceVersion(version),
        value,
    })
}

/// `key`'s version, read without building its content.
pub fn version(desktop: &DesktopState, key: ResourceKey) -> Result<ResourceVersion, ErError> {
    let version = match &key {
        ResourceKey::Tabs => desktop.service.app_registry().tabs_view().revision,
        ResourceKey::Projects => sidebar(desktop).projects().0,
        ResourceKey::Branches { project } => sidebar(desktop).branches(project).0,
    };
    Ok(ResourceVersion(version))
}

/// The sidebar's owner, read through the trait that outlives its move into
/// the service.
fn sidebar(desktop: &DesktopState) -> &dyn SidebarOwner {
    &*desktop.app_owner
}

/// The registry's published list. A read never takes the registry's command
/// path, which applies tab commands inline on the caller's thread.
fn tabs(desktop: &DesktopState) -> (u64, Resource) {
    let view = desktop.service.app_registry().tabs_view();
    let tabs = er_service::tabs_resource(&view);
    (view.revision, Resource::Tabs(tabs))
}

/// What the relay compares between publishes: each resource with a wire key,
/// and each section of a loaded tab, which that tab's resources are built
/// from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Watched {
    Tabs,
    /// A resource the sidebar's owner publishes, under its wire key.
    Sidebar(ResourceKey),
    Diff(TabKey),
    Reviewed(TabKey),
    Ai(TabKey),
    Pr(TabKey),
    /// A view the tab records without loading it.
    Mode(TabKey),
}

impl Watched {
    /// The wire key a change here moves; none while no resource reads it.
    pub fn wire(&self) -> Option<ResourceKey> {
        match self {
            Self::Tabs => Some(ResourceKey::Tabs),
            Self::Sidebar(key) => Some(key.clone()),
            Self::Diff(_) | Self::Reviewed(_) | Self::Ai(_) | Self::Pr(_) | Self::Mode(_) => None,
        }
    }
}

/// Every watched version as its owner last published it. Starts no actor.
pub fn watched(desktop: &DesktopState) -> BTreeMap<Watched, u64> {
    let tabs = desktop.service.app_registry().tabs_view();
    let mut versions = BTreeMap::from([(Watched::Tabs, tabs.revision)]);
    let sidebar = sidebar(desktop).versions();
    versions.extend(
        sidebar
            .into_iter()
            .map(|(key, v)| (Watched::Sidebar(key), v)),
    );
    for view in desktop.service.published_views() {
        for (&key, content) in &view.tabs {
            versions.extend([
                (Watched::Diff(key), content.diff.revision),
                (Watched::Reviewed(key), content.reviewed.revision),
                (Watched::Ai(key), content.ai.revision),
                (Watched::Pr(key), content.pr.revision),
                (Watched::Mode(key), content.mode_revision),
            ]);
        }
    }
    versions
}
