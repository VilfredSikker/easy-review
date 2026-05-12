use ratatui::style::{Color, Modifier, Style};

// ── Background colors ──
#[allow(non_snake_case)]
pub fn BG() -> Color {
    super::themes::current().bg
}
#[allow(non_snake_case)]
pub fn SURFACE() -> Color {
    super::themes::current().surface
}
#[allow(non_snake_case)]
pub fn PANEL() -> Color {
    super::themes::current().panel
}
#[allow(non_snake_case)]
pub fn BORDER() -> Color {
    super::themes::current().border
}

// ── Text colors ──
#[allow(non_snake_case)]
pub fn TEXT() -> Color {
    super::themes::current().text
}
#[allow(non_snake_case)]
pub fn DIM() -> Color {
    super::themes::current().text_dim
}
#[allow(non_snake_case)]
pub fn MUTED() -> Color {
    super::themes::current().text_muted
}
#[allow(non_snake_case)]
pub fn BRIGHT() -> Color {
    super::themes::current().text_bright
}

// ── Accent colors ──
#[allow(non_snake_case)]
pub fn BLUE() -> Color {
    super::themes::current().blue
}
#[allow(non_snake_case)]
pub fn CYAN() -> Color {
    super::themes::current().cyan
}
#[allow(non_snake_case)]
pub fn GREEN() -> Color {
    super::themes::current().green
}
#[allow(non_snake_case)]
pub fn YELLOW() -> Color {
    super::themes::current().yellow
}
#[allow(non_snake_case)]
pub fn RED() -> Color {
    super::themes::current().red
}
#[allow(non_snake_case)]
pub fn RED_TEXT() -> Color {
    RED()
}
#[allow(non_snake_case)]
pub fn PURPLE() -> Color {
    super::themes::current().purple
}

// ── AI overlay colors ──
#[allow(non_snake_case)]
pub fn ORANGE() -> Color {
    super::themes::current().orange
}

// ── Diff colors ──
#[allow(non_snake_case)]
pub fn ADD_BG() -> Color {
    super::themes::current().add_bg
}
#[allow(non_snake_case)]
pub fn ADD_TEXT() -> Color {
    super::themes::current().add_text
}
#[allow(non_snake_case)]
pub fn DEL_BG() -> Color {
    super::themes::current().del_bg
}
#[allow(non_snake_case)]
pub fn DEL_TEXT() -> Color {
    super::themes::current().del_text
}
#[allow(non_snake_case)]
pub fn HUNK_BG() -> Color {
    super::themes::current().hunk_bg
}

// ── Interactive colors ──
#[allow(non_snake_case)]
pub fn LINE_CURSOR_BG() -> Color {
    super::themes::current().line_cursor_bg
}
#[allow(non_snake_case)]
pub fn FINDING_BG() -> Color {
    super::themes::current().finding_bg
}
#[allow(non_snake_case)]
pub fn FINDING_FOCUS_BG() -> Color {
    super::themes::current().finding_focus_bg
}
#[allow(non_snake_case)]
pub fn COMMENT_BG() -> Color {
    super::themes::current().comment_bg
}
#[allow(non_snake_case)]
pub fn INLINE_COMMENT_BG() -> Color {
    super::themes::current().inline_comment_bg
}
#[allow(non_snake_case)]
pub fn COMMENT_FOCUS_BG() -> Color {
    super::themes::current().comment_focus_bg
}

// ── Status colors ──
#[allow(non_snake_case)]
pub fn STALE() -> Color {
    super::themes::current().stale
}
#[allow(non_snake_case)]
pub fn WATCHED_TEXT() -> Color {
    super::themes::current().watched_text
}
#[allow(non_snake_case)]
pub fn WATCHED_MUTED() -> Color {
    super::themes::current().watched_muted
}
#[allow(non_snake_case)]
pub fn WATCHED_BG() -> Color {
    super::themes::current().watched_bg
}
#[allow(non_snake_case)]
pub fn UNMERGED() -> Color {
    super::themes::current().unmerged
}
#[allow(non_snake_case)]
pub fn RELOCATED_INDICATOR() -> Color {
    super::themes::current().relocated_indicator
}
#[allow(non_snake_case)]
pub fn LOST_INDICATOR() -> Color {
    super::themes::current().lost_indicator
}

// ── Composed styles ──

pub fn default_style() -> Style {
    Style::default().fg(TEXT()).bg(BG())
}

pub fn surface_style() -> Style {
    Style::default().fg(TEXT()).bg(SURFACE())
}

#[allow(dead_code)]
pub fn dim_style() -> Style {
    Style::default().fg(DIM())
}

pub fn selected_style() -> Style {
    Style::default()
        .fg(PURPLE())
        .bg(super::themes::current().selected_bg)
}

pub fn add_style() -> Style {
    Style::default().fg(ADD_TEXT()).bg(ADD_BG())
}

pub fn del_style() -> Style {
    Style::default().fg(DEL_TEXT()).bg(DEL_BG())
}

pub fn hunk_header_style() -> Style {
    Style::default().fg(PURPLE()).bg(HUNK_BG())
}

pub fn key_hint_style() -> Style {
    Style::default().fg(TEXT()).add_modifier(Modifier::BOLD)
}

pub fn status_added() -> Style {
    Style::default().fg(GREEN()).add_modifier(Modifier::BOLD)
}

pub fn status_deleted() -> Style {
    Style::default().fg(RED()).add_modifier(Modifier::BOLD)
}

pub fn status_modified() -> Style {
    Style::default().fg(YELLOW()).add_modifier(Modifier::BOLD)
}

pub fn status_unmerged() -> Style {
    Style::default().fg(UNMERGED()).add_modifier(Modifier::BOLD)
}

pub fn status_resolved() -> Style {
    Style::default().fg(GREEN()).add_modifier(Modifier::BOLD)
}

/// Risk dot styles
pub fn risk_high() -> Style {
    Style::default().fg(RED()).add_modifier(Modifier::BOLD)
}

pub fn risk_medium() -> Style {
    Style::default().fg(ORANGE()).add_modifier(Modifier::BOLD)
}

pub fn risk_low() -> Style {
    Style::default().fg(YELLOW())
}

#[allow(dead_code)]
pub fn risk_info() -> Style {
    Style::default().fg(BLUE())
}

/// Line cursor styles — brighter bg to show selected line
pub fn line_cursor() -> Style {
    Style::default().fg(TEXT()).bg(LINE_CURSOR_BG())
}

pub fn line_cursor_add() -> Style {
    Style::default().fg(ADD_TEXT()).bg(LINE_CURSOR_BG())
}

pub fn line_cursor_del() -> Style {
    Style::default().fg(DEL_TEXT()).bg(LINE_CURSOR_BG())
}

/// Human comment style
#[allow(dead_code)]
pub fn comment_style() -> Style {
    Style::default().fg(CYAN()).bg(COMMENT_BG())
}

/// Inline line-comment style
#[allow(dead_code)]
pub fn inline_comment_style() -> Style {
    Style::default().fg(CYAN()).bg(INLINE_COMMENT_BG())
}

/// Focused comment style
#[allow(dead_code)]
pub fn comment_focus_style() -> Style {
    Style::default().fg(CYAN()).bg(COMMENT_FOCUS_BG())
}

/// Stale warning style
pub fn stale_style() -> Style {
    Style::default().fg(STALE())
}

/// Watched file content line style
pub fn watched_line_style() -> Style {
    Style::default().fg(TEXT()).bg(WATCHED_BG())
}

/// Watched file gutter style
pub fn watched_gutter_style() -> Style {
    Style::default().fg(DIM()).bg(WATCHED_BG())
}

// ── Split diff view styles ──

/// Focused pane border in split diff view
pub fn split_border_focused() -> Style {
    Style::default().fg(BLUE())
}

/// Inactive pane border in split diff view
pub fn split_border_inactive() -> Style {
    Style::default().fg(BORDER())
}
