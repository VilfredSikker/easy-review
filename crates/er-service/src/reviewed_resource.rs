//! `reviewed(tab)`: one tab's reviewed marks and counts, and the view they
//! were read in, built from the content the workspace published for it.
//! The view comparison a keyed write runs lives here too, so every write
//! that carries a painted view compares it with the content the same way.

use er_engine::file_rows::mode_str;
use er_protocol as wire;

use crate::content::TabContent;

/// The version `reviewed(tab)` answers at. The counts read the diff's file
/// list beside the marks, and the view reads the recorded mode, so a reload
/// or a mode switch moves it as a reviewed write does.
pub fn reviewed_version(content: &TabContent) -> u64 {
    content
        .diff
        .revision
        .max(content.reviewed.revision)
        .max(content.mode_revision)
}

/// `content`'s marks among its diff's files, counted on the server.
pub fn reviewed_resource(content: &TabContent) -> wire::TabReviewed {
    let marks = &content.reviewed.marks;
    let mut paths: Vec<String> = content
        .diff
        .files
        .iter()
        .filter(|file| marks.contains_key(&file.path))
        .map(|file| file.path.clone())
        .collect();
    paths.sort();
    wire::TabReviewed {
        reviewed_count: paths.len() as u64,
        total_count: content.diff.files.len() as u64,
        paths,
        view: content_view(content),
    }
}

/// The view `content` shows, as a page paints it.
pub fn content_view(content: &TabContent) -> wire::PaintedView {
    let tab = &content.tab;
    wire::PaintedView {
        repo_root: tab.repo_root.clone(),
        pr_number: tab.pr_number,
        branch: tab.local_branch_view.clone().unwrap_or_default(),
        mode: mode_str(tab.mode).to_string(),
    }
}

/// Whether the page painted `view` on the view `content` shows. A write
/// painted on another view is refused, since what it names (a pillar, a
/// line) belongs to the view the page showed.
pub fn painted_view_matches(content: &TabContent, view: &wire::PaintedView) -> bool {
    content_view(content) == *view
}
