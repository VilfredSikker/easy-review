use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::slot::SlotName;

/// One typed artifact kind.
///
/// `FILE` is a path relative to the scope directory. `{slot}` is replaced by
/// [`SlotName::as_str`]; single-file kinds have no placeholder.
pub trait Artifact: Serialize + DeserializeOwned + JsonSchema {
    /// Kebab-case, for example `github-comments`.
    const KIND: &'static str;
    /// The payload's schema version. The envelope carries this same number.
    const VERSION: u32;
    const FILE: &'static str;
    type Slot: SlotName;

    /// The payload's own diff hash, or `None` when the kind is not tied to a diff.
    fn diff_hash(&self) -> Option<&str> {
        None
    }

    /// Convert a payload from an older version. The default rejects every
    /// version, so a mismatched file reads as an unknown-version payload.
    fn migrate(from: u32, _raw: serde_json::Value) -> Result<serde_json::Value, MigrateError> {
        Err(MigrateError::UnsupportedVersion { found: from })
    }
}

/// Why a payload could not be migrated to this build's schema.
///
/// The store maps this onto its own error: an unsupported version reads as an
/// unknown-version payload, any other failure as a malformed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateError {
    /// No migration exists from the version on disk.
    UnsupportedVersion { found: u32 },
    /// A migration ran and could not produce a payload this build reads.
    Failed { reason: String },
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { found } => {
                write!(f, "no migration from artifact version {found}")
            }
            Self::Failed { reason } => write!(f, "artifact migration failed: {reason}"),
        }
    }
}

impl std::error::Error for MigrateError {}
