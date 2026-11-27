//! Reading Rust sources for the scans: comments dropped, and each
//! `#[cfg(test)]` item or module cut out, the scan going on after it (as
//! er-tui's `tests/writes.rs` does).

use std::path::{Path, PathBuf};

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every `.rs` file under `dir`, sorted.
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
                .path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// The code of `text`: comments dropped, test items and modules cut out.
pub fn code_of(text: &str) -> String {
    const ATTR: &str = "#[cfg(test)]";
    let text = without_comments(text);
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(ATTR) {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = at + ATTR.len();
        let tail = rest.get(after..).unwrap_or_default();
        let end = match (tail.find(';'), tail.find('{')) {
            (Some(semi), Some(open)) if semi < open => after + semi + 1,
            (Some(semi), None) => after + semi + 1,
            (_, Some(open)) => block_end(rest, after + open),
            (None, None) => rest.len(),
        };
        rest = rest.get(end..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// `text` with its comments dropped; string and char literals are kept.
fn without_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let next = bytes.get(at + 1).copied();
        let end = match byte {
            b'/' if next == Some(b'/') => {
                at = text
                    .get(at..)
                    .and_then(|rest| rest.find('\n'))
                    .map_or(bytes.len(), |n| at + n);
                continue;
            }
            b'/' if next == Some(b'*') => {
                at = text
                    .get(at..)
                    .and_then(|rest| rest.find("*/"))
                    .map_or(bytes.len(), |n| at + n + 2);
                continue;
            }
            b'"' | b'r' | b'\'' => literal_end(text, at),
            _ => at + 1,
        };
        out.extend_from_slice(bytes.get(at..end).unwrap_or_default());
        at = end;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Past the string, raw string or char literal at `at`; one byte on for
/// anything else.
fn literal_end(text: &str, at: usize) -> usize {
    let bytes = text.as_bytes();
    let find = |from: usize, what: &str| text.get(from..).and_then(|rest| rest.find(what));
    let next = bytes.get(at + 1).copied();
    match bytes.get(at) {
        Some(b'r') if matches!(next, Some(b'"' | b'#')) => {
            let hashes = bytes.get(at + 1..).unwrap_or_default();
            let hashes = hashes.iter().take_while(|&&c| c == b'#').count();
            if bytes.get(at + 1 + hashes) != Some(&b'"') {
                return at + 1;
            }
            let close = format!("\"{}", "#".repeat(hashes));
            let from = at + 2 + hashes;
            find(from, &close).map_or(text.len(), |n| from + n + close.len())
        }
        Some(b'"') => {
            let mut end = at + 1;
            while let Some(&c) = bytes.get(end) {
                match c {
                    b'\\' => end += 2,
                    b'"' => return end + 1,
                    _ => end += 1,
                }
            }
            text.len()
        }
        Some(b'\'') if bytes.get(at + 2) == Some(&b'\'') => at + 3,
        Some(b'\'') if next == Some(b'\\') => find(at + 3, "'").map_or(text.len(), |n| at + 4 + n),
        _ => at + 1,
    }
}

/// Past the `}` that closes the `{` at `open` in comment-free code.
pub fn block_end(text: &str, open: usize) -> usize {
    let bytes = text.as_bytes();
    let (mut at, mut depth) = (open, 0usize);
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'"' | b'r' | b'\'' => {
                let end = literal_end(text, at);
                if end > at + 1 {
                    at = end;
                    continue;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    text.len()
}

/// Each `pub fn` in `code`, with its body.
pub fn functions(code: &str) -> Vec<(String, String)> {
    code.match_indices("pub fn ")
        .map(|(at, _)| {
            let name: String = code
                .get(at + 7..)
                .unwrap_or_default()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let body = code
                .get(at..)
                .and_then(|rest| rest.find('{'))
                .map(|open| at + open)
                .map_or("", |open| {
                    code.get(open..block_end(code, open)).unwrap_or_default()
                });
            (name, body.to_string())
        })
        .collect()
}

/// How often `code` names `name` as a word of its own, other than as a
/// method (`.name`).
pub fn mentions(code: &str, name: &str) -> usize {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    code.match_indices(name)
        .filter(|(at, _)| {
            let before = code.get(..*at).and_then(|head| head.chars().next_back());
            let after = code
                .get(at + name.len()..)
                .and_then(|tail| tail.chars().next());
            !before.is_some_and(|c| word(c) || c == '.') && !after.is_some_and(word)
        })
        .count()
}
