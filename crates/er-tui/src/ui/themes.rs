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

// ─────────────────────────────────────────────────────────────────────────
// Token system — mirrors the design source of truth (`theme-tokens.js`).
//
// Each theme is a pure data swap of role tokens; every interactive/status
// color the TUI needs that isn't a raw design token is *derived* from the
// tokens below (alpha-composited over the canvas), so adding or editing a
// theme touches data alone — matching the design's token philosophy.
// ─────────────────────────────────────────────────────────────────────────

/// Raw role tokens for one theme. Hex strings mirror `theme-tokens.js`.
/// `line2` may carry an 8-digit alpha suffix (e.g. `ffffff26`).
struct Tokens {
    name: &'static str,
    bg: &'static str,
    bg1: &'static str,
    bg2: &'static str,
    bg3: &'static str,
    line2: &'static str,
    tx: &'static str,
    tx2: &'static str,
    tx3: &'static str,
    accent: &'static str,
    red: &'static str,
    amber: &'static str,
    blue: &'static str,
    cyan: &'static str,
    purple: &'static str,
    green: &'static str,
    add: &'static str,
    del: &'static str,
    syntect: &'static str,
}

type Rgb = (u8, u8, u8);

fn hex(s: &str) -> Rgb {
    let s = s.trim_start_matches('#');
    let r = u8::from_str_radix(s.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
    let g = u8::from_str_radix(s.get(2..4).unwrap_or("00"), 16).unwrap_or(0);
    let b = u8::from_str_radix(s.get(4..6).unwrap_or("00"), 16).unwrap_or(0);
    (r, g, b)
}

/// Parse a hex color with optional 8-digit alpha; returns (rgb, alpha 0..1).
fn hexa(s: &str) -> (Rgb, f32) {
    let trimmed = s.trim_start_matches('#');
    let a = if trimmed.len() >= 8 {
        u8::from_str_radix(&trimmed[6..8], 16).unwrap_or(255) as f32 / 255.0
    } else {
        1.0
    };
    (hex(trimmed), a)
}

fn col(rgb: Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

/// Composite `fg` at opacity `a` over opaque `bg`.
fn over(fg: Rgb, a: f32, bg: Rgb) -> Rgb {
    let f = |x: u8, y: u8| (x as f32 * a + y as f32 * (1.0 - a)).round() as u8;
    (f(fg.0, bg.0), f(fg.1, bg.1), f(fg.2, bg.2))
}

/// Linear blend; t=0 → a, t=1 → b.
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

fn build(t: &Tokens) -> Theme {
    let bg = hex(t.bg);
    let bg1 = hex(t.bg1);
    let bg2 = hex(t.bg2);
    let bg3 = hex(t.bg3);
    let tx = hex(t.tx);
    let tx2 = hex(t.tx2);
    let tx3 = hex(t.tx3);
    let accent = hex(t.accent);
    let red = hex(t.red);
    let amber = hex(t.amber);
    let blue = hex(t.blue);
    let cyan = hex(t.cyan);
    let purple = hex(t.purple);
    let green = hex(t.green);
    let add = hex(t.add);
    let del = hex(t.del);
    let (line2_rgb, line2_a) = hexa(t.line2);

    Theme {
        name: t.name.to_string(),

        bg: col(bg),
        surface: col(bg1),
        panel: col(bg2),
        border: col(over(line2_rgb, line2_a, bg)),

        text: col(tx),
        text_bright: col(tx),
        text_dim: col(tx2),
        text_muted: col(tx3),

        blue: col(blue),
        cyan: col(cyan),
        green: col(green),
        yellow: col(amber),
        red: col(red),
        purple: col(purple),
        orange: col(accent),

        add_bg: col(over(add, 0.15, bg)),
        add_text: col(add),
        del_bg: col(over(del, 0.15, bg)),
        del_text: col(del),
        hunk_bg: col(over(blue, 0.10, bg)),

        line_cursor_bg: col(over(accent, 0.14, bg)),
        selected_bg: col(bg3),
        finding_bg: col(over(amber, 0.10, bg)),
        finding_focus_bg: col(over(amber, 0.18, bg)),
        comment_bg: col(over(cyan, 0.08, bg)),
        inline_comment_bg: col(over(cyan, 0.11, bg)),
        comment_focus_bg: col(over(cyan, 0.20, bg)),

        stale: col(amber),
        watched_text: col(blue),
        watched_muted: col(mix(tx3, blue, 0.35)),
        watched_bg: col(bg1),
        unmerged: col(accent),
        relocated_indicator: col(green),
        lost_indicator: col(red),

        syntect_theme: t.syntect.to_string(),
    }
}

static CURRENT_THEME: OnceLock<RwLock<Theme>> = OnceLock::new();

pub fn current() -> RwLockReadGuard<'static, Theme> {
    CURRENT_THEME
        .get_or_init(|| RwLock::new(graphite()))
        .read()
        .unwrap()
}

pub fn set_theme(theme: Theme) {
    let lock = CURRENT_THEME.get_or_init(|| RwLock::new(graphite()));
    *lock.write().unwrap() = theme;
}

pub fn set_theme_by_name(name: &str) {
    if let Some(theme) = theme_by_name(name) {
        set_theme(theme);
    } else {
        eprintln!("er: unknown theme {name:?}, using graphite");
        set_theme(graphite());
    }
}

pub fn theme_by_name(name: &str) -> Option<Theme> {
    match name {
        "graphite" => Some(graphite()),
        "slate" => Some(slate()),
        "midnight" => Some(midnight()),
        "ember" => Some(ember()),
        "paper" => Some(paper()),
        "daylight" => Some(daylight()),
        "contrast-dark" => Some(contrast_dark()),
        "contrast-light" => Some(contrast_light()),

        // Backward-compat aliases for the retired theme set.
        "ocean-depth" => Some(graphite()),
        "moonlight" => Some(slate()),
        "high-contrast" => Some(contrast_dark()),
        "daybreak" => Some(daylight()),
        "tokyo-night" | "tokyo-night-storm" | "tokyo-night-moon" => Some(midnight()),
        "tokyo-night-day" => Some(paper()),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn available_themes() -> Vec<&'static str> {
    vec![
        "graphite",
        "slate",
        "midnight",
        "ember",
        "paper",
        "daylight",
        "contrast-dark",
        "contrast-light",
    ]
}

// ── DARK ──────────────────────────────────────────────────────────────────

/// Graphite — neutral near-black. The signature default.
pub fn graphite() -> Theme {
    build(&Tokens {
        name: "graphite",
        bg: "#0b0b0d",
        bg1: "#16161a",
        bg2: "#1d1d22",
        bg3: "#28282f",
        line2: "#ffffff26",
        tx: "#ededf0",
        tx2: "#a1a1ab",
        tx3: "#6b6b75",
        accent: "#f2843c",
        red: "#ef5f5b",
        amber: "#e3b341",
        blue: "#5f9cea",
        cyan: "#4cc4e0",
        purple: "#a78bf6",
        green: "#46bd6c",
        add: "#46bd6c",
        del: "#ef5f5b",
        syntect: "OneHalfDark",
    })
}

/// Slate — cool blue-grey. Calm, high legibility.
pub fn slate() -> Theme {
    build(&Tokens {
        name: "slate",
        bg: "#0c1118",
        bg1: "#131b25",
        bg2: "#1a2431",
        bg3: "#243140",
        line2: "#9fc0ff2e",
        tx: "#e9eef5",
        tx2: "#9aa9bb",
        tx3: "#647588",
        accent: "#f2843c",
        red: "#f0635f",
        amber: "#e6b84a",
        blue: "#6aa6f5",
        cyan: "#43c8e2",
        purple: "#ab92f7",
        green: "#4ec977",
        add: "#4ec977",
        del: "#f0635f",
        syntect: "OneHalfDark",
    })
}

/// Midnight — deep indigo. Keeps the loved Tokyo palette for semantics.
pub fn midnight() -> Theme {
    build(&Tokens {
        name: "midnight",
        bg: "#0e0f1a",
        bg1: "#171829",
        bg2: "#1e2036",
        bg3: "#292c48",
        line2: "#a9b8ff2e",
        tx: "#e6e8f5",
        tx2: "#9aa0c4",
        tx3: "#666c90",
        accent: "#f2843c",
        red: "#f7768e",
        amber: "#e0af68",
        blue: "#7aa2f7",
        cyan: "#7dcfff",
        purple: "#bb9af7",
        green: "#9ece6a",
        add: "#9ece6a",
        del: "#f7768e",
        syntect: "OneHalfDark",
    })
}

/// Ember — warm charcoal. Easy on the eyes at night.
pub fn ember() -> Theme {
    build(&Tokens {
        name: "ember",
        bg: "#100c0a",
        bg1: "#1b1512",
        bg2: "#241c17",
        bg3: "#312620",
        line2: "#ffd9b82b",
        tx: "#f1e9e2",
        tx2: "#b09c8d",
        tx3: "#75665c",
        accent: "#f2843c",
        red: "#ef6151",
        amber: "#e7b24a",
        blue: "#7aa6dd",
        cyan: "#56bfc0",
        purple: "#c193e8",
        green: "#76b95f",
        add: "#76b95f",
        del: "#ef6151",
        syntect: "OneHalfDark",
    })
}

// ── LIGHT ─────────────────────────────────────────────────────────────────

/// Paper — a genuinely warm white, no blue cast.
pub fn paper() -> Theme {
    build(&Tokens {
        name: "paper",
        bg: "#faf8f4",
        bg1: "#ffffff",
        bg2: "#f3efe8",
        bg3: "#e9e3d8",
        line2: "#3a2a1a2b",
        tx: "#211d18",
        tx2: "#6b6258",
        tx3: "#9a9085",
        accent: "#cf5f17",
        red: "#cc3b39",
        amber: "#a9740f",
        blue: "#2f6fd0",
        cyan: "#0e87a3",
        purple: "#7a4fd0",
        green: "#1c854b",
        add: "#1c854b",
        del: "#cc3b39",
        syntect: "base16-ocean.light",
    })
}

/// Daylight — crisp cool light, kept high-contrast.
pub fn daylight() -> Theme {
    build(&Tokens {
        name: "daylight",
        bg: "#f6f7f9",
        bg1: "#ffffff",
        bg2: "#eef0f3",
        bg3: "#e3e7ec",
        line2: "#0b1b3a29",
        tx: "#161a21",
        tx2: "#5b6573",
        tx3: "#8b95a3",
        accent: "#cf5f17",
        red: "#cc3b39",
        amber: "#9a6b12",
        blue: "#2563cf",
        cyan: "#0b7e9c",
        purple: "#6f45cc",
        green: "#168049",
        add: "#168049",
        del: "#cc3b39",
        syntect: "base16-ocean.light",
    })
}

// ── ACCESSIBILITY ───────────────────────────────────────────────────────────

/// Contrast Dark — pure black, AAA text, vivid semantics.
pub fn contrast_dark() -> Theme {
    build(&Tokens {
        name: "contrast-dark",
        bg: "#000000",
        bg1: "#0a0a0b",
        bg2: "#151517",
        bg3: "#202024",
        line2: "#ffffff5c",
        tx: "#ffffff",
        tx2: "#d4d4da",
        tx3: "#9a9aa2",
        accent: "#ff9a4d",
        red: "#ff6b66",
        amber: "#ffcf4d",
        blue: "#7fb4ff",
        cyan: "#5fd6f0",
        purple: "#cbabff",
        green: "#5fe08a",
        add: "#5fe08a",
        del: "#ff6b66",
        syntect: "OneHalfDark",
    })
}

/// Contrast Light — pure white, near-black ink, AAA.
pub fn contrast_light() -> Theme {
    build(&Tokens {
        name: "contrast-light",
        bg: "#ffffff",
        bg1: "#ffffff",
        bg2: "#f2f2f4",
        bg3: "#e6e6ea",
        line2: "#0000005c",
        tx: "#000000",
        tx2: "#2e2e33",
        tx3: "#5a5a61",
        accent: "#b8530c",
        red: "#bf1b1b",
        amber: "#7a5300",
        blue: "#1551c4",
        cyan: "#056b85",
        purple: "#6321c0",
        green: "#0c7a3f",
        add: "#0c7a3f",
        del: "#bf1b1b",
        syntect: "base16-ocean.light",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_presets_construct() {
        let themes = [
            graphite(),
            slate(),
            midnight(),
            ember(),
            paper(),
            daylight(),
            contrast_dark(),
            contrast_light(),
        ];
        let expected_names = [
            "graphite",
            "slate",
            "midnight",
            "ember",
            "paper",
            "daylight",
            "contrast-dark",
            "contrast-light",
        ];
        for (theme, expected) in themes.iter().zip(expected_names.iter()) {
            assert_eq!(theme.name, *expected);
        }
    }

    #[test]
    fn available_themes_returns_all() {
        let names = available_themes();
        assert_eq!(names.len(), 8);
        assert!(names.contains(&"graphite"));
        assert!(names.contains(&"slate"));
        assert!(names.contains(&"midnight"));
        assert!(names.contains(&"ember"));
        assert!(names.contains(&"paper"));
        assert!(names.contains(&"daylight"));
        assert!(names.contains(&"contrast-dark"));
        assert!(names.contains(&"contrast-light"));
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
    fn legacy_theme_names_alias_to_new_themes() {
        // Old config values must keep resolving after the rebrand.
        assert_eq!(theme_by_name("ocean-depth").unwrap().name, "graphite");
        assert_eq!(theme_by_name("moonlight").unwrap().name, "slate");
        assert_eq!(
            theme_by_name("high-contrast").unwrap().name,
            "contrast-dark"
        );
        assert_eq!(theme_by_name("daybreak").unwrap().name, "daylight");
        assert_eq!(theme_by_name("tokyo-night").unwrap().name, "midnight");
        assert_eq!(theme_by_name("tokyo-night-day").unwrap().name, "paper");
    }

    #[test]
    fn set_and_get_roundtrip() {
        // Static state is shared across tests; use distinct values.
        set_theme(daylight());
        assert_eq!(current().name, "daylight");

        set_theme_by_name("contrast-dark");
        assert_eq!(current().name, "contrast-dark");

        // Restore default so other tests are unaffected.
        set_theme_by_name("graphite");
        assert_eq!(current().name, "graphite");
    }

    #[test]
    fn graphite_token_derivation() {
        let t = graphite();
        // Direct tokens.
        assert_eq!(t.bg, Color::Rgb(0x0b, 0x0b, 0x0d));
        assert_eq!(t.surface, Color::Rgb(0x16, 0x16, 0x1a));
        assert_eq!(t.panel, Color::Rgb(0x1d, 0x1d, 0x22));
        assert_eq!(t.text, Color::Rgb(0xed, 0xed, 0xf0));
        assert_eq!(t.text_dim, Color::Rgb(0xa1, 0xa1, 0xab));
        assert_eq!(t.text_muted, Color::Rgb(0x6b, 0x6b, 0x75));
        assert_eq!(t.orange, Color::Rgb(0xf2, 0x84, 0x3c));
        assert_eq!(t.blue, Color::Rgb(0x5f, 0x9c, 0xea));
        assert_eq!(t.green, Color::Rgb(0x46, 0xbd, 0x6c));
        assert_eq!(t.red, Color::Rgb(0xef, 0x5f, 0x5b));
        // Border = line2 (#ffffff26 = white @ 0.149) composited over bg.
        assert_eq!(t.border, Color::Rgb(47, 47, 49));
        assert_eq!(t.selected_bg, Color::Rgb(0x28, 0x28, 0x2f)); // bg3
        assert_eq!(t.syntect_theme, "OneHalfDark");
    }
}
