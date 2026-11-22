use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A document's text, and the keys of the view it was read in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FilePreview {
    pub path: String,
    pub text: String,
    pub preview_context_key: String,
    pub preview_key: String,
}

/// Both sides of an image change as `data:` URLs, so the page needs no file
/// access to show them; a side that does not exist is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ImagePreview {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub preview_context_key: String,
    pub preview_key: String,
}

/// Every path a tab's diff holds, whatever its filter keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiffPaths {
    pub paths: Vec<String>,
}
