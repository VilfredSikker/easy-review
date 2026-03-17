use crate::config::ThemeConfig;
use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

/// Resolved theme — all colors with fallback to brand defaults.
#[derive(Debug, Clone)]
pub struct Theme {
    // Background colors
    pub bg: Color,
    pub surface: Color,
    pub panel: Color,
    pub border: Color,

    // Text colors
    pub text: Color,
    pub dim: Color,
    pub muted: Color,
    pub bright: Color,

    // Accent colors
    pub blue: Color,
    pub cyan: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub purple: Color,
    pub orange: Color,

    // Diff colors
    pub add_bg: Color,
    pub add_text: Color,
    pub del_bg: Color,
    pub del_text: Color,
    pub hunk_bg: Color,
}

/// Built-in brand defaults (cool blue undertone).
impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::Rgb(11, 11, 15),
            surface: Color::Rgb(19, 19, 26),
            panel: Color::Rgb(26, 26, 36),
            border: Color::Rgb(42, 42, 58),

            text: Color::Rgb(228, 228, 239),
            dim: Color::Rgb(136, 136, 160),
            muted: Color::Rgb(85, 85, 106),
            bright: Color::Rgb(232, 232, 242),

            blue: Color::Rgb(96, 165, 250),
            cyan: Color::Rgb(34, 211, 238),
            green: Color::Rgb(74, 222, 128),
            yellow: Color::Rgb(250, 204, 21),
            red: Color::Rgb(248, 113, 113),
            purple: Color::Rgb(167, 139, 250),
            orange: Color::Rgb(251, 146, 60),

            add_bg: Color::Rgb(16, 36, 28),
            add_text: Color::Rgb(74, 222, 128),
            del_bg: Color::Rgb(42, 16, 22),
            del_text: Color::Rgb(248, 113, 113),
            hunk_bg: Color::Rgb(22, 22, 42),
        }
    }
}

impl Theme {
    /// Build a Theme from config overrides, falling back to brand defaults.
    pub fn from_config(config: &ThemeConfig) -> Self {
        let d = Self::default();
        Self {
            bg: config.bg.unwrap_or(d.bg),
            surface: config.surface.unwrap_or(d.surface),
            panel: config.panel.unwrap_or(d.panel),
            border: config.border.unwrap_or(d.border),

            text: config.text.unwrap_or(d.text),
            dim: config.dim.unwrap_or(d.dim),
            muted: config.muted.unwrap_or(d.muted),
            bright: config.bright.unwrap_or(d.bright),

            blue: config.blue.unwrap_or(d.blue),
            cyan: config.cyan.unwrap_or(d.cyan),
            green: config.green.unwrap_or(d.green),
            yellow: config.yellow.unwrap_or(d.yellow),
            red: config.red.unwrap_or(d.red),
            purple: config.purple.unwrap_or(d.purple),
            orange: config.orange.unwrap_or(d.orange),

            add_bg: config.add_bg.unwrap_or(d.add_bg),
            add_text: config.add_text.unwrap_or(d.add_text),
            del_bg: config.del_bg.unwrap_or(d.del_bg),
            del_text: config.del_text.unwrap_or(d.del_text),
            hunk_bg: config.hunk_bg.unwrap_or(d.hunk_bg),
        }
    }
}

static THEME: OnceLock<Theme> = OnceLock::new();

/// Initialize the global theme from config. Call once at startup.
pub fn init_theme(config: &ThemeConfig) {
    let _ = THEME.set(Theme::from_config(config));
}

/// Get the active theme (falls back to brand defaults if not initialized).
pub fn theme() -> &'static Theme {
    THEME.get_or_init(Theme::default)
}

// ── Color accessors (read from theme, fall back to brand defaults) ──

pub fn bg() -> Color {
    theme().bg
}
pub fn surface() -> Color {
    theme().surface
}
pub fn panel() -> Color {
    theme().panel
}
pub fn border() -> Color {
    theme().border
}

pub fn text() -> Color {
    theme().text
}
pub fn dim_color() -> Color {
    theme().dim
}
pub fn muted() -> Color {
    theme().muted
}
pub fn bright() -> Color {
    theme().bright
}

pub fn blue() -> Color {
    theme().blue
}
pub fn cyan() -> Color {
    theme().cyan
}
pub fn green() -> Color {
    theme().green
}
pub fn yellow() -> Color {
    theme().yellow
}
pub fn red() -> Color {
    theme().red
}
pub fn red_text() -> Color {
    theme().red
}
pub fn purple() -> Color {
    theme().purple
}
pub fn orange() -> Color {
    theme().orange
}

pub fn add_bg() -> Color {
    theme().add_bg
}
pub fn add_text() -> Color {
    theme().add_text
}
pub fn del_bg() -> Color {
    theme().del_bg
}
pub fn del_text() -> Color {
    theme().del_text
}
pub fn hunk_bg() -> Color {
    theme().hunk_bg
}

// ── Derived colors (computed from theme values) ──

pub fn stale_color() -> Color {
    Color::Rgb(180, 160, 40)
}

pub fn finding_bg() -> Color {
    Color::Rgb(36, 28, 18)
}

pub fn finding_focus_bg() -> Color {
    Color::Rgb(50, 38, 22)
}

pub fn line_cursor_bg() -> Color {
    Color::Rgb(36, 28, 52)
}

pub fn comment_bg() -> Color {
    Color::Rgb(18, 28, 38)
}

pub fn inline_comment_bg() -> Color {
    Color::Rgb(22, 32, 42)
}

pub fn comment_focus_bg() -> Color {
    Color::Rgb(35, 50, 70)
}

pub fn unmerged() -> Color {
    Color::Rgb(255, 140, 0)
}

pub fn relocated_indicator() -> Color {
    Color::Rgb(100, 200, 150)
}

pub fn lost_indicator() -> Color {
    Color::Rgb(180, 100, 100)
}

pub fn watched_text() -> Color {
    Color::Rgb(120, 160, 220)
}

pub fn watched_muted() -> Color {
    Color::Rgb(70, 85, 110)
}

pub fn watched_bg() -> Color {
    Color::Rgb(14, 16, 24)
}

// ── Composed styles ──

pub fn default_style() -> Style {
    Style::default().fg(text()).bg(bg())
}

pub fn surface_style() -> Style {
    Style::default().fg(text()).bg(surface())
}

#[allow(dead_code)]
pub fn dim_style() -> Style {
    Style::default().fg(dim_color())
}

pub fn selected_style() -> Style {
    Style::default().fg(purple()).bg(Color::Rgb(30, 24, 48))
}

pub fn add_style() -> Style {
    Style::default().fg(add_text()).bg(add_bg())
}

pub fn del_style() -> Style {
    Style::default().fg(del_text()).bg(del_bg())
}

pub fn hunk_header_style() -> Style {
    Style::default().fg(purple()).bg(hunk_bg())
}

pub fn key_hint_style() -> Style {
    Style::default().fg(text()).add_modifier(Modifier::BOLD)
}

pub fn status_added() -> Style {
    Style::default().fg(green()).add_modifier(Modifier::BOLD)
}

pub fn status_deleted() -> Style {
    Style::default().fg(red()).add_modifier(Modifier::BOLD)
}

pub fn status_modified() -> Style {
    Style::default().fg(yellow()).add_modifier(Modifier::BOLD)
}

pub fn status_unmerged() -> Style {
    Style::default().fg(unmerged()).add_modifier(Modifier::BOLD)
}

pub fn status_resolved() -> Style {
    Style::default().fg(green()).add_modifier(Modifier::BOLD)
}

/// Risk dot styles
pub fn risk_high() -> Style {
    Style::default().fg(red()).add_modifier(Modifier::BOLD)
}

pub fn risk_medium() -> Style {
    Style::default().fg(orange()).add_modifier(Modifier::BOLD)
}

pub fn risk_low() -> Style {
    Style::default().fg(yellow())
}

#[allow(dead_code)]
pub fn risk_info() -> Style {
    Style::default().fg(blue())
}

/// Line cursor styles
pub fn line_cursor() -> Style {
    Style::default().fg(text()).bg(line_cursor_bg())
}

pub fn line_cursor_add() -> Style {
    Style::default().fg(add_text()).bg(line_cursor_bg())
}

pub fn line_cursor_del() -> Style {
    Style::default().fg(del_text()).bg(line_cursor_bg())
}

/// Human comment style
#[allow(dead_code)]
pub fn comment_style() -> Style {
    Style::default().fg(cyan()).bg(comment_bg())
}

/// Inline line-comment style
#[allow(dead_code)]
pub fn inline_comment_style() -> Style {
    Style::default().fg(cyan()).bg(inline_comment_bg())
}

/// Focused comment style
#[allow(dead_code)]
pub fn comment_focus_style() -> Style {
    Style::default().fg(cyan()).bg(comment_focus_bg())
}

/// Stale warning style
pub fn stale_style() -> Style {
    Style::default().fg(stale_color())
}

/// Watched file content line style
pub fn watched_line_style() -> Style {
    Style::default().fg(text()).bg(watched_bg())
}

/// Watched file gutter style
pub fn watched_gutter_style() -> Style {
    Style::default().fg(dim_color()).bg(watched_bg())
}

/// Focused pane border in split diff view
pub fn split_border_focused() -> Style {
    Style::default().fg(blue())
}

/// Inactive pane border in split diff view
pub fn split_border_inactive() -> Style {
    Style::default().fg(border())
}
