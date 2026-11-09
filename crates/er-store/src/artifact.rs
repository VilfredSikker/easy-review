use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::StoreError;
use crate::key::SlotName;

/// How a kind is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The host-only wrapper `{kind, version, diff_hash, producer, written_at, payload}`.
    Enveloped,
    /// The payload JSON by itself, with no wrapper. Agent-written kinds stay
    /// bare until the phase 3 runner commits their output.
    Bare,
}

/// One typed artifact kind.
///
/// `FILE` is a path relative to the scope directory. `{slot}` is replaced by
/// [`SlotName::as_str`]; single-file kinds have no placeholder.
pub trait Artifact: Serialize + DeserializeOwned + JsonSchema {
    /// Kebab-case, for example `github-comments`.
    const KIND: &'static str;
    /// The payload's schema version. The envelope carries this same number.
    const VERSION: u32;
    const MODE: Mode;
    /// The highest version this kind had while it was Bare, if it ever was.
    const WAS_BARE_BEFORE: Option<u32> = None;
    const FILE: &'static str;
    type Slot: SlotName;

    /// The payload's own diff hash, or `None` when the kind is not tied to a diff.
    fn diff_hash(&self) -> Option<&str> {
        None
    }

    /// Convert a payload from an older version. The default rejects every
    /// version, so a mismatched file reads as [`crate::Unreadable::UnknownVersion`].
    fn migrate(from: u32, _raw: serde_json::Value) -> Result<serde_json::Value, StoreError> {
        Err(StoreError::UnsupportedVersion { found: from })
    }
}
