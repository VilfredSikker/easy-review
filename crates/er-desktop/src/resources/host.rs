//! The dispatch from each key to its owner, and the versions the relay
//! compares. Only the service build has owners to ask.

use std::collections::BTreeMap;

use er_engine::TabKey;
use er_protocol::{
    Ack, AvailableBranches, AvailablePrs, Command, ErError, ErErrorKind, Resource, ResourceChanged,
    ResourceKey, ResourceVersion, Versioned,
};

use crate::app_owner::SidebarOwner;
use crate::inbox::InboxState;
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
        ResourceKey::AvailableBranches { project } => {
            let branches = crate::commands::project_available_branches(desktop, project)
                .map_err(|message| failed(project, message))?;
            (
                0,
                Resource::AvailableBranches(AvailableBranches { branches }),
            )
        }
        ResourceKey::Settings => settings(desktop),
        ResourceKey::Risk { key } => risk(desktop, key)?,
        ResourceKey::Prs { remote } => {
            let (version, prs) = sidebar(desktop).prs(remote);
            (version, Resource::Prs(prs))
        }
        ResourceKey::Worktrees { root } => {
            let (version, worktrees) = sidebar(desktop).worktrees(root);
            (version, Resource::Worktrees(worktrees))
        }
        ResourceKey::Inbox => {
            let (version, inbox) = sidebar(desktop).inbox();
            (version, Resource::Inbox(inbox))
        }
        ResourceKey::AvailablePrs { project } => {
            let prs = crate::commands::project_available_prs(desktop, project)
                .map_err(|message| failed(project, message))?;
            let prs = prs
                .iter()
                .map(|pr| crate::app_owner::pr_row(pr, None))
                .collect();
            (0, Resource::AvailablePrs(AvailablePrs { prs }))
        }
        ResourceKey::Session => {
            let (version, session) =
                super::session::session(desktop, &desktop.service.settings().view());
            (version, Resource::Session(Box::new(session)))
        }
        ResourceKey::Panels => {
            let (version, panels) = super::session::panels(desktop);
            (version, Resource::Panels(panels))
        }
        ResourceKey::View { tab } => super::tabs::view(desktop, *tab)?,
        ResourceKey::Diff { tab } => super::diff::diff(desktop, *tab)?,
        ResourceKey::Hunks { tab, path, hash } => super::hunks::hunks(desktop, *tab, path, hash)?,
        ResourceKey::FilePreview { tab, path, context } => {
            super::preview::file_preview(desktop, *tab, path, context)?
        }
        ResourceKey::ImagePreview { tab, path, context } => {
            super::preview::image_preview(desktop, *tab, path, context)?
        }
        ResourceKey::DiffPaths { tab } => super::preview::diff_paths(desktop, *tab)?,
    };
    Ok(Versioned {
        key,
        version: ResourceVersion(version),
        value,
    })
}

/// Each key's content, in the order asked. The unparsed files the keys ask
/// for are parsed first, one parse per tab.
pub fn fetch_all(
    desktop: &DesktopState,
    keys: Vec<ResourceKey>,
) -> Result<Vec<Versioned<Resource>>, ErError> {
    super::hunks::parse_asked(desktop, &keys);
    keys.into_iter().map(|key| fetch(desktop, key)).collect()
}

/// `key`'s version, read without building its content.
pub fn version(desktop: &DesktopState, key: ResourceKey) -> Result<ResourceVersion, ErError> {
    let version = match &key {
        ResourceKey::Tabs => desktop.service.app_registry().tabs_view().revision,
        ResourceKey::Projects => sidebar(desktop).projects().0,
        ResourceKey::Branches { project } => sidebar(desktop).branches(project).0,
        // Computed on each read, or immutable under their key, so they never
        // have a newer version to announce.
        ResourceKey::AvailableBranches { .. }
        | ResourceKey::AvailablePrs { .. }
        | ResourceKey::Hunks { .. }
        | ResourceKey::FilePreview { .. }
        | ResourceKey::ImagePreview { .. }
        | ResourceKey::DiffPaths { .. } => 0,
        ResourceKey::Prs { remote } => sidebar(desktop).prs(remote).0,
        ResourceKey::Worktrees { root } => sidebar(desktop).worktrees(root).0,
        ResourceKey::Inbox => sidebar(desktop).inbox().0,
        ResourceKey::Settings => desktop.service.settings().view().revision,
        ResourceKey::Risk { key } => match desktop.service.settings().view().risk.get(key) {
            Some(view) => view.version,
            None => return Err(risk_not_loaded(key)),
        },
        ResourceKey::Session => {
            super::session::session_version(desktop, Some(&desktop.service.settings().view()))
        }
        ResourceKey::Panels => super::session::panels_version(desktop),
        ResourceKey::View { tab } => super::tabs::view_version(desktop, *tab)?,
        ResourceKey::Diff { tab } => super::diff::version(desktop, *tab)?,
    };
    Ok(ResourceVersion(version))
}

/// Applies `command` and answers the versions it moved. A branch change
/// refreshes its project's metadata before the answer, so the `Ack` names
/// that project's `branches` and the frontend refetches them at once. A PR
/// list refresh fetches in the background, and the push brings what it moved.
/// An inbox write publishes before the answer, so the `Ack` names `inbox`;
/// an inbox open also names `tabs` and the tab it left active.
pub fn send(desktop: &DesktopState, command: Command) -> Result<Ack, ErError> {
    let before = sidebar(desktop).versions();
    let git = &*desktop.ports.git;
    let mut opened = None;
    match command {
        Command::ReorderProjects { ordered } => {
            crate::projects::reorder_projects(&ordered).map_err(|e| failed("", e.to_string()))?;
        }
        Command::DeleteProject { project } => {
            crate::commands::delete_project_and_its_prs(desktop, &project)
                .map_err(|message| failed(&project, message))?;
        }
        Command::AddTrackedBranch { project, name } => {
            crate::commands::track_branch(desktop, &project, &name)
                .map_err(|message| failed(&project, message))?;
            desktop.app_owner.refresh_project_meta(git, &project);
        }
        Command::RemoveTrackedBranch { project, name } => {
            crate::commands::untrack_branch(desktop, &project, &name)
                .map_err(|message| failed(&project, message))?;
            desktop.app_owner.refresh_project_meta(git, &project);
        }
        Command::PatchProjectReviewSettings { project, patch } => {
            let patch = crate::projects::ProjectReviewSettingsPatch {
                auto_triage_max_diff_kb: patch.auto_triage_max_diff_kb,
                review_ignore_glob_add: patch.review_ignore_glob_add,
                review_ignore_glob_remove: patch
                    .review_ignore_glob_remove
                    .and_then(|index| usize::try_from(index).ok()),
            };
            crate::projects::patch_project_review_settings(&project, patch)
                .map_err(|e| failed(&project, e.to_string()))?;
        }
        Command::RefreshPrList => refresh_prs(desktop, None)?,
        Command::RefreshProjectPrList { project } => {
            let file = crate::projects::load();
            let Some(record) = file.projects.iter().find(|p| p.id == project) else {
                return Err(failed(&project, format!("Project not found: {project}")));
            };
            if let Some(remote) = record.remote.clone() {
                refresh_prs(desktop, Some(remote))?;
            }
        }
        Command::DismissPr { project, number } => {
            crate::projects::dismiss_pr(&project, number);
        }
        Command::UndismissPr { project, number } => {
            crate::projects::undismiss_pr(&project, number)
                .map_err(|e| failed(&project, e.to_string()))?;
        }
        Command::TrackPr { project, number } => {
            crate::projects::track_pr(&project, number)
                .map_err(|e| failed(&project, e.to_string()))?;
        }
        Command::UntrackPr { project, number } => {
            crate::projects::untrack_pr(&project, number)
                .map_err(|e| failed(&project, e.to_string()))?;
        }
        Command::SavePr {
            project,
            number,
            title,
        } => {
            let title =
                crate::commands::resolve_pr_title_for_project(&project, number, title, desktop)
                    .map_err(|message| failed(&project, message))?;
            crate::projects::save_pr(&project, number, &title)
                .map_err(|e| failed(&project, e.to_string()))?;
        }
        Command::UnsavePr { project, number } => {
            crate::projects::unsave_pr(&project, number)
                .map_err(|e| failed(&project, e.to_string()))?;
        }
        Command::MarkInboxItemRead { id } => {
            let now = crate::inbox::now_epoch_ms();
            write_inbox(desktop, |inbox| {
                inbox.mark_item_read(&id, now);
            })?;
        }
        Command::MarkAllInboxRead => {
            let now = crate::inbox::now_epoch_ms();
            write_inbox(desktop, |inbox| inbox.mark_all_read(now))?;
        }
        Command::MarkInboxItemsRead { ids } => {
            let now = crate::inbox::now_epoch_ms();
            write_inbox(desktop, |inbox| inbox.mark_items_read(&ids, now))?;
        }
        Command::ClearReadInboxItems => write_inbox(desktop, InboxState::clear_read)?,
        Command::ClearInboxItems { ids } => {
            write_inbox(desktop, |inbox| inbox.clear_read_items(&ids))?;
        }
        Command::RefreshNotifications => refresh_prs(desktop, None)?,
        Command::TestNativeNotification => {
            crate::commands::show_test_notification(desktop).map_err(|message| ErError {
                kind: ErErrorKind::Refused,
                message,
            })?;
        }
        Command::OpenInboxItem { id, replace } => {
            crate::commands::service_open_inbox_item(desktop, &id, replace).map_err(refused)?;
            opened = Some(super::tabs::opened(desktop));
        }
        Command::PrefetchInboxItem { id } => {
            crate::commands::prefetch_inbox_target(&id, desktop).map_err(refused)?;
        }
        // Other owners apply these; `send::apply` routes them there.
        other @ (Command::ReloadSettings
        | Command::ApplyConfigPatch { .. }
        | Command::SetReviewFlowModels { .. }
        | Command::RefreshAiModels { .. }
        | Command::UpsertAiProvider { .. }
        | Command::DeleteAiProvider { .. }
        | Command::UpsertAiModel { .. }
        | Command::DeleteAiModel { .. }
        | Command::PreviewUninstall { .. }
        | Command::RunUninstall { .. }
        | Command::LoadRisk { .. }
        | Command::SetAiSelection { .. }
        | Command::SetAiEffort { .. }
        | Command::CheckAppUpdate
        | Command::TogglePanel { .. }
        | Command::NewTab
        | Command::CloseTab { .. }
        | Command::SelectTab { .. }
        | Command::ReorderTabs { .. }
        | Command::OpenPrReview { .. }
        | Command::OpenRemotePr { .. }
        | Command::OpenPrUrl { .. }
        | Command::OpenLocalBranch { .. }
        | Command::OpenProjectBranch { .. }
        | Command::OpenWorktree
        | Command::SetActiveProject { .. }
        | Command::PrefetchPrOpen { .. }
        | Command::PrefetchRemotePrOpen { .. }
        | Command::UpdateTabBrowser { .. }
        | Command::CycleTabBrowserLayout
        | Command::SetDiffMode { .. }
        | Command::ChangeBase { .. }
        | Command::SetFilter { .. }
        | Command::RefreshDiff { .. }
        | Command::SyncPr { .. }
        | Command::SyncBranch { .. }
        | Command::SetFileCompacted { .. }) => {
            return Err(ErError {
                kind: ErErrorKind::Internal,
                message: format!("not a sidebar command: {other:?}"),
            });
        }
    }
    desktop.app_owner.publish();
    // The snapshot still carries the projects for the readers not yet on
    // these resources.
    crate::profile_log::bump_desktop_revision(&desktop.desktop_revision, "sidebar_command");
    let mut changed: Vec<_> = sidebar(desktop)
        .versions()
        .into_iter()
        .filter(|entry| !before.contains(entry))
        .map(|(key, version)| ResourceChanged {
            key,
            version: ResourceVersion(version),
        })
        .collect();
    let created = opened.and_then(|tabs| {
        changed.extend(tabs.changed);
        tabs.created
    });
    Ok(Ack { changed, created })
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}

/// One inbox write through the sidebar's owner, which publishes before it
/// saves. The inbox's kind filter is the settings actor's, so this starts
/// the actor, and the publish builds `inbox` for the `Ack` to name.
fn write_inbox(desktop: &DesktopState, write: impl FnOnce(&mut InboxState)) -> Result<(), ErError> {
    desktop.service.settings();
    desktop
        .app_owner
        .write_inbox(write)
        .map_err(|message| ErError {
            kind: ErErrorKind::Internal,
            message,
        })
}

/// Starts a PR-list fetch. The inbox prefs come from the settings actor,
/// which owns the config; the App's copy is not this module's to read.
fn refresh_prs(desktop: &DesktopState, remote: Option<String>) -> Result<(), ErError> {
    let service = desktop.service.clone();
    let prefs = move || service.settings().view().config.inbox.clone();
    crate::commands::kick_pr_list_refresh(desktop, remote, prefs).map_err(|message| ErError {
        kind: ErErrorKind::Internal,
        message,
    })
}

/// A refused project command or read; a project the record does not hold is
/// not found.
fn failed(project: &str, message: String) -> ErError {
    let missing = !project.is_empty()
        && !crate::projects::load()
            .projects
            .iter()
            .any(|p| p.id == project);
    ErError {
        kind: if missing {
            ErErrorKind::NotFound
        } else {
            ErErrorKind::Refused
        },
        message,
    }
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

/// The settings actor's published view. The first read starts the actor,
/// which loads the config on this thread.
fn settings(desktop: &DesktopState) -> (u64, Resource) {
    let view = desktop.service.settings().view();
    (
        view.revision,
        Resource::Settings(Box::new(er_service::settings_resource(&view))),
    )
}

/// One rules key's risk view, as the settings actor last published it. A
/// key no load named is not there: the load is what resolves a repo to its
/// key, so the frontend sends `load_risk` before it reads.
fn risk(desktop: &DesktopState, key: &str) -> Result<(u64, Resource), ErError> {
    let view = desktop.service.settings().view();
    let version = view.risk.get(key).map(|risk| risk.version);
    match (version, er_service::risk_resource(&view, key)) {
        (Some(version), Some(risk)) => Ok((version, Resource::Risk(risk))),
        _ => Err(risk_not_loaded(key)),
    }
}

fn risk_not_loaded(key: &str) -> ErError {
    ErError {
        kind: ErErrorKind::NotFound,
        message: format!("no risk rules were loaded for {key}"),
    }
}

/// What the relay compares between publishes: each resource with a wire key,
/// and each section of a loaded tab, which that tab's resources are built
/// from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Watched {
    Tabs,
    /// A resource the sidebar's owner publishes, under its wire key.
    Sidebar(ResourceKey),
    Settings,
    /// One rules key's risk view.
    Risk(String),
    Session,
    Panels,
    Diff(TabKey),
    Reviewed(TabKey),
    Ai(TabKey),
    Pr(TabKey),
    /// A view the tab records without loading it.
    Mode(TabKey),
    /// A tab's desktop view, under the key the registry issued.
    View(TabKey),
    /// A listed tab's `diff(tab)`: every section its rows read, or its load
    /// failure.
    TabDiff(TabKey),
}

impl Watched {
    /// The wire key a change here moves; none while no resource reads it.
    pub fn wire(&self) -> Option<ResourceKey> {
        match self {
            Self::Tabs => Some(ResourceKey::Tabs),
            Self::Sidebar(key) => Some(key.clone()),
            Self::Settings => Some(ResourceKey::Settings),
            Self::Risk(key) => Some(ResourceKey::Risk { key: key.clone() }),
            Self::Session => Some(ResourceKey::Session),
            Self::Panels => Some(ResourceKey::Panels),
            Self::View(key) => Some(ResourceKey::View {
                tab: er_service::wire_tab_key(*key),
            }),
            Self::TabDiff(key) => Some(ResourceKey::Diff {
                tab: er_service::wire_tab_key(*key),
            }),
            Self::Diff(_) | Self::Reviewed(_) | Self::Ai(_) | Self::Pr(_) | Self::Mode(_) => None,
        }
    }
}

/// Every watched version as its owner last published it. Starts no actor:
/// until something reads `settings`, the settings actor is not watched.
pub fn watched(desktop: &DesktopState) -> BTreeMap<Watched, u64> {
    let tabs = desktop.service.app_registry().tabs_view();
    let settings = desktop.service.settings_if_started().map(|s| s.view());
    let mut versions = BTreeMap::from([
        (Watched::Tabs, tabs.revision),
        (
            Watched::Session,
            super::session::session_version(desktop, settings.as_deref()),
        ),
        (Watched::Panels, super::session::panels_version(desktop)),
    ]);
    let sidebar = sidebar(desktop).versions();
    versions.extend(
        sidebar
            .into_iter()
            .map(|(key, v)| (Watched::Sidebar(key), v)),
    );
    if let Ok(view) = desktop.view.lock() {
        versions.extend(
            view.browser_revisions()
                .map(|(key, tick)| (Watched::View(key), tick)),
        );
    }
    if let Some(view) = settings {
        versions.insert(Watched::Settings, view.revision);
        versions.extend(
            view.risk
                .iter()
                .map(|(key, risk)| (Watched::Risk(key.clone()), risk.version)),
        );
    }
    // Only a tab that loaded or failed has a diff to announce; a tab still
    // at version 0 announces nothing until it does.
    versions.extend(tabs.tabs.iter().filter_map(|entry| {
        let version = super::diff::published_version(desktop, entry.key, &entry.descriptor);
        (version > 0).then_some((Watched::TabDiff(entry.key), version))
    }));
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
