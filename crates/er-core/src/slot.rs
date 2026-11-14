//! The key part that selects one file when a kind has several.

/// The key part that selects one file when a kind has several.
pub trait SlotName: Clone + std::fmt::Debug + PartialEq + Eq + PartialOrd + Ord {
    /// Substituted for `{slot}` in [`crate::artifact::Artifact::FILE`].
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

/// Why a string cannot name a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdError {
    what: &'static str,
    value: String,
}

impl std::fmt::Display for IdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid {} `{}`", self.what, self.value)
    }
}

impl std::error::Error for IdError {}

/// An expert id. Expert files are stored as `experts/<id>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExpertId(String);

impl ExpertId {
    pub fn new(value: &str) -> Result<Self, IdError> {
        if is_safe_id(value) {
            Ok(Self(value.to_string()))
        } else {
            Err(IdError {
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
    pub fn new(value: &str) -> Result<Self, IdError> {
        if is_safe_id(value) {
            Ok(Self(value.to_string()))
        } else {
            Err(IdError {
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
