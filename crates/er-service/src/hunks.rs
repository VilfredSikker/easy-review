//! `hunks(tab, path, hash)`: one file's hunks, named by the hash of its own
//! lines (ADR 0013). Threads are not part of them, so a comment moves the
//! tab's annotations and never a file's hash.

use er_engine::file_rows::{file_lines_key, stub_key};
use er_engine::git::{DiffFile, LineType};
use er_protocol as wire;

use crate::content::TabContent;

/// What a read of one file's hunks finds in a tab's content.
#[derive(Debug, PartialEq, Eq)]
pub enum HunkRead {
    Hunks(wire::FileHunks),
    /// The file is a lazy stub still unparsed; a parse through the actor
    /// fills it.
    Unparsed,
    /// The content no longer holds that hash for the path.
    Stale,
}

/// Whether `file` is listed but not parsed yet. A file with no +/- lines (a
/// pure rename, a mode change) never parses into hunks, so it is never a stub.
pub fn is_stub(content: &TabContent, file: &DiffFile) -> bool {
    content.diff.meta.lazy && file.hunks.is_empty() && !file.compacted && file.adds + file.dels > 0
}

/// The hash `diff(tab)` names for `file`. An unparsed stub has no lines to
/// hash, so it is named by its section of the raw diff, which a rewrite moves.
pub fn row_hash(content: &TabContent, file: &DiffFile) -> String {
    if is_stub(content, file) {
        stub_hash(content, &file.path)
    } else {
        format!("{:016x}", file_lines_key(file))
    }
}

fn stub_hash(content: &TabContent, path: &str) -> String {
    format!("{:016x}", stub_key(path, section(content, path)))
}

fn section<'a>(content: &'a TabContent, path: &str) -> Option<&'a str> {
    let meta = &content.diff.meta;
    let header = meta.headers.iter().find(|header| header.path == path)?;
    meta.raw
        .get(header.byte_offset..header.byte_offset + header.byte_length)
}

/// `path`'s hunks in `content` when `hash` names them. A parsed file
/// answers its own hash, and the hash its stub had in the same raw diff, since
/// a page can ask by the row it read before the parse landed.
pub fn read_hunks(content: &TabContent, path: &str, hash: &str) -> HunkRead {
    let Some(file) = content.diff.files.iter().find(|file| file.path == path) else {
        return HunkRead::Stale;
    };
    let named = row_hash(content, file);
    if is_stub(content, file) {
        return if named == hash {
            HunkRead::Unparsed
        } else {
            HunkRead::Stale
        };
    }
    if named != hash && !(content.diff.meta.lazy && stub_hash(content, path) == hash) {
        return HunkRead::Stale;
    }
    HunkRead::Hunks(wire::FileHunks {
        hash: named,
        hunks: file_hunks(file),
    })
}

/// `file`'s hunks as the wire carries them.
pub fn file_hunks(file: &DiffFile) -> Vec<wire::Hunk> {
    file.hunks
        .iter()
        .map(|hunk| {
            let lines: Vec<wire::HunkLine> = hunk
                .lines
                .iter()
                .map(|line| {
                    let (kind, text) = match line.line_type {
                        LineType::Add => ("add", line.content.clone()),
                        LineType::Delete => ("del", line.content.clone()),
                        LineType::Context => ("context", line.content.clone()),
                        LineType::Fold(hidden) => {
                            ("fold", format!("··· {hidden} unchanged lines ···"))
                        }
                    };
                    wire::HunkLine {
                        old_num: line.old_num.map(number),
                        new_num: line.new_num.map(number),
                        kind: kind.to_string(),
                        text,
                    }
                })
                .collect();
            let count = |side: fn(&LineType) -> bool| {
                number(hunk.lines.iter().filter(|l| side(&l.line_type)).count())
            };
            wire::Hunk {
                header: hunk.header.clone(),
                old_start: number(hunk.old_start),
                old_count: count(|t| matches!(t, LineType::Context | LineType::Delete)),
                new_start: number(hunk.new_start),
                new_count: count(|t| matches!(t, LineType::Context | LineType::Add)),
                lines,
            }
        })
        .collect()
}

fn number(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
