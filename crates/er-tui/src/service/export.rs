//! The flag-on export picker (`E`): the App's four sections, rendered by the
//! engine's export renderer over the active tab's published AI state.

use er_engine::copy_text::Copied;
use er_engine::review_export::{render, ExportOpts, ExportSource};
use er_service::TabContent;

use super::tui_view::Overlay;
use crate::input::keys::OverlayAction;

pub enum ExportStep {
    Stay,
    Close,
    /// Nothing was selected: the picker opens afresh and says so.
    Reopen(&'static str),
    Copy(Copied),
}

pub const fn open() -> Overlay {
    Overlay::ExportPicker {
        include_comments: true,
        include_findings: true,
        include_questions: true,
        include_notes: true,
        selected: 0,
    }
}

pub fn act(overlay: &mut Overlay, action: OverlayAction, content: &TabContent) -> ExportStep {
    let Overlay::ExportPicker {
        include_comments,
        include_findings,
        include_questions,
        include_notes,
        selected,
    } = overlay
    else {
        return ExportStep::Close;
    };
    match action {
        OverlayAction::Next if *selected + 1 < 4 => *selected += 1,
        OverlayAction::Prev => *selected = selected.saturating_sub(1),
        OverlayAction::ToggleExportKind => {
            let row = match *selected {
                0 => include_comments,
                1 => include_findings,
                2 => include_questions,
                _ => include_notes,
            };
            *row = !*row;
        }
        OverlayAction::Close => return ExportStep::Close,
        OverlayAction::Export => {
            if !(*include_comments || *include_findings || *include_questions || *include_notes) {
                return ExportStep::Reopen("Select at least one section to export");
            }
            let opts = ExportOpts {
                include_comments: *include_comments,
                include_questions: *include_questions,
                include_notes: *include_notes,
                include_findings: *include_findings,
                only_unresolved: false,
                include_annotations: false,
                ..ExportOpts::default()
            };
            let source = ExportSource {
                branch: &content.tab.current_branch,
                ai: &content.ai.state,
                er_dir: &content.context.er_dir,
            };
            let text = render(&source, &opts);
            let notice = format!("Copied review export ({} chars)", text.len());
            return ExportStep::Copy(Copied { text, notice });
        }
        _ => {}
    }
    ExportStep::Stay
}
