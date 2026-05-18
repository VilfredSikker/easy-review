use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// A syntax-highlighted text span with color as hex string.
#[derive(Debug, Clone)]
pub struct HighlightSpan {
    pub text: String,
    /// Foreground color as "#RRGGBB", or empty string for default color.
    pub color: String,
}

/// Cache entry: (text, color_hex) per span in the line.
#[derive(Debug, Clone)]
struct CachedLine {
    spans: Vec<(String, String)>,
    /// LRU generation counter for eviction
    access_gen: u64,
}

fn cache_key(line: &str, filename: &str, theme_name: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut hasher);
    filename.hash(&mut hasher);
    theme_name.hash(&mut hasher);
    hasher.finish()
}

const MAX_CACHE_SIZE: usize = 10_000;
const EVICT_COUNT: usize = MAX_CACHE_SIZE / 4;

/// Engine-side syntax highlighter. Outputs color-annotated spans without any
/// dependency on a specific rendering library.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    cache: HashMap<u64, CachedLine>,
    gen: u64,
}

impl Highlighter {
    pub fn new() -> Self {
        Highlighter {
            syntax_set: two_face::syntax::extra_newlines(),
            theme_set: ThemeSet::load_defaults(),
            cache: HashMap::new(),
            gen: 0,
        }
    }

    /// Highlight a single line. `theme_name` is a syntect theme name (e.g.
    /// "base16-ocean.dark"). Falls back to "base16-ocean.dark" if not found.
    pub fn highlight_line(
        &mut self,
        line: &str,
        filename: &str,
        theme_name: &str,
    ) -> Vec<HighlightSpan> {
        let key = cache_key(line, filename, theme_name);

        if let Some(cached) = self.cache.get(&key) {
            return cached
                .spans
                .iter()
                .map(|(t, c)| HighlightSpan {
                    text: t.clone(),
                    color: c.clone(),
                })
                .collect();
        }

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

        let theme = self
            .theme_set
            .themes
            .get(theme_name)
            .or_else(|| self.theme_set.themes.get("base16-ocean.dark"))
            .unwrap_or_else(|| self.theme_set.themes.values().next().unwrap());

        let mut hl = HighlightLines::new(syntax, theme);
        let input = if line.ends_with('\n') {
            line.to_string()
        } else {
            format!("{}\n", line)
        };

        match hl.highlight_line(&input, &self.syntax_set) {
            Ok(ranges) => {
                self.gen += 1;
                let current_gen = self.gen;

                let spans: Vec<(String, String)> = ranges
                    .iter()
                    .map(|(style, text)| {
                        let text = text.trim_end_matches('\n').to_string();
                        let color = format!(
                            "#{:02x}{:02x}{:02x}",
                            style.foreground.r, style.foreground.g, style.foreground.b
                        );
                        (text, color)
                    })
                    .collect();

                let result = spans
                    .iter()
                    .map(|(t, c)| HighlightSpan {
                        text: t.clone(),
                        color: c.clone(),
                    })
                    .collect();

                if self.cache.len() >= MAX_CACHE_SIZE {
                    let mut entries: Vec<(u64, u64)> =
                        self.cache.iter().map(|(k, v)| (*k, v.access_gen)).collect();
                    entries.sort_unstable_by_key(|&(_, g)| g);
                    for (k, _) in entries.into_iter().take(EVICT_COUNT) {
                        self.cache.remove(&k);
                    }
                }

                self.cache.insert(
                    key,
                    CachedLine {
                        spans,
                        access_gen: current_gen,
                    },
                );
                result
            }
            Err(_) => vec![HighlightSpan {
                text: line.to_string(),
                color: String::new(),
            }],
        }
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}
