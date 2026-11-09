use std::path::PathBuf;

use crate::lock::HolderInfo;
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
    /// Another process or thread holds the bucket's write lock.
    Locked {
        path: PathBuf,
        holder: Option<HolderInfo>,
    },
    /// Another process or thread holds the bucket's lease.
    LeaseHeld {
        path: PathBuf,
        holder: Option<HolderInfo>,
    },
    /// A lock guard does not cover the key's scope.
    WrongScope { guard: PathBuf, key: PathBuf },
    /// The current thread already holds this bucket's lock.
    AlreadyLocked { path: PathBuf },
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
            Self::Locked { path, holder } => {
                write!(f, "{} is locked", path.display())?;
                write_holder(f, holder.as_ref())
            }
            Self::LeaseHeld { path, holder } => {
                write!(f, "{} has a lease", path.display())?;
                write_holder(f, holder.as_ref())
            }
            Self::WrongScope { guard, key } => write!(
                f,
                "lock guard for {} does not cover {}",
                guard.display(),
                key.display()
            ),
            Self::AlreadyLocked { path } => {
                write!(f, "the current thread already holds {}", path.display())
            }
        }
    }
}

fn write_holder(f: &mut std::fmt::Formatter<'_>, holder: Option<&HolderInfo>) -> std::fmt::Result {
    if let Some(holder) = holder {
        write!(f, " by pid {} ({})", holder.pid, holder.label)?;
    }
    Ok(())
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

/// A read-modify-write failure: the store, or the caller's closure.
#[derive(Debug)]
pub enum UpdateError<E> {
    Store(StoreError),
    Apply(E),
}

impl<E: std::fmt::Display> std::fmt::Display for UpdateError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Apply(error) => write!(f, "{error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for UpdateError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Apply(error) => Some(error),
        }
    }
}

impl<E> From<StoreError> for UpdateError<E> {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
