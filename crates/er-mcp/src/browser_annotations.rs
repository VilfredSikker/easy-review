//! Bounded image delivery for the shared browser annotation prompt.

use std::{collections::BTreeMap, path::Path};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use er_engine::ai::{
    browser_annotations::{
        load_browser_annotations, read_browser_asset, BrowserAnnotation, BrowserAnnotationsError,
        ImageEvidence,
    },
    browser_prompt::{render_browser_annotations, BrowserPromptOpts, ImageDelivery},
};
use rmcp::{
    model::{CallToolResult, ContentBlock},
    ErrorData as McpError,
};
use serde_json::json;

const MAX_RESPONSE_ASSET_BYTES: usize = 16 * 1024 * 1024;
const SIZE_LIMIT: &str = "response size limit (16 MiB)";
const IMAGES_DISABLED: &str = "include_images=false";
const MISSING_ASSET: &str = "captured image asset is missing";

// Storage errors can contain an absolute path or untrusted metadata values.
// Report the problem category without copying those values into protocol errors.
fn storage_error(error: BrowserAnnotationsError) -> McpError {
    let problem = match error {
        BrowserAnnotationsError::Io(error) => {
            return McpError::internal_error(
                format!("browser annotation I/O failed: {}", error.kind()),
                None,
            );
        }
        BrowserAnnotationsError::Malformed(_) => "malformed browser annotation metadata",
        BrowserAnnotationsError::UnsupportedVersion(_) => "unsupported browser annotations version",
        BrowserAnnotationsError::InvalidRecord { .. } => "invalid browser annotation record",
        BrowserAnnotationsError::InvalidAsset(_) => "invalid browser image asset",
        BrowserAnnotationsError::NumberExhausted => "browser annotation numbers exhausted",
        BrowserAnnotationsError::LockPoisoned => "browser annotation bucket lock poisoned",
    };
    McpError::internal_error(problem, None)
}

pub(crate) fn read_response(dir: &Path, include_images: bool) -> Result<CallToolResult, McpError> {
    read_response_with(dir, include_images, |asset_id| {
        read_browser_asset(dir, asset_id)
    })
}

fn read_response_with(
    dir: &Path,
    include_images: bool,
    mut read_asset: impl FnMut(&str) -> Result<Vec<u8>, BrowserAnnotationsError>,
) -> Result<CallToolResult, McpError> {
    let mut annotations = load_browser_annotations(dir).map_err(storage_error)?;
    let mut opts = BrowserPromptOpts::default();
    let mut images = BTreeMap::new();
    let mut deleted = Vec::new();
    let mut total_bytes = 0;
    for annotation in &annotations {
        let ImageEvidence::Captured { asset_id, .. } = &annotation.image else {
            continue;
        };
        let delivery = if !include_images {
            ImageDelivery::Omitted {
                reason: IMAGES_DISABLED.into(),
            }
        } else {
            match read_asset(asset_id) {
                Ok(bytes) if bytes.len() <= MAX_RESPONSE_ASSET_BYTES - total_bytes => {
                    total_bytes += bytes.len();
                    images.insert(asset_id.clone(), bytes);
                    ImageDelivery::Included
                }
                Ok(_) => ImageDelivery::Omitted {
                    reason: SIZE_LIMIT.into(),
                },
                Err(BrowserAnnotationsError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    // Delete commits metadata before unlinking the asset. A fresh
                    // read distinguishes deletion from a broken persisted capture.
                    let current = load_browser_annotations(dir).map_err(storage_error)?;
                    if !current.iter().any(|record| record.id == annotation.id) {
                        deleted.push(annotation.id.clone());
                        continue;
                    }
                    ImageDelivery::Unavailable {
                        reason: MISSING_ASSET.into(),
                    }
                }
                Err(error) => return Err(storage_error(error)),
            }
        };
        opts.image_delivery.insert(asset_id.clone(), delivery);
    }
    annotations.retain(|annotation| !deleted.contains(&annotation.id));
    build_response(&annotations, &opts, images)
}

fn build_response(
    annotations: &[BrowserAnnotation],
    opts: &BrowserPromptOpts,
    mut images: BTreeMap<String, Vec<u8>>,
) -> Result<CallToolResult, McpError> {
    let prompt = render_browser_annotations(annotations, opts);
    let mut content = vec![ContentBlock::text(prompt.text)];
    // The renderer references every capture, including ones omitted in this
    // response. Only the delivery map can authorize a caption/image pair.
    for image in prompt.images {
        match opts.image_delivery.get(&image.asset_id) {
            Some(ImageDelivery::Included) => {
                let bytes = images.remove(&image.asset_id).ok_or_else(|| {
                    McpError::internal_error("browser image delivery is incomplete", None)
                })?;
                content.push(ContentBlock::text(image.caption));
                content.push(ContentBlock::image(STANDARD.encode(bytes), "image/png"));
            }
            Some(ImageDelivery::Omitted { reason }) => content.push(ContentBlock::text(format!(
                "Image omitted for Comment {}: {reason}.",
                image.number
            ))),
            Some(ImageDelivery::Unavailable { reason }) => content.push(ContentBlock::text(
                format!("Image unavailable for Comment {}: {reason}.", image.number),
            )),
            _ => (),
        }
    }
    let records = serde_json::to_string_pretty(&json!({
        "version": er_engine::ai::browser_annotations::BROWSER_ANNOTATIONS_VERSION,
        "annotations": annotations,
    }))
    .map_err(|_| McpError::internal_error("could not serialize browser annotations", None))?;
    content.push(ContentBlock::text(records));
    Ok(CallToolResult::success(content))
}

#[cfg(test)]
mod tests;
