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
}

/// Maximum cache entries before eviction
const MAX_CACHE_SIZE: usize = 10_000;

impl Highlighter {
    pub fn new() -> Self {
        Highlighter {
            syntax_set: two_face::syntax::extra_newlines(),
            theme_set: ThemeSet::load_defaults(),
            cache: HashMap::new(),
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

        // Check cache
        if let Some(cached) = self.cache.get(&key) {
            return cached
                .iter()
                .map(|cs| Span::styled(cs.text.clone(), base_style.fg(cs.fg)))
                .collect();
        }

        // Cache miss — perform highlighting
        // two-face's syntax set covers TS, TSX, TOML, Svelte (via HTML), etc.
        // Fall back to HTML for .svelte/.vue/.astro which aren't in the set directly.
        let syntax = self
            .syntax_set
            .find_syntax_for_file(filename)
            .ok()
            .flatten()
            .or_else(|| {
                let fallback = match filename.rsplit('.').next() {
                    Some("svelte" | "vue" | "astro") => "HTML",
                    _ => return None,
                };
                self.syntax_set.find_syntax_by_name(fallback)
            })
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

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

                // Store in cache (evict if too large)
                if self.cache.len() >= MAX_CACHE_SIZE {
                    self.cache.clear();
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
