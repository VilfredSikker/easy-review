//! `.er/ui-annotations.json` — annotations captured from the embedded browser
//! view.
//!
//! They stay plain files: no `ArtifactStore` kind, and #401's registry ignore
//! list covers them, as it does `finding-promotions.json`.

use serde::{Deserialize, Serialize};

/// A point/region annotation captured from the embedded browser view.
/// Stored at `<comments_dir>/ui-annotations.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiAnnotation {
    pub id: String,
    /// Path portion of the page URL (no query/hash), e.g. `/dashboard`.
    pub url: String,
    /// CSS selector (best-effort tag + nth-child path). `None` for cross-origin.
    #[serde(default)]
    pub selector: Option<String>,
    pub box_x: f64,
    pub box_y: f64,
    pub box_w: f64,
    pub box_h: f64,
    pub viewport_w: u32,
    pub viewport_h: u32,
    pub text: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default = "default_author")]
    pub author: String,
    #[serde(default)]
    pub screenshot_path: Option<String>,
    /// Persisted: set when re-anchoring fails or the new box deviates too much.
    #[serde(default)]
    pub stale: bool,
    /// Short description of the annotated element (tag + label/text), e.g. "button: Submit".
    #[serde(default)]
    pub element_context: Option<String>,
    /// Structured DOM context captured from the resolved element at annotation time.
    /// This is intended for agent communication, not as a stable re-anchor.
    #[serde(default)]
    pub dom_context: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErUiAnnotations {
    #[serde(default = "default_ui_version")]
    pub version: u32,
    #[serde(default)]
    pub annotations: Vec<UiAnnotation>,
}

const fn default_ui_version() -> u32 {
    1
}

fn default_author() -> String {
    "You".to_string()
}

/// Load annotations from `<comments_dir>/ui-annotations.json`. Missing file → empty.
pub fn load_ui_annotations(comments_dir: &str) -> Vec<UiAnnotation> {
    let path = format!("{comments_dir}/ui-annotations.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<ErUiAnnotations>(&content)
        .map(|f| f.annotations)
        .unwrap_or_default()
}

/// Atomically write annotations to `<comments_dir>/ui-annotations.json`.
pub fn save_ui_annotations(
    comments_dir: &str,
    annotations: &[UiAnnotation],
) -> std::io::Result<()> {
    let path = format!("{comments_dir}/ui-annotations.json");
    let file = ErUiAnnotations {
        version: 1,
        annotations: annotations.to_vec(),
    };
    let json = serde_json::to_string_pretty(&file).map_err(std::io::Error::other)?;
    er_store::atomic::write_atomic(std::path::Path::new(&path), json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_annotation_roundtrips_via_serde() {
        let ann = UiAnnotation {
            id: "ann-1".into(),
            url: "/dashboard".into(),
            selector: Some("button.primary:nth-child(2)".into()),
            box_x: 12.5,
            box_y: 30.0,
            box_w: 100.0,
            box_h: 28.0,
            viewport_w: 1280,
            viewport_h: 800,
            text: "This button looks off".into(),
            timestamp: "2026-05-13T10:00:00Z".into(),
            author: "You".into(),
            screenshot_path: None,
            stale: false,
            element_context: Some("button: Save".into()),
            dom_context: Some(serde_json::json!({
                "tag": "button",
                "text": "Save",
                "role": "button"
            })),
        };
        let json = serde_json::to_string(&ann).unwrap();
        let back: UiAnnotation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ann);
    }

    #[test]
    fn ui_annotation_file_load_missing_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_string_lossy().to_string();
        let v = load_ui_annotations(&dir);
        assert!(v.is_empty());
    }

    #[test]
    fn ui_annotation_save_then_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_string_lossy().to_string();
        let anns = vec![UiAnnotation {
            id: "a".into(),
            url: "/x".into(),
            selector: None,
            box_x: 0.0,
            box_y: 0.0,
            box_w: 10.0,
            box_h: 10.0,
            viewport_w: 800,
            viewport_h: 600,
            text: "hi".into(),
            timestamp: "t".into(),
            author: "You".into(),
            screenshot_path: None,
            stale: false,
            element_context: None,
            dom_context: None,
        }];
        save_ui_annotations(&dir, &anns).unwrap();
        let back = load_ui_annotations(&dir);
        assert_eq!(back, anns);
    }
}
