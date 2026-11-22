use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    AnnotationScreenshot, AvailableBranches, AvailablePrs, Branches, FileHunks, Inbox, Jobs,
    Panels, Projects, Prs, ReviewJson, ReviewRevisions, Risk, Session, Settings, TabDiff, TabRow,
    TabView, UiAnnotations, Worktrees,
};

/// A tab as the wire names it. The engine's key has no public constructor, so
/// a wire key is resolved by looking it up among the published tabs, never by
/// building an engine key from the number.
///
/// Keys count from 1 in each backend process; a key kept across a restart can
/// name a different tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
pub struct TabKey(
    // JSON over IPC delivers a number, never a BigInt; keys stay below 2^53.
    #[ts(type = "number")] pub u64,
);

/// The revision a resource's one writer stamped when it last replaced it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS,
)]
pub struct ResourceVersion(
    // JSON over IPC delivers a number, never a BigInt; revisions stay below 2^53.
    #[ts(type = "number")] pub u64,
);

/// Names one resource: a piece of published state with one writer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceKey {
    /// The open tabs and the active one.
    Tabs,
    /// The registered projects.
    Projects,
    /// One project's branches, by project id.
    Branches { project: String },
    /// One project's local branches that are neither checked out nor tracked,
    /// computed on each read and never pushed.
    AvailableBranches { project: String },
    /// The Settings page's rows and AI defaults.
    Settings,
    /// One repo's risk rules, by the rules key `load_risk` answered.
    Risk { key: String },
    /// One remote's pull requests, by the exact remote string the projects
    /// record holds (`owner/repo`).
    Prs { remote: String },
    /// One project's pull requests no list shows, computed on each read and
    /// never pushed.
    AvailablePrs { project: String },
    /// The worktrees of the repository an open tab shows, by that tab's root.
    /// The backend tick lists the active tab's.
    Worktrees { root: String },
    /// The app-wide inbox.
    Inbox,
    /// The app chrome: AI pick labels, watcher, version, update check.
    Session,
    /// Which side panels are open.
    Panels,
    /// One open tab's desktop view, by its key.
    View { tab: TabKey },
    /// One open tab's file list and diff summary, by its key.
    Diff { tab: TabKey },
    /// One file's hunks by the hash `diff(tab)` names for it. The tab and
    /// the path find the workspace that holds them. Never pushed: what a
    /// hash names does not change.
    Hunks {
        tab: TabKey,
        path: String,
        hash: String,
    },
    /// One open tab's browser annotations, from its view's bucket.
    UiAnnotations { tab: TabKey },
    /// The app-wide work in flight.
    Jobs,
    /// One open tab's review as JSON, read on each fetch and never pushed.
    ReviewJson { tab: TabKey },
    /// One open tab's review revisions, read on each fetch and never pushed.
    ReviewRevisions { tab: TabKey },
    /// A saved annotation screenshot by its path, read on each fetch and
    /// never pushed.
    AnnotationScreenshot { path: String },
}

/// One resource's content, tagged with the kind of key it answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resource {
    Tabs(Tabs),
    Projects(Projects),
    Branches(Branches),
    AvailableBranches(AvailableBranches),
    Settings(Box<Settings>),
    Risk(Risk),
    Prs(Prs),
    AvailablePrs(AvailablePrs),
    Worktrees(Worktrees),
    Inbox(Inbox),
    Session(Box<Session>),
    Panels(Panels),
    View(TabView),
    Diff(Box<TabDiff>),
    Hunks(FileHunks),
    UiAnnotations(UiAnnotations),
    Jobs(Jobs),
    ReviewJson(ReviewJson),
    ReviewRevisions(ReviewRevisions),
    AnnotationScreenshot(AnnotationScreenshot),
}

/// The open tabs in strip order, and the active one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tabs {
    pub tabs: Vec<TabRow>,
    pub active: Option<TabKey>,
}

/// A resource's content at the version it was read.
///
/// It carries its key, so a batched fetch answer names what each entry is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Versioned<T> {
    pub key: ResourceKey,
    pub version: ResourceVersion,
    pub value: T,
}

/// A resource moved to a new version; the frontend refetches the keys it shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResourceChanged {
    pub key: ResourceKey,
    pub version: ResourceVersion,
}
