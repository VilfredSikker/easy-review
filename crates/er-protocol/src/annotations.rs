use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The browser annotations of a tab's view bucket.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct UiAnnotations {
    pub rows: Vec<UiAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct UiAnnotation {
    pub id: String,
    pub url: String,
    pub selector: Option<String>,
    pub box_x: f64,
    pub box_y: f64,
    pub box_w: f64,
    pub box_h: f64,
    pub viewport_w: u32,
    pub viewport_h: u32,
    pub text: String,
    pub timestamp: String,
    pub author: String,
    pub screenshot_path: Option<String>,
    pub stale: bool,
    pub element_context: Option<String>,
    /// The DOM context as JSON text, which the page parses.
    pub dom_context_json: Option<String>,
}

/// The tab's review projected as the reviewer sees it, as pretty JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewJson {
    pub json: String,
}

/// The review revisions a tab's bucket holds: at most the current one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewRevisions {
    pub revisions: Vec<ReviewRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewRevision {
    pub revision_id: String,
    pub created_at: String,
    pub scope: String,
    pub diff_hash: String,
    pub active: bool,
    pub agents: Vec<String>,
}

/// A saved annotation screenshot as a `data:` URL.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AnnotationScreenshot {
    pub data_url: String,
}
