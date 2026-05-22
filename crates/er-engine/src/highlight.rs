use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::{SyntaxDefinition, SyntaxSet};

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
        let mut theme_set = ThemeSet::load_defaults();
        let extra: ThemeSet = (&two_face::theme::extra()).into();
        theme_set.themes.extend(extra.themes);

        // Extend two_face's set with Svelte (and any future custom syntaxes).
        // into_builder() decompiles the frozen SyntaxSet back into a builder so
        // we can add definitions before re-compiling with build().
        let mut builder = two_face::syntax::extra_newlines().into_builder();
        const SVELTE_SYNTAX: &str =
            include_str!("syntaxes/Svelte.sublime-syntax");
        if let Ok(def) = SyntaxDefinition::load_from_str(SVELTE_SYNTAX, true, None) {
            builder.add(def);
        }

        Highlighter {
            syntax_set: builder.build(),
            theme_set,
            cache: HashMap::new(),
            gen: 0,
        }
    }

    /// Highlight a single line. `theme_name` is a syntect theme name (e.g.
    /// "OneHalfDark"). Falls back to "OneHalfDark" if not found.
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

        // Svelte/Vue/Astro: syntect's `extends` doesn't resolve the embedded
        // TypeScript scopes from the bundled HTML syntax, so keywords inside
        // <script> blocks render as default text. Pragmatic fallback:
        // highlight the whole file as TypeScript — sacrifices template markup
        // coloring for proper script highlighting, which is what code reviewers
        // actually look at.
        let ext_lower = filename
            .rsplit('.')
            .next()
            .map(|s| s.to_ascii_lowercase());
        // Force TypeScript for:
        // - All .ts/.tsx/.cts/.mts files (Path::extension() returns just the
        //   last segment, so foo.unit.ts → "ts", but be explicit here in case
        //   the syntect default detection ever skips compound-extension files).
        // - .svelte/.vue/.astro — syntect's `extends` doesn't resolve embedded
        //   TS scopes from two_face's HTML syntax, so use TS for the whole file.
        let force_ts = matches!(
            ext_lower.as_deref(),
            Some("ts") | Some("tsx") | Some("cts") | Some("mts")
                | Some("svelte") | Some("vue") | Some("astro")
        );
        let syntax = if force_ts {
            self.syntax_set
                .find_syntax_by_extension("ts")
                .or_else(|| self.syntax_set.find_syntax_by_name("TypeScript"))
        } else {
            None
        }
        .or_else(|| {
            self.syntax_set
                .find_syntax_for_file(filename)
                .ok()
                .flatten()
        })
        .or_else(|| {
            ext_lower
                .as_deref()
                .and_then(|ext| self.syntax_set.find_syntax_by_extension(ext))
        })
        .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let theme = self
            .theme_set
            .themes
            .get(theme_name)
            .or_else(|| self.theme_set.themes.get("OneHalfDark"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts_keywords_highlighted_in_unit_test() {
        let mut h = Highlighter::new();
        let spans = h.highlight_line(
            "import { foo } from './bar';",
            "something.unit.test.ts",
            "OneHalfDark",
        );
        let mut colors: Vec<(String, String)> = Vec::new();
        for s in &spans {
            colors.push((s.text.clone(), s.color.clone()));
        }
        println!("=== .unit.test.ts spans ===");
        for (t, c) in &colors {
            println!("  {:?} => {}", t, c);
        }
        // Look for distinct colors — `import`, `from` must NOT be default.
        let distinct: std::collections::HashSet<_> = colors.iter().map(|(_, c)| c.as_str()).collect();
        assert!(
            distinct.len() > 2,
            "expected multiple distinct colors, got: {:?}",
            distinct
        );
    }

    #[test]
    fn ts_keywords_highlighted_in_svelte_script() {
        let mut h = Highlighter::new();
        let spans = h.highlight_line(
            "  import { foo } from './bar';",
            "Component.svelte",
            "OneHalfDark",
        );
        println!("=== Component.svelte (import line) spans ===");
        for s in &spans {
            println!("  {:?} => {}", s.text, s.color);
        }
        let distinct: std::collections::HashSet<_> =
            spans.iter().map(|s| s.color.as_str()).collect();
        assert!(
            distinct.len() > 2,
            "expected Svelte fallback to give TS-like highlighting, got: {:?}",
            distinct
        );
    }

    #[test]
    fn ts_file_extension_resolved() {
        let h = Highlighter::new();
        let s = h.syntax_set.find_syntax_by_extension("ts");
        assert!(s.is_some(), "TypeScript not found by .ts extension");
        let s = s.unwrap();
        println!("ts extension → {} (scope: {})", s.name, s.scope);
    }

    #[test]
    fn unit_ts_file_highlights() {
        let mut h = Highlighter::new();
        let spans = h.highlight_line(
            "import { describe } from 'vitest';",
            "experiment-template-resolution.unit.ts",
            "OneHalfDark",
        );
        println!("=== .unit.ts spans ===");
        for s in &spans {
            println!("  {:?} => {}", s.text, s.color);
        }
        let distinct: std::collections::HashSet<_> =
            spans.iter().map(|s| s.color.as_str()).collect();
        assert!(distinct.len() > 2, "got colors: {:?}", distinct);
    }
}
