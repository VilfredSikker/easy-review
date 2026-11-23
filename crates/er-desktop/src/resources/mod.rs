//! The resource host: each `ResourceKey` answered from its one owner.
//!
//! The owners are the app registry (`tabs`), the workspace actors through
//! `ServiceState`, the settings actor and the desktop's own owners; each
//! resource joins the dispatch below when its entry lands. A read answers from
//! what the owner last published and never waits on an actor's queue
//! (ADR 0067). Every module here takes `DesktopState`, so no resource can
//! read the App; the tick is the one exception, and it is handed the App.

#[cfg(feature = "v1-service")]
pub mod annotation_writes;
#[cfg(feature = "v1-service")]
pub mod annotations;
#[cfg(feature = "v1-service")]
pub mod arena;
pub mod commands;
#[cfg(feature = "v1-service")]
pub mod diff;
#[cfg(feature = "v1-service")]
pub mod flows;
#[cfg(feature = "v1-service")]
mod host;
#[cfg(feature = "v1-service")]
pub mod hunks;
#[cfg(feature = "v1-service")]
pub mod jobs;
#[cfg(feature = "v1-service")]
pub mod notice;
#[cfg(feature = "v1-service")]
pub mod preview;
#[cfg(feature = "v1-service")]
pub mod relay;
#[cfg(feature = "v1-service")]
pub mod reviewed;
#[cfg(feature = "v1-service")]
pub mod send;
#[cfg(feature = "v1-service")]
pub mod session;
#[cfg(feature = "v1-service")]
pub mod tabs;
#[cfg(feature = "v1-service")]
pub mod tick;
#[cfg(feature = "v1-service")]
pub mod ui_annotation_writes;

#[cfg(feature = "v1-service")]
pub use host::{fetch, fetch_all, send, version, watched, Watched};
