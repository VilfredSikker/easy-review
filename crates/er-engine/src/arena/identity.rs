use sha1::{Digest, Sha1};

/// Normalize finding text for stable cross-run IDs.
pub fn canonical_finding_text(text: &str) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_lowercase()
}

/// Stable finding id: `sha1(file + nearest_function + canonical_text)` (hex).
pub fn finding_id(file: &str, nearest_function: &str, text: &str) -> String {
    let canonical = canonical_finding_text(text);
    let payload = format!("{file}\0{nearest_function}\0{canonical}");
    let digest = Sha1::digest(payload.as_bytes());
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_stable_for_same_inputs() {
        let a = finding_id("src/foo.rs", "bar", "  Hello   World ");
        let b = finding_id("src/foo.rs", "bar", "hello world");
        assert_eq!(a, b);
        assert_eq!(a.len(), 40);
    }

    #[test]
    fn id_changes_when_file_or_fn_changes() {
        let base = finding_id("a.ts", "fn", "issue");
        assert_ne!(base, finding_id("b.ts", "fn", "issue"));
        assert_ne!(base, finding_id("a.ts", "other", "issue"));
    }
}
