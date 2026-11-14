//! The artifact contract now lives in `er-core`; the store re-exports it so a
//! caller written against `er_store::Artifact` keeps compiling.

pub use er_core::artifact::{Artifact, MigrateError};
