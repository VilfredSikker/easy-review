use std::path::PathBuf;

use crate::store::MAX_ARTIFACT_BYTES;

/// Every failure the store reports to a caller.
#[derive(Debug)]
pub enum StoreError {
    /// An I/O error other than a missing file.
    Io(std::io::Error),
    /// A value could not be serialized.
    Serialize(serde_json::Error),
    /// A slug, bucket directory, slot string or relative path is unsafe.
    InvalidComponent { what: &'static str, value: String },
    /// An artifact file template or a resolved relative path is unusable.
    InvalidPath { value: String },
    /// The registry already holds this kind or pattern.
    DuplicateRegistration { kind: &'static str },
    /// The artifact's schema version has no migration.
    UnsupportedVersion { found: u32 },
    /// The file on disk is newer than this build; writes leave it alone.
    NewerVersion {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
    /// A payload or text file is over the store's size limit.
    TooLarge { bytes: u64 },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "store I/O: {error}"),
            Self::Serialize(error) => write!(f, "serialize artifact: {error}"),
            Self::InvalidComponent { what, value } => write!(f, "invalid {what} `{value}`"),
            Self::InvalidPath { value } => write!(f, "invalid artifact path `{value}`"),
            Self::DuplicateRegistration { kind } => {
                write!(f, "artifact kind `{kind}` is already registered")
            }
            Self::UnsupportedVersion { found } => {
                write!(f, "no migration from artifact version {found}")
            }
            Self::NewerVersion {
                path,
                found,
                expected,
            } => write!(
                f,
                "{} is version {found}; this build reads version {expected}",
                path.display()
            ),
            Self::TooLarge { bytes } => write!(
                f,
                "artifact is {bytes} bytes, over the {MAX_ARTIFACT_BYTES} limit"
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialize(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialize(error)
    }
}
