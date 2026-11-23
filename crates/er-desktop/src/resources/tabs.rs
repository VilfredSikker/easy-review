//! The tab commands, the opens and each tab's `view` on the flag path.
//!
//! A command names its tab by the wire key, looked up in the registry's
//! published list, and a browser command acts on the registry's active tab.
//! Every body takes `DesktopState`, so none can wait on the App lock: the App
//! follows the registry on a thread of its own.

use er_engine::TabKey;
use er_protocol::{
    Ack, BrowserLayout as WireLayout, BrowserView, Command, ErError, ErErrorKind, Resource,
    ResourceChanged, ResourceKey, ResourceVersion, TabView,
};
use er_service::{resolve_tab_key, wire_tab_key, AppCommand};

use super::send::Effects;
use crate::browser_webview::{webview_step, TabWebviews};
use crate::commands::{self, content_routes};
use crate::state::DesktopState;
use crate::view_state::{BrowserLayout, BrowserUpdate, TabBrowserState};

/// A tab command's answer, or the command handed back for the next owner.
pub enum Routed {
    Applied(Result<Ack, ErError>),
    Other(Command),
}

/// Applies `command` when it is a tab command or an open.
pub fn apply(desktop: &DesktopState, command: Command, effects: &Effects<'_>) -> Routed {
    let applied = match command {
        Command::NewTab => commands::service_new_tab(desktop)
            .map_err(refused)
            .map(|()| opened(desktop)),
        Command::CloseTab { key } => close(desktop, key),
        Command::SelectTab { key } => resolve(desktop, key).and_then(|key| {
            content_routes::app_command(desktop, AppCommand::SelectTab { key }).map_err(refused)?;
            Ok(tabs_moved(desktop, None))
        }),
        Command::ReorderTabs { key, to_idx } => resolve(desktop, key).and_then(|key| {
            let to_idx = usize::try_from(to_idx).map_err(|e| refused(e.to_string()))?;
            let command = AppCommand::ReorderTabs { key, to_idx };
            content_routes::app_command(desktop, command).map_err(refused)?;
            Ok(tabs_moved(desktop, None))
        }),
        Command::OpenPrReview {
            project_id,
            pr_number,
            replace,
        } => open(desktop, || {
            commands::service_open_pr_review(desktop, project_id, pr_number, Some(replace))
        }),
        Command::OpenRemotePr {
            owner,
            repo,
            number,
            replace,
        } => open(desktop, || {
            commands::service_open_remote_pr(desktop, owner, repo, number, Some(replace))
        }),
        Command::OpenPrUrl { url, replace } => open(desktop, || {
            commands::service_open_pr_url(desktop, url, Some(replace))
        }),
        Command::OpenLocalBranch {
            project_id,
            name,
            replace,
        } => open(desktop, || {
            let tab = content_routes::branch_tab(&project_id, name)?;
            content_routes::open(desktop, tab, replace)
        }),
        Command::OpenProjectBranch {
            project_id,
            branch,
            replace,
        } => open(desktop, || {
            commands::service_open_project_branch(desktop, project_id, branch, replace)
        }),
        Command::OpenWorktree => match (effects.pick_folder)() {
            Some(path) => open(desktop, || commands::service_open_folder(desktop, path)),
            None => Ok(Ack::default()),
        },
        Command::SetActiveProject { project_id } => open(desktop, || {
            commands::service_set_active_project(desktop, &project_id)
        }),
        Command::PrefetchPrOpen {
            project_id,
            pr_number,
            hint,
        } => {
            let hint = commands::PrOpenHint {
                base_ref: hint.base_ref,
                head_ref: hint.head_ref,
                head_oid: hint.head_oid,
                updated_at: hint.updated_at,
                title: String::new(),
                author: String::new(),
            };
            commands::prefetch_pr_open_impl(project_id, pr_number, hint, desktop)
                .map(|()| Ack::default())
                .map_err(refused)
        }
        Command::PrefetchRemotePrOpen {
            owner,
            repo,
            number,
        } => commands::prefetch_remote_pr_open_impl(owner, repo, number, desktop)
            .map(|()| Ack::default())
            .map_err(refused),
        Command::UpdateTabBrowser {
            layout,
            url,
            navigate,
            annotate,
            tooltips,
            split_ratio,
        } => {
            let update = BrowserUpdate {
                layout: layout.map(|layout| wire_layout_label(layout).to_string()),
                url,
                annotate,
                tooltips,
                split_ratio,
            };
            update_browser(desktop, effects.webviews, update, navigate)
        }
        Command::CycleTabBrowserLayout => cycle_browser_layout(desktop, effects.webviews),
        other => return Routed::Other(other),
    };
    Routed::Applied(applied)
}

/// Closes the tab and drops its content. The `Ack` names the closed tab's
/// view too, which the prune empties.
fn close(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<Ack, ErError> {
    let key = resolve(desktop, wire)?;
    content_routes::close_key(desktop, key).map_err(refused)?;
    Ok(tabs_moved(desktop, None))
}

/// Runs an open; the `Ack` names the tab it left active.
fn open(desktop: &DesktopState, run: impl FnOnce() -> Result<(), String>) -> Result<Ack, ErError> {
    run().map_err(refused)?;
    Ok(opened(desktop))
}

/// The `Ack` of an open: `tabs`, and the tab it left active as created.
pub(super) fn opened(desktop: &DesktopState) -> Ack {
    let active = desktop.service.app_registry().tabs_view().active;
    tabs_moved(desktop, active.map(|key| wire_tab_key(key).0.to_string()))
}

fn tabs_moved(desktop: &DesktopState, created: Option<String>) -> Ack {
    Ack {
        changed: vec![ResourceChanged {
            key: ResourceKey::Tabs,
            version: ResourceVersion(super::host::tabs_version(desktop)),
        }],
        created,
    }
}

fn resolve(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<TabKey, ErError> {
    resolve_tab_key(&desktop.service.app_registry().tabs_view(), wire)
}

fn refused(message: String) -> ErError {
    ErError {
        kind: ErErrorKind::Refused,
        message,
    }
}

/// The registry's active tab and its position, which the webviews are keyed by.
fn active_tab(desktop: &DesktopState) -> Result<(TabKey, usize), ErError> {
    let view = desktop.service.app_registry().tabs_view();
    view.active
        .and_then(|key| Some((key, view.tabs.iter().position(|tab| tab.key == key)?)))
        .ok_or_else(|| ErError {
            kind: ErErrorKind::NotFound,
            message: "no tab is open".into(),
        })
}

/// Writes the active tab's browser state, then drives its webview as the
/// update asks. The write lands before the webview step, which may fail.
pub fn update_browser(
    desktop: &DesktopState,
    webviews: &dyn TabWebviews,
    update: BrowserUpdate,
    navigate: bool,
) -> Result<Ack, ErError> {
    let (key, idx) = active_tab(desktop)?;
    let (show, sync_annotate) = webview_step(&update, navigate);
    let version = write_browser(desktop, key, |browser| browser.apply(update))?;
    if show {
        webviews.show(&desktop.view, key, idx).map_err(refused)?;
    } else if let Some(sync_annotate) = sync_annotate {
        webviews
            .sync_chrome(&desktop.view, key, idx, sync_annotate)
            .map_err(refused)?;
    }
    Ok(view_moved(key, version))
}

/// Hidden, split, fullscreen, hidden on the active tab, then shows the page.
pub fn cycle_browser_layout(
    desktop: &DesktopState,
    webviews: &dyn TabWebviews,
) -> Result<Ack, ErError> {
    let (key, idx) = active_tab(desktop)?;
    let version = write_browser(desktop, key, |browser| {
        browser.layout = browser.layout.cycle();
    })?;
    webviews.show(&desktop.view, key, idx).map_err(refused)?;
    Ok(view_moved(key, version))
}

/// Applies `write` to `key`'s browser state and wakes the relay, which
/// pushes the tab's new `view` version.
fn write_browser(
    desktop: &DesktopState,
    key: TabKey,
    write: impl FnOnce(&mut TabBrowserState),
) -> Result<u64, ErError> {
    let version = {
        let mut view = desktop.view.lock().map_err(|e| refused(e.to_string()))?;
        write(view.browser_mut(key));
        view.browser_revision(key)
    };
    desktop.session.wake();
    Ok(version)
}

fn view_moved(key: TabKey, version: u64) -> Ack {
    Ack {
        changed: vec![ResourceChanged {
            key: ResourceKey::View {
                tab: wire_tab_key(key),
            },
            version: ResourceVersion(version),
        }],
        created: None,
    }
}

/// One listed tab's view at the tick its browser state was last written.
pub fn view(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<(u64, Resource), ErError> {
    let key = resolve(desktop, wire)?;
    let view = desktop.view.lock().map_err(|e| refused(e.to_string()))?;
    let browser = view.browser(key);
    let value = TabView {
        browser: BrowserView {
            url: browser.url,
            layout: match browser.layout {
                BrowserLayout::Hidden => WireLayout::Hidden,
                BrowserLayout::Split => WireLayout::Split,
                BrowserLayout::Fullscreen => WireLayout::Fullscreen,
            },
            split_ratio: browser.split_ratio,
            annotate_mode: browser.annotate_mode,
            show_tooltips: browser.show_tooltips,
        },
    };
    Ok((view.browser_revision(key), Resource::View(value)))
}

/// `key`'s view version, read without building it.
pub fn view_version(desktop: &DesktopState, wire: er_protocol::TabKey) -> Result<u64, ErError> {
    let key = resolve(desktop, wire)?;
    let view = desktop.view.lock().map_err(|e| refused(e.to_string()))?;
    Ok(view.browser_revision(key))
}

fn wire_layout_label(layout: WireLayout) -> &'static str {
    match layout {
        WireLayout::Hidden => BrowserLayout::Hidden.as_str(),
        WireLayout::Split => BrowserLayout::Split.as_str(),
        WireLayout::Fullscreen => BrowserLayout::Fullscreen.as_str(),
    }
}
