#![expect(
    non_snake_case,
    reason = "each accessor reads as a colour constant at the call site (`styles::PURPLE()`); \
              it is a function only so the theme can change at runtime"
)]

use ratatui::style::{Color, Modifier, Style};

// ── Background colors ──
pub fn BG() -> Color {
    super::themes::current().bg
}
pub fn SURFACE() -> Color {
    super::themes::current().surface
}
pub fn PANEL() -> Color {
    super::themes::current().panel
}
pub fn BORDER() -> Color {
    super::themes::current().border
}

// ── Text colors ──
pub fn TEXT() -> Color {
    super::themes::current().text
}
pub fn DIM() -> Color {
    super::themes::current().text_dim
}
pub fn MUTED() -> Color {
    super::themes::current().text_muted
}
pub fn BRIGHT() -> Color {
    super::themes::current().text_bright
}

// ── Accent colors ──
pub fn BLUE() -> Color {
    super::themes::current().blue
}
pub fn CYAN() -> Color {
    super::themes::current().cyan
}
pub fn GREEN() -> Color {
    super::themes::current().green
}
pub fn YELLOW() -> Color {
    super::themes::current().yellow
}
pub fn RED() -> Color {
    super::themes::current().red
}
pub fn RED_TEXT() -> Color {
    RED()
}
pub fn PURPLE() -> Color {
    super::themes::current().purple
}

// ── AI overlay colors ──
pub fn ORANGE() -> Color {
    super::themes::current().orange
}

// ── Diff colors ──
pub fn ADD_BG() -> Color {
    super::themes::current().add_bg
}
pub fn ADD_TEXT() -> Color {
    super::themes::current().add_text
}
pub fn DEL_BG() -> Color {
    super::themes::current().del_bg
}
pub fn DEL_TEXT() -> Color {
    super::themes::current().del_text
}
pub fn HUNK_BG() -> Color {
    super::themes::current().hunk_bg
}

// ── Interactive colors ──
pub fn LINE_CURSOR_BG() -> Color {
    super::themes::current().line_cursor_bg
}
pub fn FINDING_BG() -> Color {
    super::themes::current().finding_bg
}
pub fn FINDING_FOCUS_BG() -> Color {
    super::themes::current().finding_focus_bg
}
pub fn COMMENT_BG() -> Color {
    super::themes::current().comment_bg
}
pub fn INLINE_COMMENT_BG() -> Color {
    super::themes::current().inline_comment_bg
}
pub fn COMMENT_FOCUS_BG() -> Color {
    super::themes::current().comment_focus_bg
}

// ── Status colors ──
pub fn STALE() -> Color {
    super::themes::current().stale
}
pub fn WATCHED_TEXT() -> Color {
    super::themes::current().watched_text
}
pub fn WATCHED_MUTED() -> Color {
    super::themes::current().watched_muted
}
pub fn WATCHED_BG() -> Color {
    super::themes::current().watched_bg
}
pub fn UNMERGED() -> Color {
    super::themes::current().unmerged
}
pub fn RELOCATED_INDICATOR() -> Color {
    super::themes::current().relocated_indicator
}
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

pub fn status_style(status: &er_engine::git::FileStatus) -> Style {
    use er_engine::git::FileStatus;
    match status {
        FileStatus::Added => status_added(),
        FileStatus::Deleted => status_deleted(),
        _ => status_modified(),
    }
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
