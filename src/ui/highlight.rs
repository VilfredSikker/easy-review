use ratatui::style::{Color, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// Cached syntax highlighting state — loaded once, reused for all files.
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Highlighter {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Highlight a single line of code, returning styled spans.
    /// `filename` is used to detect the language (e.g., "main.rs" → Rust).
    /// `base_style` is the background style to layer highlighting on top of
    /// (so add/delete background colors are preserved).
    pub fn highlight_line<'a>(
        &self,
        line: &'a str,
        filename: &str,
        base_style: Style,
    ) -> Vec<Span<'a>> {
        // Detect syntax from filename extension
        let syntax = self
            .syntax_set
            .find_syntax_for_file(filename)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        // Use a dark theme that works well with our dark TUI background
        let theme = &self.theme_set.themes["base16-ocean.dark"];

        let mut highlighter = HighlightLines::new(syntax, theme);

        // syntect needs a trailing newline
        let input = if line.ends_with('\n') {
            line.to_string()
        } else {
            format!("{}\n", line)
        };

        match highlighter.highlight_line(&input, &self.syntax_set) {
            Ok(ranges) => {
                ranges
                    .into_iter()
                    .map(|(syn_style, text)| {
                        // Strip trailing newline we added
                        let text = text.trim_end_matches('\n');
                        let fg = Color::Rgb(
                            syn_style.foreground.r,
                            syn_style.foreground.g,
                            syn_style.foreground.b,
                        );
                        // Layer syntax fg color on top of the base style
                        // (preserving add/delete background)
                        Span::styled(
                            text.to_string(),
                            base_style.fg(fg),
                        )
                    })
                    .collect()
            }
            Err(_) => {
                // Fallback: return unstyled
                vec![Span::styled(line.to_string(), base_style)]
            }
        }
    }
}
