use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Panels, Risk, Session, Settings};

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
    /// The Settings page's rows and AI defaults.
    Settings,
    /// One repo's risk rules, by the rules key `load_risk` answered.
    Risk { key: String },
    /// The app chrome: AI pick labels, watcher, version, update check.
    Session,
    /// Which side panels are open.
    Panels,
}

/// One resource's content, tagged with the kind of key it answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resource {
    Tabs(Tabs),
    Settings(Box<Settings>),
    Risk(Risk),
    Session(Box<Session>),
    Panels(Panels),
}

/// The open tabs in strip order, and the active one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tabs {
    pub tabs: Vec<TabKey>,
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
