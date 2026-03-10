use ratatui::style::{Color, Style};
use ratatui::text::Span;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// A cached highlighted span (owned, can be cloned across frames)
#[derive(Debug, Clone)]
struct CachedSpan {
    /// Rc<str> so cache hits are a pointer bump (~1ns) vs heap alloc (~50ns)
    text: Rc<str>,
    fg: Color,
    /// Insertion counter for LRU eviction
    access_gen: u64,
}

/// Cache key: content hash + filename hash (for language detection)
fn cache_key(line: &str, filename: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut hasher);
    filename.hash(&mut hasher);
    hasher.finish()
}

/// Cached syntax highlighting state — loaded once, reused for all files.
/// Includes a line-level cache to avoid re-highlighting identical content
/// across frames (high hit rate since most lines don't change between renders).
///
/// Uses two-face's extended syntax set (Sublime Text 4 definitions) for
/// broad language coverage including TypeScript, TSX, TOML, Svelte (via HTML), etc.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    /// Cache: content+filename hash → highlighted spans (fg colors only)
    cache: HashMap<u64, Vec<CachedSpan>>,
    /// Monotonic generation counter for LRU eviction tracking
    gen: u64,
}

/// Maximum cache entries before LRU eviction
const MAX_CACHE_SIZE: usize = 10_000;
/// Number of entries to evict when cache is full (oldest 25%)
const EVICT_COUNT: usize = MAX_CACHE_SIZE / 4;

impl Highlighter {
    pub fn new() -> Self {
        Highlighter {
            syntax_set: two_face::syntax::extra_newlines(),
            theme_set: ThemeSet::load_defaults(),
            cache: HashMap::new(),
            gen: 0,
        }
    }

    /// Highlight a single line of code, returning styled spans.
    /// `filename` is used to detect the language (e.g., "main.rs" → Rust).
    /// `base_style` is the background style to layer highlighting on top of
    /// (so add/delete background colors are preserved).
    ///
    /// Results are cached by content hash + filename for fast re-rendering.
    pub fn highlight_line<'a>(
        &mut self,
        line: &'a str,
        filename: &str,
        base_style: Style,
    ) -> Vec<Span<'a>> {
        let key = cache_key(line, filename);

        // Check cache — Rc::clone is a pointer bump, not a heap allocation
        if let Some(cached) = self.cache.get(&key) {
            return cached
                .iter()
                .map(|cs| Span::styled(Rc::clone(&cs.text).to_string(), base_style.fg(cs.fg)))
                .collect();
        }

        // Cache miss — perform highlighting
        // two-face's syntax set covers TS, TSX, TOML, Svelte, Vue, etc.
        // For .svelte/.vue files, force TypeScript syntax: these are embedded-language
        // files where per-line highlighting with the native syntax fails (HighlightLines
        // starts fresh each line, losing <script> block context). TypeScript handles
        // the script section correctly, which is the majority of reviewed code.
        let syntax = match filename.rsplit('.').next() {
            Some("svelte" | "vue") => self
                .syntax_set
                .find_syntax_by_name("TypeScript")
                .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text()),
            Some("astro") => self
                .syntax_set
                .find_syntax_by_name("HTML")
                .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text()),
            _ => self
                .syntax_set
                .find_syntax_for_file(filename)
                .ok()
                .flatten()
                .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text()),
        };

        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        let input = if line.ends_with('\n') {
            line.to_string()
        } else {
            format!("{}\n", line)
        };

        match highlighter.highlight_line(&input, &self.syntax_set) {
            Ok(ranges) => {
                self.gen += 1;
                let current_gen = self.gen;
                let cached_spans: Vec<CachedSpan> = ranges
                    .iter()
                    .map(|(syn_style, text)| {
                        let text = text.trim_end_matches('\n');
                        CachedSpan {
                            text: Rc::from(text),
                            fg: Color::Rgb(
                                syn_style.foreground.r,
                                syn_style.foreground.g,
                                syn_style.foreground.b,
                            ),
                            access_gen: current_gen,
                        }
                    })
                    .collect();

                let result = cached_spans
                    .iter()
                    .map(|cs| Span::styled(Rc::clone(&cs.text).to_string(), base_style.fg(cs.fg)))
                    .collect();

                // LRU eviction: remove oldest 25% of entries when cache is full
                if self.cache.len() >= MAX_CACHE_SIZE {
                    let mut entries: Vec<(u64, u64)> = self
                        .cache
                        .iter()
                        .map(|(k, v)| (*k, v.first().map_or(0, |s| s.access_gen)))
                        .collect();
                    entries.sort_unstable_by_key(|&(_, gen)| gen);
                    for (k, _) in entries.into_iter().take(EVICT_COUNT) {
                        self.cache.remove(&k);
                    }
                }
                self.cache.insert(key, cached_spans);

                result
            }
            Err(_) => {
                // Fallback: return unstyled
                vec![Span::styled(line.to_string(), base_style)]
            }
        }
    }

    /// Clear the highlight cache (e.g., on file switch)
    #[allow(dead_code)]
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}
