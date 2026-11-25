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

/// Shared handle in `DesktopState`.
pub type ViewStateHandle = Arc<Mutex<DesktopViewState>>;

#[derive(Debug, Default)]
pub struct DesktopViewState {
    panels: PanelsVisible,
    /// Rises with each toggle, so `panels` has a version to push.
    panels_revision: u64,
    tabs: HashMap<TabKey, TabBrowserState>,
    /// The browser write clock, and the tick each tab's browser state was last
    /// written at, so `view(tab)` has a version per tab that never repeats.
    browser_clock: u64,
    browser_revisions: HashMap<TabKey, u64>,
    /// Each tab's file filter on the service path, and the clock its writes
    /// move, which `diff(tab)`'s version adds so a filter change moves it.
    filters: HashMap<TabKey, TabFilter>,
    filter_clock: u64,
}

impl DesktopViewState {
    pub fn panels(&self) -> &PanelsVisible {
        &self.panels
    }

    pub fn panels_revision(&self) -> u64 {
        self.panels_revision
    }

    pub fn toggle_panel(&mut self, name: &str) {
        let panels = &mut self.panels;
        match name {
            "left" => panels.left = !panels.left,
            "tree" => panels.tree = !panels.tree,
            "right" => panels.right = !panels.right,
            _ => return,
        }
        self.panels_revision += 1;
    }

    /// The tab's browser state; a tab that never set one gets the defaults.
    pub fn browser(&self, key: TabKey) -> TabBrowserState {
        self.tabs.get(&key).cloned().unwrap_or_default()
    }

    pub fn browser_mut(&mut self, key: TabKey) -> &mut TabBrowserState {
        self.browser_clock += 1;
        self.browser_revisions.insert(key, self.browser_clock);
        self.tabs.entry(key).or_default()
    }

    /// The tick `key`'s browser state was last written at; 0 when never.
    pub fn browser_revision(&self, key: TabKey) -> u64 {
        self.browser_revisions.get(&key).copied().unwrap_or(0)
    }

    pub fn browser_revisions(&self) -> impl Iterator<Item = (TabKey, u64)> + '_ {
        self.browser_revisions
            .iter()
            .map(|(&key, &tick)| (key, tick))
    }

    /// Drop the entries of tabs that are gone. Nav entries go too: on the flag
    /// path every tab carries the key the app registry issued, so both maps
    /// share one key space.
    pub fn retain_tabs(&mut self, live: &[TabKey]) {
        self.tabs.retain(|key, _| live.contains(key));
        self.browser_revisions.retain(|key, _| live.contains(key));
        self.filters.retain(|key, _| live.contains(key));
    }

    /// The tab's filter; a tab that never set one lists every file.
    pub fn filter(&self, key: TabKey) -> TabFilter {
        self.filters.get(&key).cloned().unwrap_or_default()
    }

    /// Filters `key`'s list by `expr`, or clears the filter when it is
    /// blank. A new expression goes to the front of the tab's recent ones, as
    /// the engine's filter does. Answers the tick the write took.
    pub fn set_filter(&mut self, key: TabKey, expr: &str) -> u64 {
        let expr = expr.trim();
        self.filter_clock += 1;
        let filter = self.filters.entry(key).or_default();
        if !expr.is_empty() {
            filter.history.retain(|recent| recent != expr);
            filter.history.insert(0, expr.to_string());
            filter.history.truncate(FILTER_HISTORY);
        }
        filter.expr = expr.to_string();
        filter.tick = self.filter_clock;
        filter.tick
    }
}

/// How many recent filters a tab keeps.
const FILTER_HISTORY: usize = 20;

/// One tab's file filter and its recent expressions, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabFilter {
    pub expr: String,
    pub history: Vec<String>,
    /// The filter clock's tick at this tab's last filter write; 0 when never.
    pub tick: u64,
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
        // D20's TUI layout group left the engine with the App; it is in
        // er-tui's `TuiView` now.
        for path in &files {
            let src = std::fs::read_to_string(path).expect("read engine source");
            for gone in ["file_tree_width", "panel_width"] {
                assert!(
                    !has_ident(production(&src), gone),
                    "{} still names the TUI layout field `{gone}`",
                    path.display()
                );
            }
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

    #[test]
    fn retain_tabs_drops_closed_tabs_only() {
        let (a, b) = (TabKey::next(), TabKey::next());
        let mut view = DesktopViewState::default();
        view.browser_mut(a).url = "a".into();
        view.browser_mut(b).url = "b".into();
        view.retain_tabs(&[b]);
        assert_eq!(view.browser(a), TabBrowserState::default());
        assert_eq!(view.browser(b).url, "b");
    }
}
