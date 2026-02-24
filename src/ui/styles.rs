use ratatui::style::{Color, Modifier, Style};

// ── Background colors ──
pub const BG: Color = Color::Rgb(12, 12, 12);
pub const SURFACE: Color = Color::Rgb(20, 20, 20);
pub const PANEL: Color = Color::Rgb(26, 26, 26);
pub const BORDER: Color = Color::Rgb(42, 42, 42);

// ── Text colors ──
pub const TEXT: Color = Color::Rgb(200, 200, 200);
pub const DIM: Color = Color::Rgb(102, 102, 102);
pub const MUTED: Color = Color::Rgb(136, 136, 136);
pub const BRIGHT: Color = Color::Rgb(232, 232, 232);

// ── Accent colors ──
pub const BLUE: Color = Color::Rgb(96, 165, 250);
pub const CYAN: Color = Color::Rgb(34, 211, 238);
pub const GREEN: Color = Color::Rgb(74, 222, 128);
pub const YELLOW: Color = Color::Rgb(250, 204, 21);
pub const RED: Color = Color::Rgb(248, 113, 113);
pub const PURPLE: Color = Color::Rgb(167, 139, 250);

// ── Diff colors ──
pub const ADD_BG: Color = Color::Rgb(16, 62, 40);
pub const ADD_TEXT: Color = Color::Rgb(120, 240, 160);
pub const DEL_BG: Color = Color::Rgb(68, 16, 24);
pub const DEL_TEXT: Color = Color::Rgb(255, 140, 140);
pub const HUNK_BG: Color = Color::Rgb(28, 28, 60);

// ── Composed styles ──

pub fn default_style() -> Style {
    Style::default().fg(TEXT).bg(BG)
}

pub fn surface_style() -> Style {
    Style::default().fg(TEXT).bg(SURFACE)
}

#[allow(dead_code)]
pub fn dim_style() -> Style {
    Style::default().fg(DIM)
}

pub fn selected_style() -> Style {
    Style::default().fg(BLUE).bg(Color::Rgb(26, 42, 58))
}

pub fn add_style() -> Style {
    Style::default().fg(ADD_TEXT).bg(ADD_BG)
}

pub fn del_style() -> Style {
    Style::default().fg(DEL_TEXT).bg(DEL_BG)
}

pub fn hunk_header_style() -> Style {
    Style::default().fg(PURPLE).bg(HUNK_BG)
}

pub fn key_hint_style() -> Style {
    Style::default().fg(MUTED).add_modifier(Modifier::BOLD)
}

pub fn status_added() -> Style {
    Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
}

pub fn status_deleted() -> Style {
    Style::default().fg(RED).add_modifier(Modifier::BOLD)
}

pub fn status_modified() -> Style {
    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
}
