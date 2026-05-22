use sha2::{Digest, Sha256};

/// Stable per-file content hash for the desktop highlight cache.
/// Advances whenever the diff changes (`tab_diff_hash` changes), which is
/// the correctness boundary needed for cache invalidation.
/// Always 32 lowercase hex chars (first 16 bytes of SHA-256).
pub fn file_cache_key(tab_diff_hash: &str, file_path: &str) -> String {
    let input = format!("{}:{}", tab_diff_hash, file_path);
    let hash = Sha256::digest(input.as_bytes());
    format!("{:x}", hash)[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_input_same_key() {
        let k1 = file_cache_key("abc123", "src/foo.ts");
        let k2 = file_cache_key("abc123", "src/foo.ts");
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_paths_different_keys() {
        let k1 = file_cache_key("abc123", "src/foo.ts");
        let k2 = file_cache_key("abc123", "src/bar.ts");
        assert_ne!(k1, k2);
    }

    #[test]
    fn different_tab_hashes_different_keys() {
        let k1 = file_cache_key("abc123", "src/foo.ts");
        let k2 = file_cache_key("def456", "src/foo.ts");
        assert_ne!(k1, k2);
    }

    #[test]
    fn key_is_32_hex_chars() {
        let k = file_cache_key("abc123", "src/foo.ts");
        assert_eq!(k.len(), 32);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
