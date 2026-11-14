//! Folder-name helpers shared by the storage paths and the branch scope.

use sha2::{Digest, Sha256};

/// The slugified name is cut to this many bytes, on a character boundary.
const READABLE_MAX_BYTES: usize = 80;

/// Sanitize a string for use as a directory name component.
pub fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Sanitize a branch name for use as a directory component.
///
/// The readable part is the slugified name; the hash keeps branches that
/// slugify alike (`feat/x` and `feat-x`) in separate folders.
pub fn slug_branch(branch: &str) -> String {
    named_slug(branch, &readable_part(branch))
}

/// The slugified name, lowercased and cut to at most [`READABLE_MAX_BYTES`]
/// on a character boundary.
pub fn readable_part(raw: &str) -> String {
    let slug = slugify(raw).to_lowercase();
    if slug.len() <= READABLE_MAX_BYTES {
        return slug;
    }
    let mut end = READABLE_MAX_BYTES;
    while !slug.is_char_boundary(end) {
        end -= 1;
    }
    slug.get(..end).map(str::to_string).unwrap_or(slug)
}

/// The first 8 hex characters of SHA-256 over `raw`.
pub fn short_hash(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    format!(
        "{:08x}",
        u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
    )
}

/// `<readable>-<8 hex>`; an empty readable part still yields a valid name.
pub fn named_slug(raw: &str, readable: &str) -> String {
    format!("{readable}-{}", short_hash(raw))
}
