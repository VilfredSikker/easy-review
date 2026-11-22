//! `hunks(tab, path, hash)` on the flag path: one file's hunks, read from
//! the content its tab's workspace published.
//!
//! A file the section has not parsed is parsed by its actor through
//! `ParseFiles`, the one parse path, within the actor's reply bound. A hash
//! the section no longer holds for the path answers `Stale`, and the page
//! fetches `diff(tab)` again; new hunks never answer an old hash.

use std::collections::BTreeMap;
use std::sync::Arc;

use er_protocol::{ErError, ErErrorKind, FileHunks, Resource, ResourceKey};
use er_service::{read_hunks, Command, HunkRead, TabContent, TabEntry};

use crate::service::actor_key;
use crate::state::DesktopState;

/// The hunks `hash` names for `path` in `wire`'s diff. Version 0: what a
/// hash names never changes, so nothing is pushed for it.
pub fn hunks(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
    path: &str,
    hash: &str,
) -> Result<(u64, Resource), ErError> {
    let entry = super::diff::listed(desktop, wire)?;
    let read = || Ok(read_hunks(&*content(desktop, &entry)?, path, hash));
    let parse_stub = || parse(desktop, &entry, vec![path.to_string()]);
    answer(read, parse_stub, path, hash).map(|hunks| (0, Resource::Hunks(hunks)))
}

/// Reads `path`'s hunks, parsing the stub first when it is unparsed. A parse
/// refused because another landed first is sent once more. A stub still
/// unparsed after a refused parse answers `Stale`, so the page asks again;
/// only a stub that an accepted parse left empty answers no hunks, which
/// keeps the page from asking again.
pub(crate) fn answer(
    read: impl Fn() -> Result<HunkRead, ErError>,
    mut parse: impl FnMut() -> Result<bool, ErError>,
    path: &str,
    hash: &str,
) -> Result<FileHunks, ErError> {
    let mut refused = false;
    for _ in 0..2 {
        match read()? {
            HunkRead::Hunks(hunks) => return Ok(hunks),
            HunkRead::Stale => return Err(stale(path)),
            HunkRead::Unparsed => {
                refused = !parse()?;
                if !refused {
                    break;
                }
            }
        }
    }
    match read()? {
        HunkRead::Hunks(hunks) => Ok(hunks),
        HunkRead::Unparsed if !refused => Ok(FileHunks {
            hash: hash.to_string(),
            hunks: Vec::new(),
        }),
        HunkRead::Unparsed | HunkRead::Stale => Err(stale(path)),
    }
}

/// Parses every unparsed file `keys` ask for with one `ParseFiles` per tab,
/// before the keys are read one by one. A key this cannot parse is left to
/// its own read, which answers why.
pub fn parse_asked(desktop: &DesktopState, keys: &[ResourceKey]) {
    let mut asked: BTreeMap<er_protocol::TabKey, Vec<(String, String)>> = BTreeMap::new();
    for key in keys {
        if let ResourceKey::Hunks { tab, path, hash } = key {
            asked
                .entry(*tab)
                .or_default()
                .push((path.clone(), hash.clone()));
        }
    }
    for (wire, files) in asked {
        let Ok(entry) = super::diff::listed(desktop, wire) else {
            continue;
        };
        let Ok(content) = content(desktop, &entry) else {
            continue;
        };
        let paths: Vec<String> = files
            .into_iter()
            .filter(|(path, hash)| read_hunks(&content, path, hash) == HunkRead::Unparsed)
            .map(|(path, _)| path)
            .collect();
        if !paths.is_empty() {
            let _ = parse(desktop, &entry, paths);
        }
    }
}

/// Parses `paths` on the tab's actor and waits within its reply bound.
/// Answers false when the actor refused the parse because the diff moved
/// under it, which a second parse can fix.
fn parse(desktop: &DesktopState, entry: &TabEntry, paths: Vec<String>) -> Result<bool, ErError> {
    let key = entry.key;
    let command = Command::ParseFiles {
        key,
        paths,
        expand: false,
    };
    match desktop.service.send(&actor_key(&entry.descriptor), command) {
        Ok(_) => Ok(true),
        Err(er_service::WorkspaceError::Failed(_)) => Ok(false),
        Err(error) => Err(ErError::from(error)),
    }
}

/// The tab's published content. A listed tab nothing loaded is asked to load
/// without waiting, and the read answers `Stale`: the page holds no hash
/// that content could name yet.
fn content(desktop: &DesktopState, entry: &TabEntry) -> Result<Arc<TabContent>, ErError> {
    match desktop.service.content(entry.key, &entry.descriptor) {
        Some(content) => Ok(content),
        None => {
            desktop.service.request_load(entry.key, &entry.descriptor);
            Err(ErError {
                kind: ErErrorKind::Stale,
                message: "the tab's diff is not loaded yet".into(),
            })
        }
    }
}

fn stale(path: &str) -> ErError {
    ErError {
        kind: ErErrorKind::Stale,
        message: format!("{path} changed since the diff was read"),
    }
}
