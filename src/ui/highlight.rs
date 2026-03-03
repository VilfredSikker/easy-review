use ratatui::style::{Color, Style};
use ratatui::text::Span;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// A cached highlighted span (owned, can be cloned across frames)
#[derive(Debug, Clone)]
struct CachedSpan {
    text: String,
    fg: Color,
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
    /// Cache: file extension → syntax index in syntax_set.syntaxes()
    /// Avoids repeated find_syntax_for_file() calls (which do file IO for shebang detection)
    syntax_cache: HashMap<String, Option<usize>>,
    /// Last filename used for syntax lookup — skip re-lookup when highlighting
    /// consecutive lines from the same file
    last_file: String,
    last_syntax_idx: Option<usize>,
}

/// Maximum cache entries before eviction
const MAX_CACHE_SIZE: usize = 10_000;

impl Highlighter {
    pub fn new() -> Self {
        Highlighter {
            syntax_set: two_face::syntax::extra_newlines(),
            theme_set: ThemeSet::load_defaults(),
            cache: HashMap::new(),
            syntax_cache: HashMap::new(),
            last_file: String::new(),
            last_syntax_idx: None,
        }
    }

    /// Look up the syntax index for a filename, using cached extension mapping.
    /// Returns the index into `self.syntax_set.syntaxes()`.
    fn syntax_index_for_file(&mut self, filename: &str) -> usize {
        // Fast path: same file as last call (common during sequential line rendering)
        if filename == self.last_file {
            if let Some(idx) = self.last_syntax_idx {
                return idx;
            }
        }

        let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();

        let idx = match self.syntax_cache.get(&ext) {
            Some(Some(idx)) => *idx,
            Some(None) => {
                let plain = self.syntax_set.find_syntax_plain_text();
                let plain_idx = self
                    .syntax_set
                    .syntaxes()
                    .iter()
                    .position(|s| std::ptr::eq(s, plain))
                    .unwrap_or(0);
                plain_idx
            }
            None => {
                // Cache miss — do the lookup once per unique extension
                let found = self.syntax_set.find_syntax_by_extension(&ext).or_else(|| {
                    // Fallback for extensions not directly in the set
                    let fallback = match ext.as_str() {
                        "svelte" | "vue" | "astro" => "HTML",
                        _ => return None,
                    };
                    self.syntax_set.find_syntax_by_name(fallback)
                });

                let syntax_idx = found.map(|syntax| {
                    self.syntax_set
                        .syntaxes()
                        .iter()
                        .position(|s| std::ptr::eq(s, syntax))
                        .unwrap_or(0)
                });

                self.syntax_cache.insert(ext, syntax_idx);
                syntax_idx.unwrap_or_else(|| {
                    let plain = self.syntax_set.find_syntax_plain_text();
                    self.syntax_set
                        .syntaxes()
                        .iter()
                        .position(|s| std::ptr::eq(s, plain))
                        .unwrap_or(0)
                })
            }
        };

        self.last_file = filename.to_string();
        self.last_syntax_idx = Some(idx);
        idx
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

        // Check cache
        if let Some(cached) = self.cache.get(&key) {
            return cached
                .iter()
                .map(|cs| Span::styled(cs.text.clone(), base_style.fg(cs.fg)))
                .collect();
        }

        // Cache miss — look up syntax (cached by extension)
        let syntax_idx = self.syntax_index_for_file(filename);
        let syntax = &self.syntax_set.syntaxes()[syntax_idx];

        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        let input = if line.ends_with('\n') {
            line.to_string()
        } else {
            format!("{}\n", line)
        };

        match highlighter.highlight_line(&input, &self.syntax_set) {
            Ok(ranges) => {
                let cached_spans: Vec<CachedSpan> = ranges
                    .iter()
                    .map(|(syn_style, text)| {
                        let text = text.trim_end_matches('\n');
                        CachedSpan {
                            text: text.to_string(),
                            fg: Color::Rgb(
                                syn_style.foreground.r,
                                syn_style.foreground.g,
                                syn_style.foreground.b,
                            ),
                        }
                    })
                    .collect();

                let result = cached_spans
                    .iter()
                    .map(|cs| Span::styled(cs.text.clone(), base_style.fg(cs.fg)))
                    .collect();

                // Store in cache — partial eviction if too large
                if self.cache.len() >= MAX_CACHE_SIZE {
                    let keys_to_remove: Vec<u64> = self
                        .cache
                        .keys()
                        .take(MAX_CACHE_SIZE / 2)
                        .copied()
                        .collect();
                    for k in keys_to_remove {
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
