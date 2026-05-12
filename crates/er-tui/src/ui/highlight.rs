use er_engine::highlight::Highlighter as EngineHighlighter;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

/// TUI syntax highlighter — thin adapter over the engine's Highlighter.
/// Converts `#RRGGBB` color strings to ratatui Color and layers them on a base style.
pub struct Highlighter(EngineHighlighter);

impl Highlighter {
    pub fn new() -> Self {
        Self(EngineHighlighter::new())
    }

    /// Highlight a single line of code, returning styled ratatui Spans.
    /// `base_style` carries the diff row background (add/del colors) which
    /// is preserved — only the foreground is overridden by syntax highlighting.
    pub fn highlight_line<'a>(
        &mut self,
        line: &'a str,
        filename: &str,
        base_style: Style,
    ) -> Vec<Span<'a>> {
        let theme = super::themes::current().syntect_theme.clone();
        self.0
            .highlight_line(line, filename, &theme)
            .into_iter()
            .map(|span| {
                let color = parse_hex_color(&span.color);
                Span::styled(span.text, base_style.fg(color))
            })
            .collect()
    }
}

fn parse_hex_color(hex: &str) -> Color {
    if hex.len() == 7 && hex.starts_with('#') {
        let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(204);
        let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(204);
        let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(204);
        Color::Rgb(r, g, b)
    } else {
        Color::Reset
    }
}
