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
        let channel = |range| {
            hex.get(range)
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .unwrap_or(204)
        };
        Color::Rgb(channel(1..3), channel(3..5), channel(5..7))
    } else {
        Color::Reset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_color_reads_rgb() {
        assert_eq!(parse_hex_color("#1a2b3c"), Color::Rgb(0x1a, 0x2b, 0x3c));
        assert_eq!(parse_hex_color("1a2b3c"), Color::Reset);
    }

    #[test]
    fn parse_hex_color_survives_multibyte_input() {
        // 7 bytes, but byte 3 falls inside 'é': slicing [1..3] used to panic.
        assert_eq!(parse_hex_color("#aé123"), Color::Rgb(204, 204, 0x23));
    }
}
