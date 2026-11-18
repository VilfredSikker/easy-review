//! The "Mark lines" attribution cache, kept apart from its command.
//!
//! `DesktopState` holds this cache, and the command that fills it lives under
//! `commands`, which reaches the App. Keeping the type here lets `state.rs`
//! name it without depending on `commands`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use er_engine::git::LineAttribution;

/// Bounds memory across long sessions; one entry is one file of one range.
const CACHE_CAP: usize = 256;

pub type LineCommitsCache = Arc<Mutex<LineCommitsLru>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    pub(crate) root: String,
    pub(crate) start: Option<String>,
    pub(crate) end: String,
    pub(crate) working_tree: bool,
    pub(crate) path: String,
    pub(crate) old_path: Option<String>,
    /// The file's `FileSnapshot.cache_key`.
    pub(crate) hunks_key: String,
}

#[derive(Default)]
pub struct LineCommitsLru {
    pub(crate) entries: HashMap<CacheKey, LineAttribution>,
    order: VecDeque<CacheKey>,
}

impl LineCommitsLru {
    pub(crate) fn get(&self, key: &CacheKey) -> Option<LineAttribution> {
        self.entries.get(key).cloned()
    }

    pub(crate) fn insert(&mut self, key: CacheKey, value: LineAttribution) {
        if self.entries.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CACHE_CAP {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_evicts_oldest_past_the_cap() {
        let mut lru = LineCommitsLru::default();
        let key = |i: usize| CacheKey {
            root: String::new(),
            start: None,
            end: String::new(),
            working_tree: false,
            path: i.to_string(),
            old_path: None,
            hunks_key: String::new(),
        };
        for i in 0..=CACHE_CAP {
            lru.insert(key(i), LineAttribution::default());
        }
        assert_eq!(lru.entries.len(), CACHE_CAP);
        assert!(lru.get(&key(0)).is_none());
        assert!(lru.get(&key(CACHE_CAP)).is_some());
    }

    /// `DesktopState` and the modules that take it stay off `commands`, which
    /// reaches the App; this cache module stays off the App itself.
    #[test]
    fn state_and_its_modules_do_not_import_commands() {
        let production = |src: &'static str| src.split("#[cfg(test)]").next().unwrap_or(src);
        let scanned = [
            ("state.rs", production(include_str!("state.rs"))),
            (
                "github_image.rs",
                production(include_str!("github_image.rs")),
            ),
            (
                "line_commits_cache.rs",
                production(include_str!("line_commits_cache.rs")),
            ),
        ];
        for (name, source) in scanned {
            assert!(
                !source.contains("crate::commands"),
                "{name} imports crate::commands"
            );
        }
        let cache = scanned[2].1;
        for name in ["AppState", "er_engine::app"] {
            assert!(!cache.contains(name), "line_commits_cache.rs names {name}");
        }
    }
}
