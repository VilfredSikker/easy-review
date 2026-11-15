//! The bucket file that records which findings were promoted to comments.
//!
//! `finding-promotions.json` is a desktop marker, not a registered artifact:
//! it says which findings the Promoted badge belongs to. Both the desktop and
//! the service's promote effect read and write it, so the helpers live here.

/// The bucket file that records which findings were promoted to comments.
pub fn finding_promotions_path(er_dir: &str) -> String {
    format!("{er_dir}/finding-promotions.json")
}

/// The promoted-finding markers the desktop renders. A missing or malformed
/// file reads as empty.
pub fn load_finding_promotions(er_dir: &str) -> std::collections::HashMap<String, String> {
    let path = finding_promotions_path(er_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the promoted-finding markers.
pub fn save_finding_promotions(
    er_dir: &str,
    map: &std::collections::HashMap<String, String>,
) -> std::io::Result<()> {
    let path = finding_promotions_path(er_dir);
    let json = serde_json::to_string_pretty(map).unwrap_or_else(|_| "{}".to_string());
    er_store::atomic::write_atomic(std::path::Path::new(&path), json.as_bytes())
}
