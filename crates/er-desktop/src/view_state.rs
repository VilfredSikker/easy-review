//! View state the desktop alone reads and writes: each tab's review browser
//! and which side panels are open. It left the engine's tab and app state
//! because no other front end uses it (ADR 0068).
//!
//! Browser state is keyed on the tab's `TabKey`, so it follows the tab through
//! reorder and close, and two tabs on one repo keep their own. `tabs.rs` copies
//! the URL and layout in and out of the saved tab list; this module writes
//! nothing to disk.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use er_engine::TabKey;

/// Shared handle in `AppState`. Lock order: the engine lock first, then this
/// one. Release it before building a snapshot, which takes it again.
pub type ViewStateHandle = Arc<Mutex<DesktopViewState>>;

#[derive(Debug, Default)]
pub struct DesktopViewState {
    panels: PanelsVisible,
    tabs: HashMap<TabKey, TabBrowserState>,
    /// Selection and scroll on the service path, keyed by the app registry's
    /// tab keys. The default build keeps these on the engine tab until the
    /// TUI's view state moves too, so it never writes here.
    nav: HashMap<TabKey, TabNavState>,
}

impl DesktopViewState {
    pub fn panels(&self) -> &PanelsVisible {
        &self.panels
    }

    pub fn toggle_panel(&mut self, name: &str) {
        let panels = &mut self.panels;
        match name {
            "left" => panels.left = !panels.left,
            "tree" => panels.tree = !panels.tree,
            "right" => panels.right = !panels.right,
            _ => {}
        }
    }

    /// The tab's browser state; a tab that never set one gets the defaults.
    pub fn browser(&self, key: TabKey) -> TabBrowserState {
        self.tabs.get(&key).cloned().unwrap_or_default()
    }

    pub fn browser_mut(&mut self, key: TabKey) -> &mut TabBrowserState {
        self.tabs.entry(key).or_default()
    }

    /// Drop the entries of tabs that are gone. Nav entries go too: on the flag
    /// path every tab carries the key the app registry issued, so both maps
    /// share one key space.
    pub fn retain_tabs(&mut self, live: &[TabKey]) {
        self.tabs.retain(|key, _| live.contains(key));
        self.nav.retain(|key, _| live.contains(key));
    }

    /// The tab's selection and scroll; a tab that never moved starts at the top.
    pub fn nav(&self, key: TabKey) -> TabNavState {
        self.nav.get(&key).copied().unwrap_or_default()
    }

    pub fn nav_mut(&mut self, key: TabKey) -> &mut TabNavState {
        self.nav.entry(key).or_default()
    }
}

/// One tab's selection and scroll positions. `selected_file` indexes the
/// tab's diff files, the same index the snapshot translates into a position
/// in the visible list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabNavState {
    pub selected_file: usize,
    pub current_hunk: usize,
    pub current_line: Option<usize>,
    pub diff_scroll: u16,
    pub h_scroll: u16,
    /// The split view's two columns scroll on their own.
    pub h_scroll_old: u16,
    pub h_scroll_new: u16,
    pub panel_scroll: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelsVisible {
    pub left: bool,
    pub tree: bool,
    pub right: bool,
}

impl Default for PanelsVisible {
    fn default() -> Self {
        Self {
            left: true,
            tree: true,
            right: true,
        }
    }
}

/// One tab's review browser.
#[derive(Debug, Clone, PartialEq)]
pub struct TabBrowserState {
    pub url: String,
    pub layout: BrowserLayout,
    /// Diff column fraction of a split, clamped 0.35..0.65.
    pub split_ratio: f32,
    pub annotate_mode: bool,
    pub show_tooltips: bool,
}

impl Default for TabBrowserState {
    fn default() -> Self {
        Self {
            url: String::new(),
            layout: BrowserLayout::Hidden,
            split_ratio: 0.45,
            annotate_mode: false,
            show_tooltips: false,
        }
    }
}

/// The fields `update_tab_browser` may set; `None` leaves a field alone.
#[derive(Debug, Default)]
pub struct BrowserUpdate {
    pub layout: Option<String>,
    pub url: Option<String>,
    pub annotate: Option<bool>,
    pub tooltips: Option<bool>,
    pub split_ratio: Option<f32>,
}

impl TabBrowserState {
    pub fn apply(&mut self, update: BrowserUpdate) {
        if let Some(l) = update.layout.as_deref() {
            self.layout = BrowserLayout::from_label(l);
        }
        if let Some(u) = update.url {
            self.url = u;
        }
        if let Some(a) = update.annotate {
            self.annotate_mode = a;
        }
        if let Some(t) = update.tooltips {
            self.show_tooltips = t;
        }
        if let Some(r) = update.split_ratio {
            self.split_ratio = r.clamp(0.35, 0.65);
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BrowserLayout {
    #[default]
    Hidden,
    Split,
    Fullscreen,
}

impl BrowserLayout {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Split => "split",
            Self::Fullscreen => "fullscreen",
        }
    }

    pub fn from_label(s: &str) -> Self {
        match s {
            "split" => Self::Split,
            "fullscreen" => Self::Fullscreen,
            _ => Self::Hidden,
        }
    }

    #[must_use]
    pub const fn cycle(self) -> Self {
        match self {
            Self::Hidden => Self::Split,
            Self::Split => Self::Fullscreen,
            Self::Fullscreen => Self::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The production half of a source file, without its test module.
    fn production(src: &str) -> &str {
        src.split("\n#[cfg(test)]\nmod tests").next().unwrap_or(src)
    }

    /// Whole-identifier match, so `App` does not match `AppState`.
    fn has_ident(src: &str, ident: &str) -> bool {
        let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
        src.match_indices(ident).any(|(at, _)| {
            let before = src.get(..at).and_then(|s| s.chars().next_back());
            let after = src.get(at + ident.len()..).and_then(|s| s.chars().next());
            !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
        })
    }

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// The six fields this module owns, and the two types that moved with them.
    const VIEW_FIELDS: [&str; 6] = [
        "browser_url",
        "browser_layout",
        "browser_split_ratio",
        "browser_annotate_mode",
        "browser_show_tooltips",
        "panels_visible",
    ];
    const VIEW_TYPES: [&str; 2] = ["BrowserLayout", "PanelsVisible"];

    #[test]
    fn the_engine_state_has_no_desktop_view_fields() {
        let engine = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../er-engine/src");
        let mut files = Vec::new();
        rust_files(&engine, &mut files);
        for path in &files {
            let src = std::fs::read_to_string(path).expect("read engine source");
            for ident in VIEW_FIELDS.iter().chain(&VIEW_TYPES) {
                assert!(
                    !has_ident(production(&src), ident),
                    "{} still names desktop view state `{ident}`",
                    path.display()
                );
            }
        }
        // D20 keeps the TUI layout group in the engine until phase 5b.
        let state = std::fs::read_to_string(engine.join("app/state/mod.rs")).expect("mod.rs");
        for kept in ["file_tree_width", "panel_width"] {
            assert!(
                has_ident(production(&state), kept),
                "`{kept}` stays on the engine state"
            );
        }
    }

    #[test]
    fn the_service_does_not_read_view_state() {
        let service = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../er-service/src");
        let mut files = Vec::new();
        rust_files(&service, &mut files);
        assert!(!files.is_empty(), "er-service sources found");
        let moved = ["DesktopViewState", "TabBrowserState", "view_state"];
        for path in &files {
            let src = std::fs::read_to_string(path).expect("read service source");
            for ident in VIEW_FIELDS.iter().chain(&VIEW_TYPES).chain(&moved) {
                assert!(
                    !has_ident(&src, ident),
                    "{} reads desktop view state `{ident}`",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn the_view_state_module_writes_no_domain_state() {
        let src = production(include_str!("view_state.rs"));
        for ident in ["App", "store_registry", "er_store", "storage", "TabState"] {
            assert!(
                !has_ident(src, ident),
                "view_state.rs must not touch domain state (`{ident}`)"
            );
        }
        for prefix in ["save_", "persist_", "write("] {
            assert!(
                !src.contains(prefix),
                "view_state.rs must not write to disk (`{prefix}`)"
            );
        }
    }

    #[test]
    fn toggle_panel_updates_the_view_state() {
        let mut view = DesktopViewState::default();
        view.toggle_panel("tree");
        assert!(!view.panels().tree);
        assert!(view.panels().left && view.panels().right);
        view.toggle_panel("tree");
        assert!(view.panels().tree);
    }

    #[test]
    fn a_browser_update_sets_only_the_given_fields() {
        let mut browser = TabBrowserState::default();
        browser.apply(BrowserUpdate {
            layout: Some("split".into()),
            url: Some("http://localhost:5173/".into()),
            split_ratio: Some(0.9),
            ..BrowserUpdate::default()
        });
        assert_eq!(browser.layout, BrowserLayout::Split);
        assert_eq!(browser.url, "http://localhost:5173/");
        assert!(
            (browser.split_ratio - 0.65).abs() < f32::EPSILON,
            "the ratio is clamped"
        );
        browser.apply(BrowserUpdate {
            annotate: Some(true),
            tooltips: Some(true),
            ..BrowserUpdate::default()
        });
        assert!(browser.annotate_mode && browser.show_tooltips);
        assert_eq!(browser.url, "http://localhost:5173/", "untouched");
    }

    /// Each tab keeps its own selection, and a tab that never moved starts
    /// at the top.
    #[test]
    fn nav_state_is_kept_per_tab_key() {
        let (a, b) = (TabKey::next(), TabKey::next());
        let mut view = DesktopViewState::default();
        view.nav_mut(a).selected_file = 3;
        view.nav_mut(a).current_hunk = 2;
        assert_eq!(view.nav(a).selected_file, 3);
        assert_eq!(view.nav(a).current_hunk, 2);
        assert_eq!(view.nav(b), TabNavState::default());
    }

    #[test]
    fn retain_tabs_drops_closed_tabs_only() {
        let (a, b) = (TabKey::next(), TabKey::next());
        let mut view = DesktopViewState::default();
        view.browser_mut(a).url = "a".into();
        view.browser_mut(b).url = "b".into();
        view.nav_mut(a).selected_file = 2;
        view.nav_mut(b).selected_file = 5;
        view.retain_tabs(&[b]);
        assert_eq!(view.browser(a), TabBrowserState::default());
        assert_eq!(view.browser(b).url, "b");
        assert_eq!(view.nav(a), TabNavState::default());
        assert_eq!(view.nav(b).selected_file, 5);
    }
}
