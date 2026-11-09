use std::path::{Component, Path, PathBuf};

use crate::artifact::Artifact;
use crate::error::StoreError;

/// A validated path component: a repo, branch or view-bucket name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(String);

impl Slug {
    pub fn new(value: &str) -> Result<Self, StoreError> {
        validate_component("slug", value)?;
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_component(what: &'static str, value: &str) -> Result<(), StoreError> {
    let invalid = value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
        || value.len() > 255;
    if invalid {
        return Err(StoreError::InvalidComponent {
            what,
            value: value.to_string(),
        });
    }
    Ok(())
}

/// A relative artifact path under a scope directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelPath(PathBuf);

impl RelPath {
    pub fn new(value: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = value.as_ref();
        let raw = path.to_string_lossy().into_owned();
        if raw.is_empty() || raw.contains('\\') || raw.contains('\0') || path.is_absolute() {
            return Err(StoreError::InvalidPath { value: raw });
        }
        let mut has_normal = false;
        for component in path.components() {
            match component {
                Component::Normal(_) => has_normal = true,
                _ => return Err(StoreError::InvalidPath { value: raw }),
            }
        }
        if !has_normal {
            return Err(StoreError::InvalidPath { value: raw });
        }
        Ok(Self(path.to_path_buf()))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// A bucket directory a caller already holds. It may be relative or absolute;
/// the store root is ignored for a key in this scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BucketDir(PathBuf);

impl BucketDir {
    pub fn new(value: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = value.into();
        let raw = path.to_string_lossy().into_owned();
        if raw.is_empty() || raw.contains('\0') {
            return Err(StoreError::InvalidPath { value: raw });
        }
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(StoreError::InvalidPath { value: raw });
        }
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// The four view buckets under a branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchView {
    Branch,
    Unstaged,
    Staged,
    History,
}

impl BranchView {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Unstaged => "unstaged",
            Self::Staged => "staged",
            Self::History => "history",
        }
    }
}

/// Where an artifact lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// `repos/<repo>/branches/<branch>/view-buckets/<view>`.
    Branch {
        repo: Slug,
        branch: Slug,
        view: BranchView,
    },
    /// `repos/<repo>/prs/pr-<number>`.
    Pr { repo: Slug, number: u64 },
    /// `app`.
    App,
    /// A bucket directory the caller already holds.
    Dir(BucketDir),
}

impl Scope {
    pub fn dir(dir: impl AsRef<Path>) -> Result<Self, StoreError> {
        Ok(Self::Dir(BucketDir::new(dir.as_ref().to_path_buf())?))
    }
}

/// The key part that selects one file when a kind has several.
pub trait SlotName: Clone + std::fmt::Debug + PartialEq + Eq + PartialOrd + Ord {
    /// Substituted for `{slot}` in [`Artifact::FILE`].
    fn as_str(&self) -> &str;

    /// Parse the stem back from a file name. `None` for a foreign name.
    fn parse(stem: &str) -> Option<Self>;

    /// `Some(())` only for single-file kinds.
    fn singleton() -> Option<Self> {
        None
    }
}

impl SlotName for () {
    fn as_str(&self) -> &'static str {
        ""
    }

    fn parse(stem: &str) -> Option<Self> {
        stem.is_empty().then_some(())
    }

    fn singleton() -> Option<Self> {
        Some(())
    }
}

fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// An expert id. Expert files are stored as `experts/<id>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExpertId(String);

impl ExpertId {
    pub fn new(value: &str) -> Result<Self, StoreError> {
        if is_safe_id(value) {
            Ok(Self(value.to_string()))
        } else {
            Err(StoreError::InvalidComponent {
                what: "expert id",
                value: value.to_string(),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl SlotName for ExpertId {
    fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(stem: &str) -> Option<Self> {
        Self::new(stem).ok()
    }
}

/// A diagram id. Diagram files are stored as `diagrams/<id>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DiagramId(String);

impl DiagramId {
    pub fn new(value: &str) -> Result<Self, StoreError> {
        if is_safe_id(value) {
            Ok(Self(value.to_string()))
        } else {
            Err(StoreError::InvalidComponent {
                what: "diagram id",
                value: value.to_string(),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl SlotName for DiagramId {
    fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(stem: &str) -> Option<Self> {
        Self::new(stem).ok()
    }
}

/// Validate a kind's `FILE` against its slot type and return the path with
/// `{slot}` replaced. A singleton kind must not carry `{slot}`; a multi-file
/// kind must carry it exactly once. The result must be a relative path with no
/// `..` component.
pub(crate) fn validated_template<T: Artifact>(replacement: &str) -> Result<RelPath, StoreError> {
    let template = T::FILE;
    let placeholders = template.matches("{slot}").count();
    if T::Slot::singleton().is_some() {
        if placeholders != 0 {
            return Err(StoreError::InvalidPath {
                value: template.to_string(),
            });
        }
        RelPath::new(template)
    } else {
        if placeholders != 1 {
            return Err(StoreError::InvalidPath {
                value: template.to_string(),
            });
        }
        RelPath::new(template.replace("{slot}", replacement))
    }
}

/// A scope, a slot and the resolved file path.
pub struct ArtifactKey<T: Artifact> {
    scope: Scope,
    slot: T::Slot,
    rel: RelPath,
}

impl<T: Artifact> ArtifactKey<T> {
    pub fn new(scope: Scope, slot: T::Slot) -> Result<Self, StoreError> {
        let rel = if T::Slot::singleton().is_some() {
            validated_template::<T>("")?
        } else {
            validate_component("slot", slot.as_str())?;
            validated_template::<T>(slot.as_str())?
        };
        Ok(Self { scope, slot, rel })
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn slot(&self) -> &T::Slot {
        &self.slot
    }

    pub(crate) fn relative_path(&self) -> &RelPath {
        &self.rel
    }
}

impl<T: Artifact> Clone for ArtifactKey<T> {
    fn clone(&self) -> Self {
        Self {
            scope: self.scope.clone(),
            slot: self.slot.clone(),
            rel: self.rel.clone(),
        }
    }
}

impl<T: Artifact> std::fmt::Debug for ArtifactKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtifactKey")
            .field("scope", &self.scope)
            .field("slot", &self.slot)
            .field("rel", &self.rel)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        BadSlot, TestBadMulti, TestBadSingleton, TestBadSlotKind, TestEnveloped, TestExpert,
    };

    fn scope() -> Scope {
        Scope::App
    }

    #[test]
    fn slug_rejects_empty_dot_dotdot_and_separators() {
        assert!(Slug::new("").is_err());
        assert!(Slug::new(".").is_err());
        assert!(Slug::new("..").is_err());
        assert!(Slug::new("a/b").is_err());
        assert!(Slug::new("a\\b").is_err());
        assert!(Slug::new("a\0b").is_err());
        assert!(Slug::new(&"x".repeat(256)).is_err());
        assert!(Slug::new("feature-x").is_ok());
    }

    #[test]
    fn diagram_id_rejects_path_escape_and_accepts_a_custom_id() {
        assert!(DiagramId::new("../x").is_err());
        assert!(DiagramId::new("a/b").is_err());
        assert!(DiagramId::new("").is_err());
        assert!(DiagramId::new(&"x".repeat(121)).is_err());
        assert!(DiagramId::new("custom-1730000000000").is_ok());
        assert!(DiagramId::new("mental-model").is_ok());
    }

    #[test]
    fn dir_scope_accepts_any_directory_and_rejects_empty_and_dotdot() {
        assert!(Scope::dir("/tmp/x").is_ok());
        assert!(Scope::dir("").is_err());
        assert!(Scope::dir("a/../b").is_err());
    }

    #[test]
    fn artifact_key_rejects_a_singleton_template_with_a_slot() {
        assert!(ArtifactKey::<TestBadSingleton>::new(scope(), ()).is_err());
    }

    #[test]
    fn artifact_key_rejects_a_multi_template_without_a_slot() {
        let id = ExpertId::new("security").unwrap();
        assert!(ArtifactKey::<TestBadMulti>::new(scope(), id).is_err());
    }

    #[test]
    fn artifact_key_rejects_a_slot_with_a_separator() {
        let slot = BadSlot("a/b".to_string());
        assert!(ArtifactKey::<TestBadSlotKind>::new(scope(), slot).is_err());
    }

    #[test]
    fn artifact_key_accepts_a_valid_multi_template() {
        let id = ExpertId::new("security").unwrap();
        let key = ArtifactKey::<TestExpert>::new(scope(), id).unwrap();
        assert_eq!(
            key.relative_path().as_path(),
            Path::new("experts/security.json")
        );
    }

    #[test]
    fn rel_path_rejects_empty_absolute_dotdot_and_backslash() {
        assert!(RelPath::new("").is_err());
        assert!(RelPath::new("/x").is_err());
        assert!(RelPath::new("a/../b").is_err());
        assert!(RelPath::new("a\\b").is_err());
        assert!(RelPath::new("a\0b").is_err());
        assert!(RelPath::new("a/b.json").is_ok());
    }

    #[test]
    fn a_single_file_kind_builds_its_path() {
        let key = ArtifactKey::<TestEnveloped>::new(scope(), ()).unwrap();
        assert_eq!(
            key.relative_path().as_path(),
            Path::new("test-enveloped.json")
        );
    }
}
