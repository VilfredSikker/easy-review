use crate::snapshot::SpanSnapshot;
use er_engine::highlight::Highlighter;
use std::collections::HashMap;

/// Per-line highlight result for one hunk.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HunkHighlight {
    pub hunk_index: usize,
    pub lines: Vec<Vec<SpanSnapshot>>,
}

/// Result returned by `highlight_file` to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HighlightResult {
    /// Echoed from the request — frontend discards if mismatched (stale diff).
    pub cache_key: String,
    pub syntax_theme: String,
    pub hunks: Vec<HunkHighlight>,
}

/// Simple LRU cache entry.
struct CacheEntry {
    generation: u64,
    hunks: Vec<HunkHighlight>,
}

/// Separate highlight state decoupled from the poll path.
/// Holds its own `Highlighter` so `highlight_file` never blocks `poll`.
pub struct HighlightState {
    pub highlighter: std::sync::Mutex<Highlighter>,
    cache: std::sync::Mutex<HighlightCache>,
}

struct HighlightCache {
    entries: HashMap<(String, String, String), CacheEntry>,
    generation: u64,
    capacity: usize,
}

impl HighlightCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            generation: 0,
            capacity,
        }
    }

    fn get(&mut self, key: &(String, String, String)) -> Option<Vec<HunkHighlight>> {
        if let Some(e) = self.entries.get_mut(key) {
            self.generation += 1;
            e.generation = self.generation;
            Some(e.hunks.clone())
        } else {
            None
        }
    }

    fn insert(&mut self, key: (String, String, String), hunks: Vec<HunkHighlight>) {
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            // Evict least-recently-used entry
            if let Some(lru_key) = self
                .entries
                .iter()
                .min_by_key(|(_, v)| v.generation)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&lru_key);
            }
        }
        self.generation += 1;
        self.entries.insert(
            key,
            CacheEntry {
                generation: self.generation,
                hunks,
            },
        );
    }
}

impl HighlightState {
    pub fn new() -> Self {
        Self {
            highlighter: std::sync::Mutex::new(Highlighter::new()),
            cache: std::sync::Mutex::new(HighlightCache::new(200)),
        }
    }

    pub fn get_cached(
        &self,
        file_path: &str,
        cache_key: &str,
        syntax_theme: &str,
    ) -> Option<Vec<HunkHighlight>> {
        let key = (
            file_path.to_string(),
            cache_key.to_string(),
            syntax_theme.to_string(),
        );
        self.cache.lock().ok()?.get(&key)
    }

    pub fn insert_cache(
        &self,
        file_path: &str,
        cache_key: &str,
        syntax_theme: &str,
        hunks: Vec<HunkHighlight>,
    ) {
        if let Ok(mut c) = self.cache.lock() {
            c.insert(
                (
                    file_path.to_string(),
                    cache_key.to_string(),
                    syntax_theme.to_string(),
                ),
                hunks,
            );
        }
    }
}
