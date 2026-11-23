//! The resource host: each `ResourceKey` answered from its one owner.
//!
//! The owners are the app registry (`tabs`), the workspace actors through
//! `ServiceState`, the settings actor and the desktop's own owners; each
//! resource joins the dispatch below when its entry lands. A read answers from
//! what the owner last published and never waits on an actor's queue
//! (ADR 0067). Every module here takes `DesktopState`, so no resource can
//! read the App; the tick is the one exception, and it is handed the App.

pub mod ai_asks;
pub mod annotation_writes;
pub mod annotations;
pub mod arena;
pub mod commands;
pub mod diff;
pub mod flows;
pub mod github_writes;
mod host;
pub mod hunks;
pub mod jobs;
pub mod notice;
pub mod pr;
pub mod preview;
pub mod relay;
pub mod reviewed;
pub mod runs;
pub mod send;
pub mod session;
pub mod tabs;
pub mod tick;
pub mod ui_annotation_writes;

pub use host::{fetch, fetch_all, send, version, watched, Watched};
