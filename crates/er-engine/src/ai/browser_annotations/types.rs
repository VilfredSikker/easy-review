use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Additional optional v2 fields survive read/modify/write, including nested fields.
pub type Extensions = BTreeMap<String, Value>;

macro_rules! record {
    ($name:ident { $($(#[$attr:meta])* $field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
        pub struct $name {
            $($(#[$attr])* pub $field: $ty,)*
            #[serde(flatten)]
            pub extra: Extensions,
        }
    };
}

record!(BrowserAnnotationsFile {
    version: u32,
    next_number: u32,
    annotations: Vec<BrowserAnnotation>,
});
record!(BrowserAnnotation {
    id: String,
    number: u32,
    idempotency_key: String,
    created_at: String,
    author: String,
    text: String,
    page: PageIdentity,
    locator: Option<Locator>,
    target: TargetDescription,
    captured: CapturedGeometry,
    dom_context: Option<Value>,
    resolution: Resolution,
    image: ImageEvidence,
});

// Evidence supplied by the host. No URL normalization, redaction, or matching
// policy is applied; production callers must resolve that policy.
record!(PageIdentity {
    origin: String,
    path: String,
    query: Option<String>,
    hash_route: Option<String>,
});
record!(Locator {
    frames: Vec<FrameLocator>,
    shadow_hosts: Vec<String>,
    selector: String,
    fingerprint: Fingerprint,
});
record!(FrameLocator {
    selector: String,
    shadow_hosts: Vec<String>,
    fingerprint: Fingerprint,
});
record!(Fingerprint {
    role: Option<String>,
    name: Option<String>,
    text_excerpt: Option<String>,
});
record!(TargetDescription {
    kind: TargetKind,
    role: Option<String>,
    name: Option<String>,
    text_excerpt: Option<String>,
    limitation: Option<String>,
});
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Element,
    Region,
}
record!(Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64
});
record!(Size {
    width: f64,
    height: f64
});
record!(Point { x: f64, y: f64 });
record!(CapturedGeometry {
    rect: Rect,
    viewport: Size,
    scroll: Point,
    device_scale: f64,
    zoom: f64,
});
record!(Resolution {
    status: ResolutionStatus,
    /// Current geometry is independent of immutable captured evidence.
    current_rect: Option<Rect>,
});
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Resolved,
    Missing,
    Ambiguous,
}
record!(ImageCapture {
    crop: Rect,
    marker: Rect,
    scale: f64,
    captured_at: String,
});
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ImageEvidence {
    None {
        #[serde(flatten)]
        extra: Extensions,
    },
    Captured {
        asset_id: String,
        capture: ImageCapture,
        #[serde(flatten)]
        extra: Extensions,
    },
    Unavailable {
        reason: String,
        #[serde(flatten)]
        extra: Extensions,
    },
}

/// The host creates IDs and assigns a number only when metadata commits.
/// Capture geometry does not reserve a marker number or select a compositing strategy.
#[derive(Clone, Debug)]
pub struct NewBrowserAnnotation {
    pub idempotency_key: String,
    pub created_at: String,
    pub author: String,
    pub text: String,
    pub page: PageIdentity,
    pub locator: Option<Locator>,
    pub target: TargetDescription,
    pub captured: CapturedGeometry,
    pub dom_context: Option<Value>,
    pub resolution: Resolution,
    pub image: NewImageEvidence,
}
#[derive(Clone, Debug)]
pub enum NewImageEvidence {
    None,
    Captured(ImageCapture),
    Unavailable { reason: String },
}
