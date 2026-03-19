use ratatui::style::Color;
use std::sync::{OnceLock, RwLock, RwLockReadGuard};

#[derive(Debug, Clone)]
pub struct Theme {
    #[allow(dead_code)]
    pub name: String,

    // Background layer
    pub bg: Color,
    pub surface: Color,
    pub panel: Color,
    pub border: Color,

    // Text layer
    pub text: Color,
    pub text_bright: Color,
    pub text_dim: Color,
    pub text_muted: Color,

    // Accent layer
    pub blue: Color,
    pub cyan: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub purple: Color,
    pub orange: Color,

    // Diff layer
    pub add_bg: Color,
    pub add_text: Color,
    pub del_bg: Color,
    pub del_text: Color,
    pub hunk_bg: Color,

    // Interactive layer
    pub line_cursor_bg: Color,
    pub selected_bg: Color,
    pub finding_bg: Color,
    pub finding_focus_bg: Color,
    pub comment_bg: Color,
    pub inline_comment_bg: Color,
    pub comment_focus_bg: Color,

    // Status layer
    pub stale: Color,
    pub watched_text: Color,
    pub watched_muted: Color,
    pub watched_bg: Color,
    pub unmerged: Color,
    pub relocated_indicator: Color,
    pub lost_indicator: Color,

    // Syntax highlighting
    pub syntect_theme: String,
}

static CURRENT_THEME: OnceLock<RwLock<Theme>> = OnceLock::new();

pub fn current() -> RwLockReadGuard<'static, Theme> {
    CURRENT_THEME
        .get_or_init(|| RwLock::new(ocean_depth()))
        .read()
        .unwrap()
}

pub fn set_theme(theme: Theme) {
    let lock = CURRENT_THEME.get_or_init(|| RwLock::new(ocean_depth()));
    *lock.write().unwrap() = theme;
}

pub fn set_theme_by_name(name: &str) {
    if let Some(theme) = theme_by_name(name) {
        set_theme(theme);
    }
}

pub fn theme_by_name(name: &str) -> Option<Theme> {
    match name {
        "ocean-depth" => Some(ocean_depth()),
        "moonlight" => Some(moonlight()),
        "daybreak" => Some(daybreak()),
        "high-contrast" => Some(high_contrast()),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn available_themes() -> Vec<&'static str> {
    vec!["ocean-depth", "moonlight", "daybreak", "high-contrast"]
}

pub fn ocean_depth() -> Theme {
    Theme {
        name: "ocean-depth".to_string(),

        bg: Color::Rgb(11, 11, 15),
        surface: Color::Rgb(19, 19, 26),
        panel: Color::Rgb(26, 26, 36),
        border: Color::Rgb(42, 42, 58),

        text: Color::Rgb(228, 228, 239),
        text_bright: Color::Rgb(232, 232, 242),
        text_dim: Color::Rgb(136, 136, 160),
        text_muted: Color::Rgb(85, 85, 106),

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

        line_cursor_bg: Color::Rgb(36, 28, 52),
        selected_bg: Color::Rgb(30, 24, 48),
        finding_bg: Color::Rgb(36, 28, 18),
        finding_focus_bg: Color::Rgb(50, 38, 22),
        comment_bg: Color::Rgb(18, 28, 38),
        inline_comment_bg: Color::Rgb(22, 32, 42),
        comment_focus_bg: Color::Rgb(35, 50, 70),

        stale: Color::Rgb(180, 160, 40),
        watched_text: Color::Rgb(120, 160, 220),
        watched_muted: Color::Rgb(70, 85, 110),
        watched_bg: Color::Rgb(14, 16, 24),
        unmerged: Color::Rgb(255, 140, 0),
        relocated_indicator: Color::Rgb(100, 200, 150),
        lost_indicator: Color::Rgb(180, 100, 100),

        syntect_theme: "base16-ocean.dark".to_string(),
    }
}

pub fn moonlight() -> Theme {
    Theme {
        name: "moonlight".to_string(),

        bg: Color::Rgb(14, 14, 18),
        surface: Color::Rgb(22, 22, 28),
        panel: Color::Rgb(30, 30, 38),
        border: Color::Rgb(50, 50, 62),

        text: Color::Rgb(210, 210, 220),
        text_bright: Color::Rgb(220, 220, 230),
        text_dim: Color::Rgb(120, 120, 140),
        text_muted: Color::Rgb(78, 78, 95),

        // Desaturated ~30% relative to Ocean Depth accents
        blue: Color::Rgb(110, 155, 220),
        cyan: Color::Rgb(80, 185, 200),
        green: Color::Rgb(100, 195, 130),
        yellow: Color::Rgb(210, 180, 60),
        red: Color::Rgb(210, 120, 120),
        purple: Color::Rgb(160, 140, 210),
        orange: Color::Rgb(210, 140, 80),

        add_bg: Color::Rgb(18, 34, 26),
        add_text: Color::Rgb(100, 195, 130),
        del_bg: Color::Rgb(38, 18, 22),
        del_text: Color::Rgb(210, 120, 120),
        hunk_bg: Color::Rgb(24, 24, 40),

        line_cursor_bg: Color::Rgb(36, 30, 50),
        selected_bg: Color::Rgb(30, 26, 44),
        finding_bg: Color::Rgb(34, 28, 20),
        finding_focus_bg: Color::Rgb(46, 36, 24),
        comment_bg: Color::Rgb(20, 28, 36),
        inline_comment_bg: Color::Rgb(24, 32, 40),
        comment_focus_bg: Color::Rgb(32, 46, 62),

        stale: Color::Rgb(165, 148, 48),
        watched_text: Color::Rgb(110, 148, 200),
        watched_muted: Color::Rgb(68, 80, 100),
        watched_bg: Color::Rgb(16, 17, 22),
        unmerged: Color::Rgb(230, 135, 20),
        relocated_indicator: Color::Rgb(90, 175, 135),
        lost_indicator: Color::Rgb(165, 98, 98),

        syntect_theme: "base16-ocean.dark".to_string(),
    }
}

pub fn daybreak() -> Theme {
    Theme {
        name: "daybreak".to_string(),

        bg: Color::Rgb(250, 250, 252),
        surface: Color::Rgb(242, 242, 246),
        panel: Color::Rgb(234, 234, 240),
        border: Color::Rgb(200, 200, 215),

        text: Color::Rgb(30, 30, 40),
        text_bright: Color::Rgb(15, 15, 25),
        text_dim: Color::Rgb(100, 100, 120),
        text_muted: Color::Rgb(148, 148, 165),

        blue: Color::Rgb(37, 99, 235),
        cyan: Color::Rgb(6, 148, 162),
        green: Color::Rgb(22, 163, 74),
        yellow: Color::Rgb(161, 120, 4),
        red: Color::Rgb(220, 38, 38),
        purple: Color::Rgb(124, 58, 237),
        orange: Color::Rgb(194, 88, 14),

        add_bg: Color::Rgb(220, 252, 231),
        add_text: Color::Rgb(22, 163, 74),
        del_bg: Color::Rgb(254, 226, 226),
        del_text: Color::Rgb(220, 38, 38),
        hunk_bg: Color::Rgb(226, 232, 248),

        line_cursor_bg: Color::Rgb(224, 220, 240),
        selected_bg: Color::Rgb(230, 225, 245),
        finding_bg: Color::Rgb(255, 243, 220),
        finding_focus_bg: Color::Rgb(255, 232, 196),
        comment_bg: Color::Rgb(218, 234, 250),
        inline_comment_bg: Color::Rgb(210, 228, 248),
        comment_focus_bg: Color::Rgb(195, 218, 244),

        stale: Color::Rgb(146, 120, 10),
        watched_text: Color::Rgb(37, 99, 200),
        watched_muted: Color::Rgb(140, 148, 168),
        watched_bg: Color::Rgb(240, 242, 250),
        unmerged: Color::Rgb(194, 100, 4),
        relocated_indicator: Color::Rgb(22, 140, 90),
        lost_indicator: Color::Rgb(190, 60, 60),

        syntect_theme: "base16-ocean.light".to_string(),
    }
}

pub fn high_contrast() -> Theme {
    Theme {
        name: "high-contrast".to_string(),

        bg: Color::Rgb(0, 0, 0),
        surface: Color::Rgb(10, 10, 10),
        panel: Color::Rgb(20, 20, 20),
        border: Color::Rgb(80, 80, 80),

        text: Color::Rgb(255, 255, 255),
        text_bright: Color::Rgb(255, 255, 255),
        text_dim: Color::Rgb(180, 180, 180),
        text_muted: Color::Rgb(120, 120, 120),

        blue: Color::Rgb(0, 120, 255),
        cyan: Color::Rgb(0, 230, 255),
        green: Color::Rgb(0, 255, 80),
        yellow: Color::Rgb(255, 220, 0),
        red: Color::Rgb(255, 50, 50),
        purple: Color::Rgb(190, 130, 255),
        orange: Color::Rgb(255, 140, 0),

        add_bg: Color::Rgb(0, 40, 20),
        add_text: Color::Rgb(0, 255, 80),
        del_bg: Color::Rgb(50, 0, 0),
        del_text: Color::Rgb(255, 50, 50),
        hunk_bg: Color::Rgb(0, 0, 40),

        line_cursor_bg: Color::Rgb(40, 20, 60),
        selected_bg: Color::Rgb(30, 20, 50),
        finding_bg: Color::Rgb(40, 28, 0),
        finding_focus_bg: Color::Rgb(60, 42, 0),
        comment_bg: Color::Rgb(0, 20, 40),
        inline_comment_bg: Color::Rgb(0, 25, 48),
        comment_focus_bg: Color::Rgb(0, 45, 80),

        stale: Color::Rgb(220, 200, 0),
        watched_text: Color::Rgb(100, 180, 255),
        watched_muted: Color::Rgb(80, 100, 130),
        watched_bg: Color::Rgb(5, 8, 15),
        unmerged: Color::Rgb(255, 140, 0),
        relocated_indicator: Color::Rgb(0, 230, 130),
        lost_indicator: Color::Rgb(220, 80, 80),

        syntect_theme: "base16-ocean.dark".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_presets_construct() {
        let themes = [ocean_depth(), moonlight(), daybreak(), high_contrast()];
        let expected_names = ["ocean-depth", "moonlight", "daybreak", "high-contrast"];
        for (theme, expected) in themes.iter().zip(expected_names.iter()) {
            assert_eq!(theme.name, *expected);
        }
    }

    #[test]
    fn available_themes_returns_all_four() {
        let names = available_themes();
        assert_eq!(names.len(), 4);
        assert!(names.contains(&"ocean-depth"));
        assert!(names.contains(&"moonlight"));
        assert!(names.contains(&"daybreak"));
        assert!(names.contains(&"high-contrast"));
    }

    #[test]
    fn theme_by_name_returns_correct_theme() {
        for name in available_themes() {
            let theme = theme_by_name(name).expect("known theme should resolve");
            assert_eq!(theme.name, name);
        }
    }

    #[test]
    fn theme_by_name_unknown_returns_none() {
        assert!(theme_by_name("nonexistent").is_none());
    }

    #[test]
    fn set_and_get_roundtrip() {
        // Set to daybreak and verify the name comes back correctly.
        // Note: static state is shared across tests; use a distinct value.
        set_theme(daybreak());
        assert_eq!(current().name, "daybreak");

        set_theme_by_name("high-contrast");
        assert_eq!(current().name, "high-contrast");

        // Restore default so other tests are unaffected.
        set_theme_by_name("ocean-depth");
        assert_eq!(current().name, "ocean-depth");
    }

    #[test]
    fn ocean_depth_matches_styles_rs_constants() {
        let t = ocean_depth();
        assert_eq!(t.bg, Color::Rgb(11, 11, 15));
        assert_eq!(t.surface, Color::Rgb(19, 19, 26));
        assert_eq!(t.panel, Color::Rgb(26, 26, 36));
        assert_eq!(t.border, Color::Rgb(42, 42, 58));
        assert_eq!(t.text, Color::Rgb(228, 228, 239));
        assert_eq!(t.text_bright, Color::Rgb(232, 232, 242));
        assert_eq!(t.text_dim, Color::Rgb(136, 136, 160));
        assert_eq!(t.text_muted, Color::Rgb(85, 85, 106));
        assert_eq!(t.blue, Color::Rgb(96, 165, 250));
        assert_eq!(t.cyan, Color::Rgb(34, 211, 238));
        assert_eq!(t.green, Color::Rgb(74, 222, 128));
        assert_eq!(t.yellow, Color::Rgb(250, 204, 21));
        assert_eq!(t.red, Color::Rgb(248, 113, 113));
        assert_eq!(t.purple, Color::Rgb(167, 139, 250));
        assert_eq!(t.orange, Color::Rgb(251, 146, 60));
        assert_eq!(t.add_bg, Color::Rgb(16, 36, 28));
        assert_eq!(t.add_text, Color::Rgb(74, 222, 128));
        assert_eq!(t.del_bg, Color::Rgb(42, 16, 22));
        assert_eq!(t.del_text, Color::Rgb(248, 113, 113));
        assert_eq!(t.hunk_bg, Color::Rgb(22, 22, 42));
        assert_eq!(t.line_cursor_bg, Color::Rgb(36, 28, 52));
        assert_eq!(t.selected_bg, Color::Rgb(30, 24, 48));
        assert_eq!(t.finding_bg, Color::Rgb(36, 28, 18));
        assert_eq!(t.finding_focus_bg, Color::Rgb(50, 38, 22));
        assert_eq!(t.comment_bg, Color::Rgb(18, 28, 38));
        assert_eq!(t.inline_comment_bg, Color::Rgb(22, 32, 42));
        assert_eq!(t.comment_focus_bg, Color::Rgb(35, 50, 70));
        assert_eq!(t.stale, Color::Rgb(180, 160, 40));
        assert_eq!(t.watched_text, Color::Rgb(120, 160, 220));
        assert_eq!(t.watched_muted, Color::Rgb(70, 85, 110));
        assert_eq!(t.watched_bg, Color::Rgb(14, 16, 24));
        assert_eq!(t.unmerged, Color::Rgb(255, 140, 0));
        assert_eq!(t.relocated_indicator, Color::Rgb(100, 200, 150));
        assert_eq!(t.lost_indicator, Color::Rgb(180, 100, 100));
        assert_eq!(t.syntect_theme, "base16-ocean.dark");
    }
}
