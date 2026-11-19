//! The wire between the desktop backend and its frontend.
//!
//! Types here derive serde and `ts_rs::TS`, and the TypeScript the frontend
//! imports is generated from them by the `protocol-gen` binary into
//! `desktop-ui/src/lib/protocol/generated/`. Export runs only through that
//! binary: a type that exported itself would be written by any `cargo test`,
//! so what the committed files hold would depend on which test ran last.
//!
//! The crate names nothing from the workspace. Builders that turn service or
//! desktop state into these types live beside that state, so the wire does not
//! move when an internal type does.

mod command;
mod error;
mod resource;
mod sidebar;

pub use command::{Ack, Command};
pub use error::{ErError, ErErrorKind};
pub use resource::{
    Resource, ResourceChanged, ResourceKey, ResourceVersion, TabKey, Tabs, Versioned,
};
pub use sidebar::{Branch, Branches, Project, Projects};
