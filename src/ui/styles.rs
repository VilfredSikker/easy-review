use ratatui::style::{Color, Modifier, Style};

// ── Background colors (cool blue undertone — matches mockup aesthetic) ──
pub const BG: Color = Color::Rgb(11, 11, 15);
pub const SURFACE: Color = Color::Rgb(19, 19, 26);
pub const PANEL: Color = Color::Rgb(26, 26, 36);
pub const BORDER: Color = Color::Rgb(42, 42, 58);

// ── Text colors (lavender-tinted grays for depth) ──
pub const TEXT: Color = Color::Rgb(228, 228, 239);
pub const DIM: Color = Color::Rgb(136, 136, 160);
pub const MUTED: Color = Color::Rgb(85, 85, 106);
pub const BRIGHT: Color = Color::Rgb(232, 232, 242);

// ── Accent colors ──
pub const BLUE: Color = Color::Rgb(96, 165, 250);
pub const CYAN: Color = Color::Rgb(34, 211, 238);
pub const GREEN: Color = Color::Rgb(74, 222, 128);
pub const YELLOW: Color = Color::Rgb(250, 204, 21);
pub const RED: Color = Color::Rgb(248, 113, 113);
pub const PURPLE: Color = Color::Rgb(167, 139, 250);

// ── Diff colors (subtle tinted backgrounds, vivid text) ──
pub const ADD_BG: Color = Color::Rgb(16, 36, 28);
pub const ADD_TEXT: Color = Color::Rgb(74, 222, 128);
pub const DEL_BG: Color = Color::Rgb(42, 16, 22);
pub const DEL_TEXT: Color = Color::Rgb(248, 113, 113);
pub const HUNK_BG: Color = Color::Rgb(22, 22, 42);

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
    Style::default().fg(PURPLE).bg(Color::Rgb(30, 24, 48))
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
    Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
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

// ── AI overlay colors ──

pub const ORANGE: Color = Color::Rgb(251, 146, 60);

/// Stale indicator color (dimmed yellow)
pub const STALE: Color = Color::Rgb(180, 160, 40);

/// AI finding banner background (warm tint against cool bg for contrast)
pub const FINDING_BG: Color = Color::Rgb(36, 28, 18);

/// Risk dot styles
pub fn risk_high() -> Style {
    Style::default().fg(RED).add_modifier(Modifier::BOLD)
}

pub fn risk_medium() -> Style {
    Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
}

pub fn risk_low() -> Style {
    Style::default().fg(YELLOW)
}

#[allow(dead_code)]
pub fn risk_info() -> Style {
    Style::default().fg(BLUE)
}

/// Finding banner style
pub fn finding_style() -> Style {
    Style::default().fg(ORANGE).bg(FINDING_BG)
}

/// Line cursor background (subtle purple tint to match selection theme)
pub const LINE_CURSOR_BG: Color = Color::Rgb(36, 28, 52);

/// Line cursor styles — brighter bg to show selected line
pub fn line_cursor() -> Style {
    Style::default().fg(TEXT).bg(LINE_CURSOR_BG)
}

pub fn line_cursor_add() -> Style {
    Style::default().fg(ADD_TEXT).bg(LINE_CURSOR_BG)
}

pub fn line_cursor_del() -> Style {
    Style::default().fg(DEL_TEXT).bg(LINE_CURSOR_BG)
}

/// Human comment background (cool tint to distinguish from AI findings)
pub const COMMENT_BG: Color = Color::Rgb(18, 28, 38);

/// Inline line-comment background (slightly lighter for visual distinction)
pub const INLINE_COMMENT_BG: Color = Color::Rgb(22, 32, 42);

/// Focused comment background (highlighted border/bg for selected comment)
pub const COMMENT_FOCUS_BG: Color = Color::Rgb(28, 38, 52);

/// Human comment style
#[allow(dead_code)]
pub fn comment_style() -> Style {
    Style::default().fg(CYAN).bg(COMMENT_BG)
}

/// Inline line-comment style
#[allow(dead_code)]
pub fn inline_comment_style() -> Style {
    Style::default().fg(CYAN).bg(INLINE_COMMENT_BG)
}

/// Focused comment style
#[allow(dead_code)]
pub fn comment_focus_style() -> Style {
    Style::default().fg(CYAN).bg(COMMENT_FOCUS_BG)
}

/// Stale warning style
pub fn stale_style() -> Style {
    Style::default().fg(STALE)
}

// ── Watched file colors ──

/// Watched file text color (cool blue to distinguish from diff files)
pub const WATCHED_TEXT: Color = Color::Rgb(120, 160, 220);

/// Watched file separator/muted color
pub const WATCHED_MUTED: Color = Color::Rgb(70, 85, 110);

/// Watched file background for content view
pub const WATCHED_BG: Color = Color::Rgb(14, 16, 24);

/// Watched file content line style
pub fn watched_line_style() -> Style {
    Style::default().fg(TEXT).bg(WATCHED_BG)
}

/// Watched file gutter style
pub fn watched_gutter_style() -> Style {
    Style::default().fg(DIM).bg(WATCHED_BG)
}
