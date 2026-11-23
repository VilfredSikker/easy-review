//! The review export over the App's tab, for the TUI and the App path. The
//! renderer and its options live in [`crate::review_export`], which compiles
//! without the App.

use crate::app::TabState;
pub use crate::review_export::ExportOpts;
use crate::review_export::{render, ExportSource};

/// Render the tab's annotations as a single markdown document.
pub fn render_markdown(tab: &TabState, opts: &ExportOpts) -> String {
    let er_dir = tab.er_dir();
    let source = ExportSource {
        branch: &tab.current_branch,
        ai: &tab.ai,
        er_dir: &er_dir,
    };
    render(&source, opts)
}
