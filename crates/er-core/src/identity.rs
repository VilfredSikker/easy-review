//! How a finding is identified across runs.
//!
//! Lives in `ai` rather than `arena` because it keys a `Finding`, and `ai` is an
//! always-on module while `arena` is gated behind the `ui` feature — a headless
//! consumer still reads `arbiter.json`, so it still needs to compute these.

use sha1::{Digest, Sha1};

/// Normalize finding text for stable cross-run IDs.
pub fn canonical_finding_text(text: &str) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_lowercase()
}

/// Stable key for a finding: `sha1(file + line + canonical_text)`.
///
/// The anchor line is part of the key so two *different* issues that happen to
/// share a title in one file stay distinct rows instead of collapsing into one.
/// Title alone could not do that, and the key has to be unique per row.
///
/// The cost is deliberate: an edit above a finding changes the line and so its
/// key, orphaning whatever verdict it had. A grade is never carried over to a
/// claim the arbiter did not read, which is the safer way round — the finding
/// simply reads as ungraded until the next arbiter pass.
///
/// Separate from `arena::finding_id`, which keys the arena's own debate findings
/// and carries a nearest-function instead of a line.
pub fn finding_key(file: &str, line: Option<usize>, text: &str) -> String {
    let canonical = canonical_finding_text(text);
    let anchor = line.map(|l| l.to_string()).unwrap_or_default();
    let payload = format!("{file}\0{anchor}\0{canonical}");
    let digest = Sha1::digest(payload.as_bytes());
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two issues sharing a title in one file are different claims, and the key
    /// has to say so.
    #[test]
    fn finding_key_separates_same_title_different_line() {
        let at_ten = finding_key("src/a.rs", Some(10), "missing null check");
        let at_four_hundred = finding_key("src/a.rs", Some(400), "missing null check");
        assert_ne!(at_ten, at_four_hundred);
        assert_ne!(
            at_ten,
            finding_key("src/b.rs", Some(10), "missing null check")
        );
    }

    #[test]
    fn finding_key_is_stable_and_normalises_text() {
        let a = finding_key("src/a.rs", Some(10), "  Missing   Null Check ");
        let b = finding_key("src/a.rs", Some(10), "missing null check");
        assert_eq!(a, b);
        assert_eq!(a.len(), 40);
    }

    /// A hunk-level finding has no line; it still needs a key, and it must not
    /// collide with a line-anchored one.
    #[test]
    fn finding_key_without_a_line_differs_from_line_one() {
        assert_ne!(
            finding_key("src/a.rs", None, "issue"),
            finding_key("src/a.rs", Some(1), "issue")
        );
    }
}
